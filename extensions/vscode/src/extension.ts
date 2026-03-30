import * as vscode from 'vscode'
import {
  DebuggerProcessConfig,
  LaunchPreflightIssue,
  LaunchPreflightQuickFix,
  validateLaunchConfig,
} from './cli/debuggerProcess'
import { SorobanDebugAdapterDescriptorFactory } from './debug/adapter'
import { LogManager } from './debug/logManager'
import {
  fromQuickPickLabel,
  runLaunchPreflightCommand,
  toQuickPickLabel,
} from './preflightCommand'
import { diagnoseBreakpoints } from './dap/sourceBreakpoints'
import { SorobanLaunchProgressReporter } from './launchProgress'

type SorobanLaunchConfig = vscode.DebugConfiguration & DebuggerProcessConfig
const RUN_LAUNCH_PREFLIGHT_COMMAND = 'soroban-debugger.runLaunchPreflight'
const DIAGNOSE_SOURCE_MAP_COMMAND = 'soroban-debugger.diagnoseSourceMap'

class SorobanDebugConfigurationProvider
  implements vscode.DebugConfigurationProvider
{
  async resolveDebugConfiguration(
    folder: vscode.WorkspaceFolder | undefined,
    config: SorobanLaunchConfig,
  ): Promise<vscode.DebugConfiguration | null | undefined> {
    if (!config.type && !config.request && !config.name) {
      return this.createDefaultLaunchConfig(folder)
    }

    if (config.type !== 'soroban') {
      return config
    }

    const settings = vscode.workspace.getConfiguration(
      'soroban-debugger',
      folder
    )
    // Apply workspace-level timeouts to both launch and attach configs
    config.requestTimeoutMs =
      config.requestTimeoutMs ?? settings.get<number>('requestTimeoutMs')
    config.connectTimeoutMs =
      config.connectTimeoutMs ?? settings.get<number>('connectTimeoutMs')

    if (config.request === 'launch') {
      const preflight = await validateLaunchConfig(config)
      if (preflight.ok) {
        return config
      }

      await showPreflightIssueAndApplyFix(preflight.issues[0], folder, config.name);
      return undefined
    }

    if (config.request === 'attach') {
      // Ensure attach-mode validation knows we will not spawn a server.
      (config as any).spawnServer = false
      const preflight = await validateLaunchConfig(config as any)
      if (preflight.ok) {
        return config
      }

      await showPreflightIssueAndApplyFix(preflight.issues[0], folder, config.name);
      return undefined
    }

    return config
  }

  private createDefaultLaunchConfig(
    folder: vscode.WorkspaceFolder | undefined
  ): vscode.DebugConfiguration {
    return createDefaultLaunchConfig(folder?.uri.fsPath ?? '${workspaceFolder}')
  }
}

let logManager: LogManager | undefined
let launchProgressReporter: SorobanLaunchProgressReporter | undefined

export function activate(context: vscode.ExtensionContext): void {
  logManager = new LogManager(context)
  launchProgressReporter = new SorobanLaunchProgressReporter()
  const factory = new SorobanDebugAdapterDescriptorFactory(
    context,
    logManager,
    launchProgressReporter
  )
  const configurationProvider = new SorobanDebugConfigurationProvider()

  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory('soroban', factory),
    vscode.debug.registerDebugConfigurationProvider(
      'soroban',
      configurationProvider
    ),
    vscode.commands.registerCommand(
      RUN_LAUNCH_PREFLIGHT_COMMAND,
      runStandaloneLaunchPreflight
    ),
    vscode.commands.registerCommand(DIAGNOSE_SOURCE_MAP_COMMAND, async () => {
      await runDiagnoseSourceMapCommand()
    }),
    factory,
    launchProgressReporter
  )
}

export function deactivate(): void {
  launchProgressReporter?.dispose()
  if (logManager) {
    logManager.dispose()
  }
}

