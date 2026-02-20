import { 
  DebugSession, 
  InitializedEvent, 
  BreakpointEvent, 
  StoppedEvent, 
  ExitedEvent,
  LogOutputEvent,
  EventEmitter
} from '@vscode/debugadapter';
import { DebugProtocol } from '@vscode/debugprotocol';
import * as readline from 'readline';
import { DebuggerProcess, DebuggerProcessConfig } from '../cli/debuggerProcess';
import { DebuggerState, Variable, StackFrame } from './protocol';

export class SorobanDebugSession extends DebugSession {
  private debuggerProcess: DebuggerProcess | null = null;
  private state: DebuggerState = {
    isRunning: false,
    isPaused: false,
    breakpoints: new Map(),
    callStack: [],
    variables: []
  };
  private variableHandles = new Map<number, any>();
  private nextVarHandle = 1;
  private threadId = 1;
  private rl: readline.Interface | null = null;

  protected initializeRequest(
    response: DebugProtocol.InitializeResponse,
    args: DebugProtocol.InitializeRequestArguments
  ): void {
    response.body = response.body || {};
    response.body.supportsConfigurationDoneRequest = true;
    response.body.supportsEvaluateForHovers = true;
    response.body.supportsSetVariable = false;
    response.body.supportsSetExpression = false;
    response.body.supportsConditionalBreakpoints = false;
    response.body.supportsHitConditionalBreakpoints = false;
    response.body.supportsLogPoints = false;

    this.sendResponse(response);
    this.sendEvent(new InitializedEvent());
  }

  protected async launchRequest(
    response: DebugProtocol.LaunchResponse,
    args: DebugProtocol.LaunchRequestArguments & DebuggerProcessConfig
  ): Promise<void> {
    try {
      this.debuggerProcess = new DebuggerProcess({
        contractPath: args.contractPath,
        snapshotPath: args.snapshotPath,
        entrypoint: args.entrypoint || 'main',
        args: args.args || [],
        trace: args.trace || false
      });

      await this.debuggerProcess.start();
      this.state.isRunning = true;

      this.attachProcessListeners();
      this.sendResponse(response);
    } catch (error) {
      this.sendErrorResponse(response, {
        id: 1001,
        format: `Failed to launch debugger: ${error}`,
        showUser: true
      });
    }
  }

  protected async setBreakpointsRequest(
    response: DebugProtocol.SetBreakpointsResponse,
    args: DebugProtocol.SetBreakpointsArguments
  ): Promise<void> {
    const source = args.source.path || args.source.name || '';
    const breakpoints = args.breakpoints || [];

    this.state.breakpoints.set(source, 
      breakpoints.map((bp, idx) => ({
        source,
        line: bp.line,
        column: bp.column
      }))
    );

    response.body = {
      breakpoints: breakpoints.map(bp => ({
        verified: true,
        line: bp.line,
        column: bp.column,
        source: args.source
      }))
    };

    this.sendResponse(response);
  }

  protected async stackTraceRequest(
    response: DebugProtocol.StackTraceResponse,
    args: DebugProtocol.StackTraceArguments
  ): Promise<void> {
    const stackFrames = this.state.callStack || [];

    response.body = {
      stackFrames: stackFrames.slice(0, 50).map(frame => ({
        id: frame.id,
        name: frame.name,
        source: {
          name: frame.source,
          path: frame.source
        },
        line: frame.line,
        column: frame.column,
        instructionPointerReference: frame.instructionPointerReference
      }))
    };

    this.sendResponse(response);
  }

  protected async scopesRequest(
    response: DebugProtocol.ScopesResponse,
    args: DebugProtocol.ScopesArguments
  ): Promise<void> {
    const scopes: DebugProtocol.Scope[] = [];

    if (this.state.variables && this.state.variables.length > 0) {
      const variablesRef = this.nextVarHandle++;
      this.variableHandles.set(variablesRef, this.state.variables);

      scopes.push({
        name: 'Storage',
        variablesReference: variablesRef,
        expensive: false
      });
    }

    response.body = { scopes };
    this.sendResponse(response);
  }

  protected async variablesRequest(
    response: DebugProtocol.VariablesResponse,
    args: DebugProtocol.VariablesArguments
  ): Promise<void> {
    const variables = this.variableHandles.get(args.variablesReference) || [];

    response.body = {
      variables: variables.map((v: Variable) => ({
        name: v.name,
        value: v.value,
        type: v.type,
        variablesReference: v.variablesReference || 0
      }))
    };

    this.sendResponse(response);
  }

  protected async continueRequest(
    response: DebugProtocol.ContinueResponse,
    args: DebugProtocol.ContinueArguments
  ): Promise<void> {
    this.state.isPaused = false;
    response.body = { allThreadsContinued: true };
    this.sendResponse(response);
  }

  protected async nextRequest(
    response: DebugProtocol.NextResponse,
    args: DebugProtocol.NextArguments
  ): Promise<void> {
    this.sendResponse(response);
  }

  protected async stepInRequest(
    response: DebugProtocol.StepInResponse,
    args: DebugProtocol.StepInArguments
  ): Promise<void> {
    this.sendResponse(response);
  }

  protected async stepOutRequest(
    response: DebugProtocol.StepOutResponse,
    args: DebugProtocol.StepOutArguments
  ): Promise<void> {
    this.sendResponse(response);
  }

  protected async threadRequest(
    response: DebugProtocol.ThreadsResponse
  ): Promise<void> {
    response.body = {
      threads: [{
        id: this.threadId,
        name: 'Main Thread'
      }]
    };
    this.sendResponse(response);
  }

  protected async configurationDoneRequest(
    response: DebugProtocol.ConfigurationDoneResponse,
    args: DebugProtocol.ConfigurationDoneArguments
  ): Promise<void> {
    this.sendResponse(response);
  }

  protected async disconnectRequest(
    response: DebugProtocol.DisconnectResponse,
    args: DebugProtocol.DisconnectArguments
  ): Promise<void> {
    await this.stop();
    this.sendResponse(response);
  }

  private attachProcessListeners(): void {
    if (!this.debuggerProcess) return;

    const stdout = this.debuggerProcess.getOutputStream();
    if (stdout) {
      this.rl = readline.createInterface({ 
        input: stdout,
        crlfDelay: Infinity 
      });

      this.rl.on('line', (line: string) => {
        this.handleDebuggerOutput(line);
      });
    }

    const stderr = this.debuggerProcess.getErrorStream();
    if (stderr) {
      stderr.on('data', (data: Buffer) => {
        this.sendEvent(new LogOutputEvent(`${data}\n`));
      });
    }
  }

  private handleDebuggerOutput(output: string): void {
    try {
      const event = JSON.parse(output);

      if (event.type === 'breakpoint' || event.type === 'stopped') {
        this.state.isPaused = true;
        this.state.callStack = event.stackTrace || [];
        this.state.variables = event.variables || [];

        this.sendEvent(new StoppedEvent('breakpoint', this.threadId));
      } else if (event.type === 'continued') {
        this.state.isPaused = false;
      } else if (event.type === 'exited') {
        this.sendEvent(new ExitedEvent(event.exitCode || 0));
      }
    } catch {
      // Not a JSON event, just log it
      this.sendEvent(new LogOutputEvent(output + '\n'));
    }
  }

  private async stop(): Promise<void> {
    if (this.rl) {
      this.rl.close();
      this.rl = null;
    }

    if (this.debuggerProcess) {
      await this.debuggerProcess.stop();
      this.debuggerProcess = null;
    }

    this.state.isRunning = false;
    this.state.isPaused = false;
  }
}
