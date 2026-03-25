import * as vscode from 'vscode';
import { DebugAdapterDescriptor, DebugAdapterInlineImplementation } from 'vscode';
import { SorobanDebugSession } from '../dap/adapter';
import { DebuggerSessionInfo } from '../cli/debuggerProcess';
import { LogManager } from './logManager';

export class SorobanDebugAdapterDescriptorFactory
  implements vscode.DebugAdapterDescriptorFactory, vscode.Disposable {

  private context: vscode.ExtensionContext;
  private logManager: LogManager;
  private sessions = new Map<string, SorobanDebugSession>();
  private sessionInfo = new Map<string, DebuggerSessionInfo>();
  private readonly onSessionInfoEmitter = new vscode.EventEmitter<{ sessionId: string; info: DebuggerSessionInfo }>();
  public readonly onSessionInfo = this.onSessionInfoEmitter.event;

  constructor(context: vscode.ExtensionContext, logManager: LogManager) {
    this.context = context;
    this.logManager = logManager;
  }

  async createDebugAdapterDescriptor(
    session: vscode.DebugSession,
    executable: vscode.DebugAdapterExecutable | undefined
  ): Promise<DebugAdapterDescriptor | null> {
    const debugSession = new SorobanDebugSession(undefined, undefined, this.logManager, (info) => {
      this.sessionInfo.set(session.id, info);
      this.onSessionInfoEmitter.fire({ sessionId: session.id, info });
    });
    this.sessions.set(session.id, debugSession);
    return new DebugAdapterInlineImplementation(debugSession);
  }

  getSessionInfo(sessionId: string): DebuggerSessionInfo | undefined {
    return this.sessionInfo.get(sessionId);
  }

  clearSession(sessionId: string): void {
    this.sessions.delete(sessionId);
    this.sessionInfo.delete(sessionId);
  }

  dispose(): void {
    this.sessions.clear();
    this.sessionInfo.clear();
    this.onSessionInfoEmitter.dispose();
  }
}