async function runDiagnoseSourceMapCommand(): Promise<void> {
  const editor = vscode.window.activeTextEditor
  if (!editor) {
    await vscode.window.showWarningMessage(
      'Open a Rust file to diagnose source maps.'
    )
    return
  }

  const currentFilePath = editor.document.uri.fsPath
  if (!currentFilePath.endsWith('.rs')) {
    await vscode.window.showWarningMessage(
      'Active file is not a Rust (.rs) file.'
    )
    return
  }

  const outputChannel = vscode.window.createOutputChannel('Soroban Diagnostics')
  outputChannel.show(true)
  outputChannel.appendLine(`=== Source Map & Breakpoint Diagnostics ===`)
  outputChannel.appendLine(`File: ${currentFilePath}`)
  outputChannel.appendLine(`Timestamp: ${new Date().toISOString()}`)
  outputChannel.appendLine(`-------------------------------------------\n`)

  const breakpoints = vscode.debug.breakpoints.filter(
    (bp): bp is vscode.SourceBreakpoint =>
      bp instanceof vscode.SourceBreakpoint &&
      bp.location.uri.fsPath === currentFilePath
  )

  if (breakpoints.length === 0) {
    outputChannel.appendLine('ℹ️ No breakpoints set in the current file.')
  } else {
    outputChannel.appendLine(
      `🔍 Found ${breakpoints.length} breakpoint(s) in this file:\n`
    )

    const lines = breakpoints.map((bp) => bp.location.range.start.line + 1)
    const reports = diagnoseBreakpoints(currentFilePath, lines)

    reports.forEach((report) => {
      outputChannel.appendLine(`Line ${report.line}:`)
      outputChannel.appendLine(`  Status: ${report.status}`)
      if (report.functionName) {
        outputChannel.appendLine(
          `  Detected Function: '${report.functionName}'`
        )
      }
      outputChannel.appendLine(`  Reason: ${report.reason}\n`)
    })
  }

  outputChannel.appendLine(`-------------------------------------------`)
  outputChannel.appendLine(`General Troubleshooting:`)
  outputChannel.appendLine(
    `1. Ensure you compiled with debuginfo (e.g., profile.dev/profile.test).`
  )
  outputChannel.appendLine(
    `2. Ensure your launch.json 'contractPath' matches the compiled WASM.`
  )
}

async function ensureLaunchConfig(
  folder: vscode.WorkspaceFolder | undefined
): Promise<void> {
  const workspaceFolder = folder ?? vscode.workspace.workspaceFolders?.[0]
  if (!workspaceFolder) {
    await vscode.window.showInformationMessage(
      'Open a workspace folder first to generate a Soroban launch configuration.'
    )
    return
  }

  const vscodeDir = vscode.Uri.joinPath(workspaceFolder.uri, '.vscode')
  const launchUri = vscode.Uri.joinPath(vscodeDir, 'launch.json')

  try {
    await vscode.workspace.fs.createDirectory(vscodeDir)
    let launchJson: {
      version: string
      configurations: vscode.DebugConfiguration[]
    }

    try {
      const existing = await vscode.workspace.fs.readFile(launchUri)
      launchJson = JSON.parse(Buffer.from(existing).toString('utf8')) as {
        version: string
        configurations: vscode.DebugConfiguration[]
      }
    } catch {
      launchJson = { version: '0.2.0', configurations: [] }
    }

    const alreadyPresent = launchJson.configurations.some(
      (configuration) =>
        configuration.type === 'soroban' && configuration.request === 'launch'
    )

    if (!alreadyPresent) {
      launchJson.configurations.push(
        createDefaultLaunchConfig('${workspaceFolder}')
      )

      await vscode.workspace.fs.writeFile(
        launchUri,
        Buffer.from(`${JSON.stringify(launchJson, null, 2)}\n`, 'utf8')
      )
    }

    const doc = await vscode.workspace.openTextDocument(launchUri)
    await vscode.window.showTextDocument(doc, { preview: false })
  } catch (error) {
    await vscode.window.showErrorMessage(
      `Failed to generate launch.json: ${String(error)}`
    )
  }
}

function createDefaultLaunchConfig(
  workspaceFolder: string
): vscode.DebugConfiguration {
  return {
    name: "Soroban: Debug Contract",
    type: "soroban",
    request: "launch",
    contractPath: `${workspaceFolder}/target/wasm32-unknown-unknown/release/contract.wasm`,
    snapshotPath: `${workspaceFolder}/snapshot.json`,
    entrypoint: "main",
    args: [],
    trace: false,
    binaryPath: `${workspaceFolder}/target/debug/${process.platform === 'win32' ? 'soroban-debug.exe' : 'soroban-debug'}`,
  }
}

async function runStandaloneLaunchPreflight(): Promise<void> {
  const sources = (() => {
    const folders = vscode.workspace.workspaceFolders
    if (!folders || folders.length === 0) {
      return [
        {
          configurations: vscode.workspace
            .getConfiguration('launch')
            .get<unknown[]>('configurations'),
        },
      ]
    }

    return folders.map((folder) => ({
      folder,
      configurations: vscode.workspace
        .getConfiguration('launch', folder)
        .get<unknown[]>('configurations'),
    }))
  })()

  await runLaunchPreflightCommand({
    launchConfigSources: sources,
    selectLaunchConfig: async (candidates) => {
      const picked = await vscode.window.showQuickPick(
        candidates.map((candidate) => ({
          label: candidate.label,
          description: candidate.description,
          detail: candidate.detail,
          candidate,
        })),
        {
          placeHolder: 'Select a Soroban launch configuration to validate',
        }
      )
      return picked?.candidate
    },
    validateLaunchConfig: async (config) => validateLaunchConfig(config as SorobanLaunchConfig),
    showInformationMessage: async (message, ...actions) => vscode.window.showInformationMessage(message, ...actions),
    showWarningMessage: async (message, ...actions) => vscode.window.showWarningMessage(message, ...actions),
    showErrorMessage: async (message, ...actions) => vscode.window.showErrorMessage(message, ...actions),
    applyQuickFix: async (quickFix, folder, configName, field) =>
      applyQuickFix(quickFix, folder as vscode.WorkspaceFolder | undefined, configName, field)
  });
}

