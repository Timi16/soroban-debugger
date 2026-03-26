import * as vscode from "vscode";
import {
  DebuggerProcessConfig,
  getDebuggerVersionInfo,
  LaunchPreflightIssue,
  LaunchPreflightQuickFix,
  validateLaunchConfig,
} from "./cli/debuggerProcess";
import { SorobanDebugAdapterDescriptorFactory } from "./debug/adapter";
import { LogManager } from "./debug/logManager";
import { SorobanLaunchProgressReporter } from "./launchProgress";
import {
  WIRE_PROTOCOL_MAX_VERSION,
  WIRE_PROTOCOL_MIN_VERSION,
} from "./dap/protocol";
import {
  fromQuickPickLabel,
  runLaunchPreflightCommand,
  toQuickPickLabel,
} from "./preflightCommand";

type SorobanLaunchConfig = vscode.DebugConfiguration & DebuggerProcessConfig;
const RUN_LAUNCH_PREFLIGHT_COMMAND = "soroban-debugger.runLaunchPreflight";

class SorobanDebugConfigurationProvider
  implements vscode.DebugConfigurationProvider
{
  async resolveDebugConfiguration(
    folder: vscode.WorkspaceFolder | undefined,
    config: SorobanLaunchConfig,
  ): Promise<vscode.DebugConfiguration | null | undefined> {
    if (!config.type && !config.request && !config.name) {
      return this.createDefaultLaunchConfig(folder);
    }

    if (config.type !== "soroban" || config.request !== "launch") {
      return config;
    }

    const settings = vscode.workspace.getConfiguration(
      "soroban-debugger",
      folder,
    );
    config.requestTimeoutMs =
      config.requestTimeoutMs ?? settings.get<number>("requestTimeoutMs");
    config.connectTimeoutMs =
      config.connectTimeoutMs ?? settings.get<number>("connectTimeoutMs");

    const preflight = await validateLaunchConfig(config);
    if (preflight.ok) {
      return config;
    }

    await showPreflightIssueAndApplyFix(preflight.issues[0], folder);

    return undefined;
  }

  private createDefaultLaunchConfig(
    folder: vscode.WorkspaceFolder | undefined,
  ): vscode.DebugConfiguration {
    return createDefaultLaunchConfig(
      folder?.uri.fsPath ?? "${workspaceFolder}",
    );
  }
}

let logManager: LogManager | undefined;
let launchProgressReporter: SorobanLaunchProgressReporter | undefined;
let versionOutputChannel: vscode.OutputChannel | undefined;

export function activate(context: vscode.ExtensionContext): void {
  logManager = new LogManager(context);
  launchProgressReporter = new SorobanLaunchProgressReporter();
  versionOutputChannel = vscode.window.createOutputChannel("Soroban Debugger");
  const factory = new SorobanDebugAdapterDescriptorFactory(
    context,
    logManager,
    launchProgressReporter,
  );
  const configurationProvider = new SorobanDebugConfigurationProvider();

  const sessionStartDisposable = vscode.debug.onDidStartDebugSession(
    (session) => {
      if (session.type !== "soroban") {
        return;
      }

      showVersionInfo(
        `backend: unknown protocol: ${WIRE_PROTOCOL_MIN_VERSION}..=${WIRE_PROTOCOL_MAX_VERSION}`,
      );
    },
  );

  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory("soroban", factory),
    vscode.debug.registerDebugConfigurationProvider(
      "soroban",
      configurationProvider,
    ),
    vscode.commands.registerCommand(
      RUN_LAUNCH_PREFLIGHT_COMMAND,
      runStandaloneLaunchPreflight,
    ),
    sessionStartDisposable,
    versionOutputChannel,
    factory,
    launchProgressReporter,
  );
}

export function deactivate(): void {
  launchProgressReporter?.dispose();
  if (logManager) {
    logManager.dispose();
  }
}

function showVersionInfo(output: string): void {
  const info = getDebuggerVersionInfo(output);
  const message = `Soroban Debugger | Backend: ${info.backendVersion} | Protocol: ${info.protocolVersion}`;

  vscode.window.setStatusBarMessage(message, 10_000);

  versionOutputChannel?.appendLine(message);
  versionOutputChannel?.show(true);
}

