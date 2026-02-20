import * as vscode from 'vscode';
import { SorobanDebugAdapterDescriptorFactory } from './debug/adapter';

export function activate(context: vscode.ExtensionContext): void {
  const factory = new SorobanDebugAdapterDescriptorFactory(context);

  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory('soroban', factory),
    factory
  );
}

export function deactivate(): void {
  // Cleanup on extension deactivation
}
