import {
  DebugSession,
  InitializedEvent,
  StoppedEvent,
  ExitedEvent
} from '@vscode/debugadapter';
import { DebugProtocol } from '@vscode/debugprotocol';
import * as readline from 'readline';
import {
  DebuggerProcess,
  DebuggerProcessConfig,
  DebuggerTimeoutError,
  LaunchLifecycleEvent,
  validateLaunchConfig
} from '../cli/debuggerProcess';
import { BreakpointCapabilities, BreakpointLocation, DebuggerState, Variable } from './protocol';
import { VariableStore } from './variableStore';
import { ResolvedBreakpoint, resolveSourceBreakpoints } from './sourceBreakpoints';
import { LogOutputEvent, LogLevel } from '@vscode/debugadapter/lib/logger';
import { LogManager, LogLevel as ManagerLogLevel, LogPhase } from '../debug/logManager';

type LaunchRequestArgs = DebugProtocol.LaunchRequestArguments & DebuggerProcessConfig;

export class SorobanDebugSession extends DebugSession {
  private logManager: LogManager | undefined;
  private debuggerProcess: DebuggerProcess | null = null;
  private state: DebuggerState = {
    isRunning: false,
    isPaused: false,
    breakpoints: new Map(),
    callStack: [],
    storage: {}
  };
  private variableStore = new VariableStore();
  private backendCapabilities: BreakpointCapabilities = {
    conditionalBreakpoints: false,
    hitConditionalBreakpoints: false,
    logPoints: false
  };
  private threadId = 1;
  private outputReaders: readline.Interface[] = [];
  private hasExecuted = false;
  private exportedFunctions = new Set<string>();
  private sourceFunctionBreakpoints = new Map<string, Set<string>>();
  private requestAbortControllers = new Map<number, AbortController>();
  private refreshAbortController: AbortController | null = null;
  private refreshGeneration = 0;
  private launchLifecycleReporter?: (event: LaunchLifecycleEvent) => void;

  constructor(
    logManagerOrLinesStartAt1?: LogManager | boolean,
    launchLifecycleReporterOrIsServer?: ((event: LaunchLifecycleEvent) => void) | boolean
  ) {
    super(
      typeof logManagerOrLinesStartAt1 === 'boolean' ? logManagerOrLinesStartAt1 : true,
      typeof launchLifecycleReporterOrIsServer === 'boolean' ? launchLifecycleReporterOrIsServer : false
    );
    this.logManager = typeof logManagerOrLinesStartAt1 === 'boolean' ? undefined : logManagerOrLinesStartAt1;
    this.launchLifecycleReporter = typeof launchLifecycleReporterOrIsServer === 'boolean'
      ? undefined
      : launchLifecycleReporterOrIsServer;
  }

  protected initializeRequest(
    response: DebugProtocol.InitializeResponse,
    args: DebugProtocol.InitializeRequestArguments
  ): void {
    this.logManager?.log(ManagerLogLevel.Info, LogPhase.DAP, `InitializeRequest: ${JSON.stringify(args)}`);
    response.body = response.body || {};
    response.body.supportsConfigurationDoneRequest = true;
    response.body.supportsEvaluateForHovers = true;
    (response.body as any).supportsVariablePaging = true;
    (response.body as any).supportsCancelRequest = true;
    response.body.supportsSetVariable = false;
    response.body.supportsSetExpression = false;
    response.body.supportsConditionalBreakpoints = true;
    response.body.supportsHitConditionalBreakpoints = true;
    response.body.supportsLogPoints = true;

    this.sendResponse(response);
    this.sendEvent(new InitializedEvent());
  }

  protected async launchRequest(
    response: DebugProtocol.LaunchResponse,
    args: LaunchRequestArgs
  ): Promise<void> {
    this.logManager?.log(ManagerLogLevel.Info, LogPhase.DAP, `LaunchRequest: ${JSON.stringify(args)}`);
    try {
      const preflight = await validateLaunchConfig(args);
      if (!preflight.ok) {
        const issue = preflight.issues[0];
        throw new Error(`${issue.message} Expected: ${issue.expected}`);
      }

      this.debuggerProcess = new DebuggerProcess({
        contractPath: args.contractPath,
        snapshotPath: args.snapshotPath,
        entrypoint: args.entrypoint || 'main',
        args: args.args || [],
        trace: args.trace || false,
        binaryPath: args.binaryPath,
        port: args.port,
        token: args.token,
        requestTimeoutMs: args.requestTimeoutMs,
        connectTimeoutMs: args.connectTimeoutMs
      }, this.logManager, this.launchLifecycleReporter);

      await this.debuggerProcess.start();
      this.state.isRunning = true;
      this.state.isPaused = false;
      this.hasExecuted = false;
      this.variableStore.reset();
      this.exportedFunctions = await this.debuggerProcess.getContractFunctions();
      this.backendCapabilities = await this.debuggerProcess.getCapabilities().catch(() => ({
        conditionalBreakpoints: false,
        hitConditionalBreakpoints: false,
        logPoints: false
      }));

      this.attachProcessListeners();
      this.sendResponse(response);
    } catch (error) {
      const message = error instanceof DebuggerTimeoutError
        ? `Failed to launch debugger (timeout): ${error.message}\n\nNext steps: ensure the backend process is running, reachable, and not stalled; then retry the session.`
        : `Failed to launch debugger: ${String(error)}`;
      this.sendErrorResponse(response, {
        id: 1001,
        format: message,
        showUser: true
      });
    }
  }

