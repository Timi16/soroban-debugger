import * as vscode from 'vscode';
import { DebugAdapterDescriptor, DebugAdapterInlineImplementation } from 'vscode';
import { SorobanDebugSession } from '../dap/adapter';

export class SorobanDebugAdapterDescriptorFactory
  implements vscode.DebugAdapterDescriptorFactory, vscode.Disposable {
  
  private context: vscode.ExtensionContext;
  private session: SorobanDebugSession | null = null;

  constructor(context: vscode.ExtensionContext) {
    this.context = context;
  }

  async createDebugAdapterDescriptor(
    session: vscode.DebugSession,
    executable: vscode.DebugAdapterExecutable | undefined
  ): Promise<DebugAdapterDescriptor | null> {
    this.session = new SorobanDebugSession();
    return new DebugAdapterInlineImplementation(this.session);
  }

  dispose(): void {
    this.session = null;
  }
}