async function ensureLaunchConfig(
  folder: vscode.WorkspaceFolder | undefined,
): Promise<void> {
  const workspaceFolder = folder ?? vscode.workspace.workspaceFolders?.[0];
  if (!workspaceFolder) {
    await vscode.window.showInformationMessage(
      "Open a workspace folder first to generate a Soroban launch configuration.",
    );
    return;
  }

  const vscodeDir = vscode.Uri.joinPath(workspaceFolder.uri, ".vscode");
  const launchUri = vscode.Uri.joinPath(vscodeDir, "launch.json");

  try {
    await vscode.workspace.fs.createDirectory(vscodeDir);
    let launchJson: {
      version: string;
      configurations: vscode.DebugConfiguration[];
    };

    try {
      const existing = await vscode.workspace.fs.readFile(launchUri);
      launchJson = JSON.parse(Buffer.from(existing).toString("utf8")) as {
        version: string;
        configurations: vscode.DebugConfiguration[];
      };
    } catch {
      launchJson = { version: "0.2.0", configurations: [] };
    }

    const alreadyPresent = launchJson.configurations.some(
      (configuration) =>
        configuration.type === "soroban" && configuration.request === "launch",
    );

    if (!alreadyPresent) {
      launchJson.configurations.push(
        createDefaultLaunchConfig("${workspaceFolder}"),
      );

      await vscode.workspace.fs.writeFile(
        launchUri,
        Buffer.from(`${JSON.stringify(launchJson, null, 2)}\n`, "utf8"),
      );
    }

    const doc = await vscode.workspace.openTextDocument(launchUri);
    await vscode.window.showTextDocument(doc, { preview: false });
  } catch (error) {
    await vscode.window.showErrorMessage(
      `Failed to generate launch.json: ${String(error)}`,
    );
  }
}

function createDefaultLaunchConfig(
  workspaceFolder: string,
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
    binaryPath: `${workspaceFolder}/target/debug/${process.platform === "win32" ? "soroban-debug.exe" : "soroban-debug"}`,
  };
}

async function runStandaloneLaunchPreflight(): Promise<void> {
  const sources = (() => {
    const folders = vscode.workspace.workspaceFolders;
    if (!folders || folders.length === 0) {
      return [
        {
          configurations: vscode.workspace
            .getConfiguration("launch")
            .get<unknown[]>("configurations"),
        },
      ];
    }

    return folders.map((folder) => ({
      folder,
      configurations: vscode.workspace
        .getConfiguration("launch", folder)
        .get<unknown[]>("configurations"),
    }));
  })();

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
          placeHolder: "Select a Soroban launch configuration to validate",
        },
      );
      return picked?.candidate;
    },
    validateLaunchConfig: async (config) =>
      validateLaunchConfig(config as SorobanLaunchConfig),
    showInformationMessage: async (message, ...actions) =>
      vscode.window.showInformationMessage(message, ...actions),
    showWarningMessage: async (message, ...actions) =>
      vscode.window.showWarningMessage(message, ...actions),
    showErrorMessage: async (message, ...actions) =>
      vscode.window.showErrorMessage(message, ...actions),
    applyQuickFix: async (quickFix, folder) =>
      applyQuickFix(quickFix, folder as vscode.WorkspaceFolder | undefined),
  });
}

async function showPreflightIssueAndApplyFix(
  issue: LaunchPreflightIssue,
  folder: vscode.WorkspaceFolder | undefined,
): Promise<void> {
  const actions = issue.quickFixes.map(toQuickPickLabel);
  const selected = await vscode.window.showErrorMessage(
    `${issue.message} Expected: ${issue.expected}`,
    ...actions,
  );
  const quickFix = fromQuickPickLabel(selected);
  if (quickFix) {
    await applyQuickFix(quickFix, folder);
  }
}

async function applyQuickFix(
  quickFix: LaunchPreflightQuickFix,
  folder: vscode.WorkspaceFolder | undefined,
): Promise<void> {
  switch (quickFix) {
    case "pickBinary":
      await pickFile("Select soroban-debug binary", ["exe", "bin", ""]);
      return;
    case "pickContract":
      await pickFile("Select Soroban contract WASM", ["wasm"]);
      return;
    case "pickSnapshot":
      await pickFile("Select snapshot JSON", ["json"]);
      return;
    case "openLaunchConfig":
      await vscode.commands.executeCommand("workbench.action.debug.configure");
      return;
    case "generateLaunchConfig":
      await ensureLaunchConfig(folder);
      return;
    case "openSettings":
      await vscode.commands.executeCommand(
        "workbench.action.openSettings",
        "@ext:soroban.soroban-debugger",
      );
      return;
    default:
      return;
  }
}

async function pickFile(title: string, extensions: string[]): Promise<void> {
  const filters = extensions.filter((ext) => ext.length > 0);
  const selected = await vscode.window.showOpenDialog({
    canSelectFiles: true,
    canSelectFolders: false,
    canSelectMany: false,
    openLabel: title,
    filters: filters.length > 0 ? { Files: filters } : undefined,
  });

  if (selected && selected.length > 0) {
    await vscode.env.clipboard.writeText(selected[0].fsPath);
    await vscode.window
      .showInformationMessage(
        `Selected path copied to clipboard: ${selected[0].fsPath}`,
        "Open launch.json",
      )
      .then(async (choice) => {
        if (choice === "Open launch.json") {
          await vscode.commands.executeCommand(
            "workbench.action.debug.configure",
          );
        }
      });
  }
}