async function showPreflightIssueAndApplyFix(
  issue: LaunchPreflightIssue,
  folder: vscode.WorkspaceFolder | undefined,
  configName?: string
): Promise<void> {
  const actions = issue.quickFixes.map(toQuickPickLabel)
  const selected = await vscode.window.showErrorMessage(
    `${issue.message} Expected: ${issue.expected}`,
    ...actions
  )
  const quickFix = fromQuickPickLabel(selected)
  if (quickFix) {
    await applyQuickFix(quickFix, folder, configName, issue.field);
  }
}

async function applyQuickFix(
  quickFix: LaunchPreflightQuickFix,
  folder: vscode.WorkspaceFolder | undefined,
  configName?: string,
  field?: string
): Promise<void> {
  switch (quickFix) {
    case 'pickBinary':
      await pickFile('Select soroban-debug binary', ['exe', 'bin', ''], folder, configName, field);
      return;
    case 'pickContract':
      await pickFile('Select Soroban contract WASM', ['wasm'], folder, configName, field);
      return;
    case 'pickSnapshot':
      await pickFile('Select snapshot JSON', ['json'], folder, configName, field);
      return;
    case 'openLaunchConfig':
      await vscode.commands.executeCommand('workbench.action.debug.configure')
      return
    case 'generateLaunchConfig':
      await ensureLaunchConfig(folder)
      return
    case 'openSettings':
      await vscode.commands.executeCommand(
        'workbench.action.openSettings',
        '@ext:soroban.soroban-debugger'
      )
      return
    default:
      return
  }
}

async function pickFile(
  title: string,
  extensions: string[],
  folder: vscode.WorkspaceFolder | undefined,
  configName?: string,
  field?: string
): Promise<void> {
  const filters = extensions.filter((ext) => ext.length > 0);
  const selected = await vscode.window.showOpenDialog({
    canSelectFiles: true,
    canSelectFolders: false,
    canSelectMany: false,
    openLabel: title,
    filters: filters.length > 0 ? { Files: filters } : undefined,
  })

  if (selected && selected.length > 0) {
    const filePath = selected[0].fsPath;

    if (configName && field) {
      const choice = await vscode.window.showInformationMessage(
        `Selected path: ${filePath}. Do you want to update "${configName}" in launch.json directly?`,
        'Update launch.json',
        'Copy to Clipboard'
      );

      if (choice === 'Update launch.json') {
        await patchLaunchConfig(folder, configName, field, filePath);
        await vscode.window.showInformationMessage(`Updated ${field} in "${configName}" launch configuration.`);
        return;
      }
    }

    await vscode.env.clipboard.writeText(filePath);
    await vscode.window.showInformationMessage(
      `Selected path copied to clipboard: ${filePath}`,
      'Open launch.json'
    ).then(async (choice) => {
      if (choice === 'Open launch.json') {
        await vscode.commands.executeCommand('workbench.action.debug.configure');
      }
    });
  }
}

async function patchLaunchConfig(
  folder: vscode.WorkspaceFolder | undefined,
  configName: string,
  field: string,
  value: any
): Promise<void> {
  const settings = vscode.workspace.getConfiguration('launch', folder);
  const configurations = settings.get<any[]>('configurations') || [];
  const index = configurations.findIndex((c) => c.name === configName);

  if (index !== -1) {
    const updatedConfigurations = [...configurations];
    updatedConfigurations[index] = {
      ...updatedConfigurations[index],
      [field]: value
    };
    await settings.update('configurations', updatedConfigurations, vscode.ConfigurationTarget.WorkspaceFolder);
  } else {
    // If not found in workspace folder, try global (though usually it should be in workspace folder for debugging)
    const globalSettings = vscode.workspace.getConfiguration('launch');
    const globalConfigs = globalSettings.get<any[]>('configurations') || [];
    const globalIndex = globalConfigs.findIndex((c) => c.name === configName);
    if (globalIndex !== -1) {
      const updatedGlobalConfigs = [...globalConfigs];
      updatedGlobalConfigs[globalIndex] = {
        ...updatedGlobalConfigs[globalIndex],
        [field]: value
      };
      await globalSettings.update('configurations', updatedGlobalConfigs, vscode.ConfigurationTarget.Workspace);
    }
  }
}
