import * as vscode from 'vscode';
import { SorobanDebugAdapterDescriptorFactory } from './debug/adapter';
import { DebuggerSessionInfo } from './cli/debuggerProcess';

export function activate(context: vscode.ExtensionContext): void {
  const factory = new SorobanDebugAdapterDescriptorFactory(context);
  const statusItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  statusItem.name = 'Soroban Debug Session Info';
  statusItem.hide();

  const applySessionInfo = (info: DebuggerSessionInfo): void => {
    const text = `Soroban: v${info.backendVersion} | protocol ${info.protocolVersion}`;
    statusItem.text = text;
    statusItem.tooltip = text;
    statusItem.show();
  };

  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory('soroban', factory),
    vscode.debug.onDidStartDebugSession((session) => {
      if (session.type !== 'soroban') {
        return;
      }

      applySessionInfo({
        backendVersion: 'unknown',
        protocolVersion: 'unknown'
      });

      const info = factory.getSessionInfo(session.id);
      if (info) {
        applySessionInfo(info);
      }
    }),
    factory.onSessionInfo(({ sessionId, info }) => {
      const active = vscode.debug.activeDebugSession;
      if (!active || active.type !== 'soroban' || active.id !== sessionId) {
        return;
      }
      applySessionInfo(info);
    }),
    vscode.debug.onDidTerminateDebugSession((session) => {
      if (session.type !== 'soroban') {
        return;
      }
      factory.clearSession(session.id);
      statusItem.hide();
    }),
    factory,
    statusItem
  );
}

export function deactivate(): void {
  // Cleanup on extension deactivation
}