  protected async setBreakPointsRequest(
    response: DebugProtocol.SetBreakpointsResponse,
    args: DebugProtocol.SetBreakpointsArguments
  ): Promise<void> {
    const source = args.source.path || args.source.name || '';
    const breakpoints = args.breakpoints || [];
    const lines = breakpoints.map((bp) => bp.line);

    try {
      let resolved: ResolvedBreakpoint[];
      if (!this.debuggerProcess || !source) {
        resolved = lines.map((line) => ({
          requestedLine: line,
          line,
          verified: false,
          reasonCode: 'NO_DEBUGGER',
          setBreakpoint: false,
          message: 'Debugger is not launched or source path is unavailable'
        }));
      } else {
        let serverResolved: Array<{ requestedLine: number; line: number; verified: boolean; functionName?: string; reasonCode: string; message: string }> | null = null;
        try {
          serverResolved = await this.debuggerProcess.resolveSourceBreakpoints(source, lines, this.exportedFunctions);
        } catch {
          serverResolved = null;
        }

        const shouldFallbackHeuristic = serverResolved
          ? serverResolved.every((bp) => ['NO_DEBUG_INFO', 'FILE_NOT_IN_DEBUG_INFO', 'WASM_PARSE_ERROR'].includes(bp.reasonCode))
          : false;

        if (serverResolved && shouldFallbackHeuristic) {
          resolved = resolveSourceBreakpoints(source, lines, this.exportedFunctions);
        } else if (serverResolved) {
          resolved = serverResolved.map((bp) => ({
            requestedLine: bp.requestedLine,
            line: bp.line,
            verified: bp.verified,
            functionName: bp.functionName,
            reasonCode: bp.reasonCode,
            message: bp.message,
            setBreakpoint: bp.verified && Boolean(bp.functionName)
          }));
        } else {
          resolved = resolveSourceBreakpoints(source, lines, this.exportedFunctions);
        }
      }

      const managedBreakpoints: BreakpointLocation[] = breakpoints.map((bp, index) => {
        const match = resolved.find((resolvedBreakpoint) => resolvedBreakpoint.line === bp.line);
        return {
          id: `${source}:${bp.line}:${bp.column ?? 1}:${index}`,
          source,
          line: bp.line,
          column: bp.column,
          functionName: match?.functionName,
          condition: bp.condition,
          hitCondition: bp.hitCondition,
          logMessage: bp.logMessage
        };
      });

      const syncErrors = await this.syncSourceBreakpoints(
        source,
        managedBreakpoints.filter((bp) => {
          const match = resolved.find((resolvedBreakpoint) => resolvedBreakpoint.line === bp.line);
          return Boolean(match?.verified && bp.functionName);
        })
      );
      const syncMessage = syncErrors.size > 0
        ? `${syncErrors.size} breakpoint sync error(s)`
        : '';
      const capabilityMessages = '';

      this.state.breakpoints.set(source, managedBreakpoints);
      this.sourceFunctionBreakpoints.set(
        source,
        new Set(
          resolved
            .filter((bp) => bp.setBreakpoint && bp.functionName)
            .map((bp) => bp.functionName as string)
        )
      );

      response.body = {
        breakpoints: breakpoints.map((bp) => {
          const match = resolved.find((resolvedBreakpoint) => resolvedBreakpoint.requestedLine === bp.line);
          return {
            verified: match?.verified ?? false,
            line: match?.line ?? bp.line,
            column: bp.column,
            source: args.source,
            message: [match?.message, syncMessage, capabilityMessages].filter(Boolean).join(' ')
          };
        })
      };

      this.sendResponse(response);
    } catch (error) {
      this.sendErrorResponse(response, {
        id: 1003,
        format: `Failed to resolve breakpoints: ${error}`,
        showUser: true
      });
    }
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
    // Rebuild handles each time to reflect the latest paused state.
    this.variableStore.reset();

    const scopes: DebugProtocol.Scope[] = [];

    const argsRef = this.variableStore.createListHandle(this.variableStore.variablesFromArgs(this.state.args));
    scopes.push({
      name: 'Arguments',
      variablesReference: argsRef,
      expensive: false
    });

    const storageKeys = this.state.storage ? Object.keys(this.state.storage) : [];
    if (storageKeys.length > 0) {
      const variablesRef = this.variableStore.createListHandle(this.variableStore.variablesFromStorage(this.state.storage as Record<string, unknown>));

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
    const variables = this.variableStore.getVariables(args.variablesReference, {
      start: (args as any).start as number | undefined,
      count: (args as any).count as number | undefined
    });

    response.body = {
      variables: variables.map((v: Variable) => ({
        name: v.name,
        value: v.value,
        type: v.type,
        variablesReference: v.variablesReference || 0,
        indexedVariables: v.indexedVariables,
        namedVariables: v.namedVariables
      }))
    };

    this.sendResponse(response);
  }

   protected async evaluateRequest(
    response: DebugProtocol.EvaluateResponse,
    args: DebugProtocol.EvaluateArguments
  ): Promise<void> {
    const expression = (args.expression || '').trim();

    try {
      if (this.debuggerProcess && this.state.isPaused) {
        await this.refreshState();
      }

      // 1. Check for "magic" variables (local overrides)
      if (expression === 'args' || expression === 'Arguments') {
        response.body = {
          result: this.state.args ?? '(none)',
          variablesReference: 0
        };
        this.sendResponse(response);
        return;
      }

      if (expression === 'storage' || expression === 'Storage') {
        response.body = {
          result: JSON.stringify(this.state.storage || {}),
          variablesReference: 0
        };
        this.sendResponse(response);
        return;
      }

      if (expression.startsWith('storage.')) {
        const key = expression.slice('storage.'.length);
        const value = this.state.storage ? (this.state.storage as Record<string, unknown>)[key] : undefined;
        if (typeof value === 'undefined') {
          throw new Error(`Unknown storage key: ${key}`);
        }
        response.body = {
          result: typeof value === 'string' ? value : JSON.stringify(value),
          variablesReference: 0
        };
        this.sendResponse(response);
        return;
      }

      // 2. Fall back to backend evaluation if available and paused
      if (this.debuggerProcess && this.state.isPaused) {
        const result = await this.debuggerProcess.evaluate(args.expression, args.frameId);
        response.body = {
          result: result.result,
          type: result.type,
          variablesReference: result.variablesReference
        };
        this.sendResponse(response);
        return;
      }

      throw new Error('Unsupported expression or debugger not paused. Try `args`, `storage`, or `storage.<key>`.');
    } catch (error) {
      if (error instanceof DebuggerTimeoutError) {
        this.sendErrorResponse(response, {
          id: 1010,
          format:
            `Evaluate timed out (${error.requestType}). The backend may be stalled.\n\n` +
            `Next steps: restart the debug session; if it persists, verify the backend binary and connectivity.`,
          showUser: true
        });
        return;
      }

      this.sendErrorResponse(response, {
        id: 1010,
        format: `Evaluate failed: ${String(error)}`,
        showUser: false
      });
    }
  }

  protected async continueRequest(
    response: DebugProtocol.ContinueResponse,
    args: DebugProtocol.ContinueArguments
  ): Promise<void> {
    try {
      if (!this.debuggerProcess) {
        throw new Error('Debugger process is not running');
      }

      response.body = { allThreadsContinued: true };
      this.sendResponse(response);

      if (!this.hasExecuted) {
        await this.runExecution('step');
        return;
      }

      const result = await this.debuggerProcess.continueExecution();
      if (result.output) {
        this.sendEvent(new LogOutputEvent(`Result: ${result.output}\n`, LogLevel.Log));
      }

      if (result.paused) {
        await this.refreshState();
        this.state.isPaused = true;
        this.sendEvent(new StoppedEvent('breakpoint', this.threadId));
        return;
      }

      this.sendEvent(new ExitedEvent(0));
      await this.stop();
    } catch (error) {
      if (error instanceof DebuggerTimeoutError) {
        this.sendEvent(new LogOutputEvent(
          `Debugger request timed out (${error.requestType}). The backend may be stalled or the connection is unhealthy.\n` +
          `Next steps: restart the debug session; if it persists, verify the backend binary and network connectivity.\n`,
          LogLevel.Error
        ));
        this.sendEvent(new ExitedEvent(1));
        await this.stop();
        return;
      }

      this.sendErrorResponse(response, {
        id: 1002,
        format: `Continue failed: ${error}`,
        showUser: true
      });
    }
  }

  protected async nextRequest(
    response: DebugProtocol.NextResponse,
    args: DebugProtocol.NextArguments
  ): Promise<void> {
    await this.stepOnce(response, 'next');
  }

  protected async stepInRequest(
    response: DebugProtocol.StepInResponse,
    args: DebugProtocol.StepInArguments
  ): Promise<void> {
    await this.stepOnce(response, 'step in');
  }

  protected async stepOutRequest(
    response: DebugProtocol.StepOutResponse,
    args: DebugProtocol.StepOutArguments
  ): Promise<void> {
    await this.stepOnce(response, 'step out');
  }

  protected async threadsRequest(
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
    this.state.isPaused = true;
    this.sendEvent(new StoppedEvent('entry', this.threadId));
  }

  // VS Code will send a DAP "cancel" request with the requestId (seq) of the request to cancel.
  // We forward this to the backend as a protocol-level Cancel to interrupt any long-running execution.
  protected cancelRequest(response: any, args: any): void {
    const requestId = args?.requestId as number | undefined;
    if (typeof requestId === 'number') {
      const controller = this.requestAbortControllers.get(requestId);
      controller?.abort();
      this.requestAbortControllers.delete(requestId);
    }
    
    this.debuggerProcess?.cancel();

    this.sendResponse(response);
  }

  protected async disconnectRequest(
    response: DebugProtocol.DisconnectResponse,
    args: DebugProtocol.DisconnectArguments
  ): Promise<void> {
    this.logManager?.log(ManagerLogLevel.Info, LogPhase.DAP, `DisconnectRequest: ${JSON.stringify(args)}`);
    await this.stop();
    this.sendResponse(response);
  }

  private attachProcessListeners(): void {
    if (!this.debuggerProcess) return;

    const stdout = this.debuggerProcess.getOutputStream();
    if (stdout) {
      const reader = readline.createInterface({
        input: stdout,
        crlfDelay: Infinity
      });

      reader.on('line', (line: string) => {
        this.logManager?.log(ManagerLogLevel.Debug, LogPhase.Backend, line);
        this.sendEvent(new LogOutputEvent(line + '\n', LogLevel.Log));
      });
      this.outputReaders.push(reader);
    }

    const stderr = this.debuggerProcess.getErrorStream();
    if (stderr) {
      const reader = readline.createInterface({
        input: stderr,
        crlfDelay: Infinity
      });

      reader.on('line', (line: string) => {
        this.logManager?.log(ManagerLogLevel.Error, LogPhase.Backend, line);
        this.sendEvent(new LogOutputEvent(line + '\n', LogLevel.Error));
      });
      this.outputReaders.push(reader);
    }
  }

  private async runExecution(reason: 'step' | 'entry' | 'breakpoint' | 'pause'): Promise<void> {
    if (!this.debuggerProcess) {
      throw new Error('Debugger process is not running');
    }

    const result = await this.debuggerProcess.execute();
    this.hasExecuted = true;
    await this.refreshState();
    if (result.output) {
      this.sendEvent(new LogOutputEvent(`Result: ${result.output}\n`, LogLevel.Log));
    }

    if (result.paused) {
      this.state.isPaused = true;
      this.sendEvent(new StoppedEvent(reason, this.threadId));
      return;
    }

    this.state.isPaused = false;
    this.sendEvent(new ExitedEvent(0));
    await this.stop();
  }

  private async stepOnce(
    response:
      | DebugProtocol.NextResponse
      | DebugProtocol.StepInResponse
      | DebugProtocol.StepOutResponse,
    label: string
  ): Promise<void> {
    try {
      if (!this.debuggerProcess) {
        throw new Error('Debugger process is not running');
      }

      this.sendResponse(response);

      if (!this.hasExecuted) {
        await this.runExecution('step');
        return;
      }

      let result;
      if (label === 'next') {
        result = await this.debuggerProcess.next();
      } else if (label === 'step in') {
        result = await this.debuggerProcess.stepIn();
      } else if (label === 'step out') {
        result = await this.debuggerProcess.stepOut();
      } else {
        result = await this.debuggerProcess.stepIn(); // Fallback
      }

      if (result.paused) {
        await this.refreshState();
        this.state.isPaused = true;
        this.sendEvent(new StoppedEvent('step', this.threadId));
        return;
      }

      this.sendEvent(new ExitedEvent(0));
      await this.stop();
    } catch (error) {
      if (error instanceof DebuggerTimeoutError) {
        this.sendEvent(new LogOutputEvent(
          `${label} timed out (${error.requestType}). The backend may be stalled.\n` +
          `Next steps: restart the debug session.\n`,
          LogLevel.Error
        ));
        this.sendEvent(new ExitedEvent(1));
        await this.stop();
        return;
      }

      this.sendEvent(new LogOutputEvent(`${label} failed: ${error}\n`, LogLevel.Error));
    }
  }

  private async syncSourceBreakpoints(
    source: string,
    nextBreakpoints: BreakpointLocation[]
  ): Promise<Map<string, string>> {
    if (!this.debuggerProcess) {
      return new Map();
    }

    const previousBreakpoints = this.state.breakpoints.get(source) || [];
    const errors = new Map<string, string>();

    for (const breakpoint of previousBreakpoints) {
      await this.debuggerProcess.clearBreakpoint(breakpoint.id);
    }

    for (const breakpoint of nextBreakpoints) {
      try {
        await this.debuggerProcess.setBreakpoint({
          id: breakpoint.id,
          functionName: breakpoint.functionName as string,
          condition: breakpoint.condition,
          hitCondition: breakpoint.hitCondition,
          logMessage: breakpoint.logMessage
        });
      } catch (error) {
        errors.set(
          breakpoint.id,
          error instanceof Error ? error.message : String(error)
        );
      }
    }

    return errors;
  }

  private async refreshState(): Promise<void> {
    if (!this.debuggerProcess) {
      return;
    }

    this.refreshAbortController?.abort();
    const controller = new AbortController();
    this.refreshAbortController = controller;
    const generation = (this.refreshGeneration += 1);

    let inspection;
    let storage;
    try {
      [inspection, storage] = await Promise.all([
        this.debuggerProcess.inspect({ signal: controller.signal }),
        this.debuggerProcess.getStorage({ signal: controller.signal })
      ]);
    } catch (error) {
      if ((error as any)?.name === 'AbortError' || (error as any)?.name === 'TimeoutError') {
        return;
      }
      throw error;
    }

    if (controller.signal.aborted || generation !== this.refreshGeneration) {
      return;
    }

      this.state.callStack = inspection.callStack.map((frame, index) => {
      let sourcePath = frame;
      let line = 1;

      // Try to find the range for the function to resolve the actual source line
      for (const [sourceFilePath, sourceBpSet] of this.sourceFunctionBreakpoints.entries()) {
        if (sourceBpSet.has(frame)) {
          sourcePath = sourceFilePath;
          try {
            const { parseFunctionRanges } = require('./sourceBreakpoints');
            const ranges = parseFunctionRanges(sourcePath);
            const range = ranges.find((r: any) => r.name === frame);
            if (range) {
              line = range.startLine;
            }
          } catch (e) {
            // Ignore if parseFunctionRanges fails
          }
          break; // Stop looking after the first match
        }
      }

      return {
        id: index + 1,
        name: frame,
        source: sourcePath,
        line: line,
        column: 1
      };
    });
    this.state.args = inspection.args;
    this.state.storage = storage;
  }

  public async stop(): Promise<void> {
    this.refreshAbortController?.abort();
    this.refreshAbortController = null;

    for (const controller of this.requestAbortControllers.values()) {
      controller.abort();
    }
    this.requestAbortControllers.clear();

    for (const reader of this.outputReaders) {
      reader.close();
    }
    this.outputReaders = [];

    if (this.debuggerProcess) {
      await this.debuggerProcess.stop();
      this.debuggerProcess = null;
    }

    this.state.isRunning = false;
    this.state.isPaused = false;
    this.state.callStack = [];
    this.state.storage = {};
    this.state.args = undefined;
    this.hasExecuted = false;
    this.sourceFunctionBreakpoints.clear();
  }

  private describeCapabilityFallback(bp: DebugProtocol.SourceBreakpoint): string | undefined {
    const notices: string[] = [];

    if (bp.condition && !this.backendCapabilities.conditionalBreakpoints) {
      notices.push('Conditional evaluation is unavailable in the current backend.');
    }
    if (bp.hitCondition && !this.backendCapabilities.hitConditionalBreakpoints) {
      notices.push('Hit-count filtering is unavailable in the current backend.');
    }
    if (bp.logMessage && !this.backendCapabilities.logPoints) {
      notices.push('Logpoints are unavailable in the current backend.');
    }

    return notices.length > 0 ? notices.join(' ') : undefined;
  }
}
