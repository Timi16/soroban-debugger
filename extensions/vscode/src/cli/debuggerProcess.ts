import { ChildProcess, execFile, spawn } from "child_process";
import * as fs from "fs";
import * as net from "net";
import * as path from "path";
import {
  WIRE_PROTOCOL_MAX_VERSION,
  WIRE_PROTOCOL_MIN_VERSION,
} from "../dap/protocol";
import { shouldPromoteToFunctionBreakpoint } from "../dap/sourceBreakpoints";
import { LogManager, LogLevel, LogPhase } from "../debug/logManager";

export interface DebuggerProcessConfig {
  contractPath: string;
  snapshotPath?: string;
  entrypoint?: string;
  args?: unknown[];
  trace?: boolean;
  binaryPath?: string;
  port?: number;
  /**
   * Remote host to connect to when `spawnServer` is false (attach mode).
   * Defaults to `"127.0.0.1"` for local-only connections.
   */
  host?: string;
  token?: string;
  requestTimeoutMs?: number;
  connectTimeoutMs?: number;
  /**
   * When false, `start()` will only connect to an already-running debugger server
   * at `host`:`port` and will not spawn the CLI process.
   *
   * Set automatically when `request` is `"attach"` in `launch.json`.
   * Intended for remote-attach workflows and tests.
   */
  spawnServer?: boolean;
  storageFilter?: string[];
  repeat?: number;
}

export interface DebuggerExecutionResult {
  output: string;
  paused: boolean;
  completed: boolean;
  sourceLocation?: SourceLocation;
}

export interface DebuggerInspection {
  function?: string;
  args?: string;
  stepCount: number;
  paused: boolean;
  callStack: string[];
  sourceLocation?: SourceLocation;
}

export interface DebuggerContinueResult {
  completed: boolean;
  output?: string;
  paused: boolean;
  sourceLocation?: SourceLocation;
}

export interface DebuggerVersionInfo {
  backendVersion: string;
  protocolVersion: string;
}

export function getDebuggerVersionInfo(output: string): DebuggerVersionInfo {
  const backendMatch = output.match(/backend[:=]\s*([^\s]+)/i);
  const protocolMatch = output.match(/protocol[:=]\s*([^\s]+)/i);

  return {
    backendVersion: backendMatch?.[1] || "unknown",
    protocolVersion: protocolMatch?.[1] || "unknown",
  };
}

export interface BackendBreakpointCapabilities {
  conditionalBreakpoints: boolean;
  hitConditionalBreakpoints: boolean;
  logPoints: boolean;
}

export interface SourceLocation {
  file: string;
  line: number;
  column?: number;
}

export type LaunchPreflightQuickFix =
  | "pickBinary"
  | "pickContract"
  | "pickSnapshot"
  | "openLaunchConfig"
  | "generateLaunchConfig"
  | "openSettings";

export interface LaunchPreflightIssue {
  field:
    | "binaryPath"
    | "contractPath"
    | "snapshotPath"
    | "entrypoint"
    | "args"
    | "port"
    | "host"
    | "token";
  message: string;
  expected: string;
  quickFixes: LaunchPreflightQuickFix[];
}

export interface LaunchPreflightResult {
  ok: boolean;
  issues: LaunchPreflightIssue[];
  resolvedBinaryPath: string;
}

export type LaunchLifecyclePhase =
  | "spawn"
  | "connect"
  | "authenticate"
  | "load"
  | "ready";
export type LaunchLifecycleStatus = "started" | "completed" | "failed";

export interface LaunchLifecycleEvent {
  phase: LaunchLifecyclePhase;
  status: LaunchLifecycleStatus;
  message: string;
}

export class DebuggerTimeoutError extends Error {
  name = "DebuggerTimeoutError";

  constructor(
    public readonly requestType: string,
    public readonly timeoutMs: number,
  ) {
    super(`${requestType} timed out after ${timeoutMs}ms`);
  }
}

type ProtocolMismatchDetails = {
  extensionVersion: string;
  backendName?: string;
  backendVersion?: string;
  backendProtocolMin?: number;
  backendProtocolMax?: number;
  extra?: string;
};

export function formatProtocolMismatchMessage(
  details: ProtocolMismatchDetails,
): string {
  const backendIdentity =
    details.backendName && details.backendVersion
      ? `${details.backendName} ${details.backendVersion}`
      : "the backend debugger";
  const backendProtocol =
    Number.isInteger(details.backendProtocolMin) &&
    Number.isInteger(details.backendProtocolMax)
      ? `${details.backendProtocolMin}..=${details.backendProtocolMax}`
      : "unknown";
  const extra = details.extra ? ` Details: ${details.extra}` : "";

  return [
    `Protocol negotiation with ${backendIdentity} failed.`,
    `Extension version: ${details.extensionVersion}.`,
    `Backend supports protocol ${backendProtocol}.`,
    "Remediation: rebuild or update the soroban-debug CLI and the VS Code extension so they come from the same revision.",
    extra.trim(),
  ]
    .filter(Boolean)
    .join(" ");
}

type DebugRequest =
  | { type: 'Handshake'; client_name: string; client_version: string; protocol_min: number; protocol_max: number }
  | { type: 'Authenticate'; token: string }
  | { type: 'LoadContract'; contract_path: string }
  | { type: 'Execute'; function: string; args?: string }
  | { type: 'StepIn' }
  | { type: 'Next' }
  | { type: 'StepOut' }
  | { type: 'Continue' }
  | { type: 'Inspect' }
  | { type: 'GetStorage' }
  | { type: 'SetBreakpoint'; id?: string; function: string; condition?: string; hit_condition?: string; log_message?: string }
  | { type: 'ClearBreakpoint'; id?: string; function?: string }
  | { type: 'ResolveSourceBreakpoints'; source_path: string; lines: number[]; exported_functions: string[] }
  | { type: 'Evaluate'; expression: string; frame_id?: number }
  | { type: 'Cancel' }
  | { type: 'Ping' }
  | { type: 'Disconnect' }
  | { type: 'LoadSnapshot'; snapshot_path: string }
  | { type: 'GetCapabilities' }
  | { type: 'Unknown' };

type DebugResponse =
  | {
      type: "HandshakeAck";
      server_name: string;
      server_version: string;
      protocol_min: number;
      protocol_max: number;
      selected_version: number;
    }
  | {
      type: "IncompatibleProtocol";
      message: string;
      server_name: string;
      server_version: string;
      protocol_min: number;
      protocol_max: number;
    }
  | { type: "Authenticated"; success: boolean; message: string }
  | { type: "ContractLoaded"; size: number }
  | {
      type: "ExecutionResult";
      success: boolean;
      output: string;
      error?: string;
      paused: boolean;
      completed: boolean;
    }
  | {
      type: "StepResult";
      paused: boolean;
      current_function?: string;
      step_count: number;
    }
  | {
      type: "ContinueResult";
      completed: boolean;
      output?: string;
      error?: string;
      paused: boolean;
    }
  | {
      type: "InspectionResult";
      function?: string;
      args?: string;
      step_count: number;
      paused: boolean;
      call_stack: string[];
    }
  | { type: "StorageState"; storage_json: string }
  | { type: "SnapshotLoaded"; summary: string }
  | { type: "BreakpointSet"; function: string }
  | { type: "BreakpointCleared"; function: string }
  | {
      type: "SourceBreakpointsResolved";
      breakpoints: Array<{
        requested_line: number;
        line: number;
        verified: boolean;
        function?: string;
        reason_code: string;
        message: string;
      }>;
    }
  | {
      type: "EvaluateResult";
      result: string;
      result_type?: string;
      variables_reference: number;
    }
  | {
      type: "Capabilities";
      breakpoints: {
        conditional_breakpoints: boolean;
        hit_conditional_breakpoints: boolean;
        log_points: boolean;
      };
    }
  | { type: "Pong" }
  | { type: "Disconnected" }
  | { type: "Unknown" }
  | { type: "Error"; message: string };

type DebugMessage = {
  id: number;
  request?: DebugRequest;
  response?: DebugResponse;
};

type PendingRequest = {
  resolve: (response: DebugResponse) => void;
  reject: (error: Error) => void;
  cleanup: () => void;
};

type RequestOptions = {
  signal?: AbortSignal;
  timeoutMs?: number;
};

class RequestAbortedError extends Error {
  name = "AbortError";
  constructor(message = "Request aborted") {
    super(message);
  }
}

export class DebuggerProcess {
  private childProcess: ChildProcess | null = null;
  private socket: net.Socket | null = null;
  private buffer = "";
  private requestId = 0;
  private pendingRequests = new Map<number, PendingRequest>();
  private config: DebuggerProcessConfig;
  private logManager: LogManager | undefined;
  private port: number | null = null;
  private negotiatedProtocolVersion: number | null = null;
  private defaultRequestTimeoutMs: number;
  private defaultConnectTimeoutMs: number;
  private launchLifecycleReporter?: (event: LaunchLifecycleEvent) => void;

  constructor(
    config: DebuggerProcessConfig,
    logManager?: LogManager,
    launchLifecycleReporter?: (event: LaunchLifecycleEvent) => void,
  ) {
    this.config = config;
    this.logManager = logManager;
    this.launchLifecycleReporter = launchLifecycleReporter;

    const envRequestTimeout = Number(
      process.env.SOROBAN_DEBUG_REQUEST_TIMEOUT_MS,
    );
    const envConnectTimeout = Number(
      process.env.SOROBAN_DEBUG_CONNECT_TIMEOUT_MS,
    );

    this.defaultRequestTimeoutMs = Number.isFinite(config.requestTimeoutMs)
      ? Number(config.requestTimeoutMs)
      : Number.isFinite(envRequestTimeout)
        ? envRequestTimeout
        : 30_000;

    this.defaultConnectTimeoutMs = Number.isFinite(config.connectTimeoutMs)
      ? Number(config.connectTimeoutMs)
      : Number.isFinite(envConnectTimeout)
        ? envConnectTimeout
        : 10_000;
  }

  public cancel(): void {
    if (!this.childProcess || this.childProcess.killed) return;
    this.sendRequest({ type: "Cancel" }).catch(() => {
      // Ignore network errors since the server dropping the connection
      // is the expected behavior for an interrupted execution
    });
  }

  async start(): Promise<void> {
    if (this.childProcess || this.socket) {
      return;
    }

    const shouldSpawnServer = this.config.spawnServer !== false;
    const binaryPath = shouldSpawnServer
      ? resolveDebuggerBinaryPath(this.config)
      : null;
    const port = this.config.port ?? (await this.findAvailablePort());
    this.port = port;
    let activePhase: LaunchLifecyclePhase = shouldSpawnServer
      ? "spawn"
      : "connect";

    try {
      if (shouldSpawnServer) {
        const child = spawn(binaryPath as string, this.buildArgs(port), {
          stdio: ["ignore", "pipe", "pipe"],
          env: {
            ...process.env,
            ...(this.config.trace ? { RUST_LOG: "debug" } : {}),
          },
        });
        this.childProcess = child;

        child.once("exit", () => {
          this.rejectPendingRequests(new Error("Debugger server exited"));
          this.socket?.destroy();
          this.socket = null;
        });
      } else if (!this.config.port) {
        throw new Error(
          "DebuggerProcessConfig.port is required when spawnServer is false",
        );
      }

      activePhase = "connect";
      this.emitLaunchLifecycle({
        phase: "connect",
        status: "started",
        message: `Connecting to debugger server on ${this.config.host ?? "127.0.0.1"}:${port}...`,
      });
      await this.waitForServer(port);
      this.logManager?.log(
        LogLevel.Info,
        LogPhase.Connect,
        `Connecting to debugger server on ${this.config.host ?? "127.0.0.1"}:${port}...`,
      );
      await this.connect(port);
      this.logManager?.log(
        LogLevel.Info,
        LogPhase.Connect,
        "Connection established. Negotiating protocol...",
      );
      await this.negotiateProtocol();
      this.logManager?.log(
        LogLevel.Info,
        LogPhase.Connect,
        `Protocol negotiated: ${this.negotiatedProtocolVersion || "unknown"}`,
      );
      this.emitLaunchLifecycle({
        phase: "connect",
        status: "completed",
        message: `Connected to debugger server on ${this.config.host ?? "127.0.0.1"}:${port}.`,
      });

      if (this.config.token) {
        activePhase = "authenticate";
        this.emitLaunchLifecycle({
          phase: "authenticate",
          status: "started",
          message: "Authenticating debugger session...",
        });
        this.logManager?.log(
          LogLevel.Info,
          LogPhase.Auth,
          "Authenticating with token...",
        );
        const response = await this.sendRequest({
          type: "Authenticate",
          token: this.config.token,
        });
        this.expectResponse(response, "Authenticated");
        if (!response.success) {
          throw new Error(response.message);
        }
        this.emitLaunchLifecycle({
          phase: "authenticate",
          status: "completed",
          message: "Debugger session authenticated.",
        });
      }

      activePhase = "load";
      this.emitLaunchLifecycle({
        phase: "load",
        status: "started",
        message: this.config.snapshotPath
          ? "Loading snapshot and contract into debugger..."
          : "Loading contract into debugger...",
      });
      if (this.config.snapshotPath) {
        this.logManager?.log(
          LogLevel.Info,
          LogPhase.Load,
          `Loading snapshot: ${this.config.snapshotPath}`,
        );
        const response = await this.sendRequest({
          type: "LoadSnapshot",
          snapshot_path: this.config.snapshotPath,
        });
        this.expectResponse(response, "SnapshotLoaded");
      }

      this.logManager?.log(
        LogLevel.Info,
        LogPhase.Load,
        `Loading contract: ${this.config.contractPath}`,
      );
      const contractResponse = await this.sendRequest({
        type: "LoadContract",
        contract_path: this.config.contractPath,
      });
      this.expectResponse(contractResponse, "ContractLoaded");
      this.emitLaunchLifecycle({
        phase: "load",
        status: "completed",
        message: "Snapshot and contract loaded.",
      });
      activePhase = "ready";
      this.emitLaunchLifecycle({
        phase: "ready",
        status: "completed",
        message: "Debugger is ready.",
      });
    } catch (error) {
      this.emitLaunchLifecycle({
        phase: activePhase,
        status: "failed",
        message: error instanceof Error ? error.message : String(error),
      });
      await this.stop().catch(() => undefined);
      throw error;
    }
  }

  async execute(): Promise<DebuggerExecutionResult> {
    const response = await this.sendRequest({
      type: "Execute",
      function: this.config.entrypoint || "main",
      args:
        this.config.args && this.config.args.length > 0
          ? JSON.stringify(this.config.args)
          : undefined,
    });
    this.expectResponse(response, "ExecutionResult");

    if (!response.success) {
      throw new Error(response.error || "Execution failed");
    }

    return {
      output: response.output,
      paused: response.paused,
      completed: response.completed,
    };
  }

  async stepIn(): Promise<{
    paused: boolean;
    current_function?: string;
    step_count: number;
  }> {
    const response = await this.sendRequest({ type: "StepIn" });
    this.expectResponse(response, "StepResult");
    return response as any;
  }

  async next(): Promise<{
    paused: boolean;
    current_function?: string;
    step_count: number;
  }> {
    const response = await this.sendRequest({ type: "Next" });
    this.expectResponse(response, "StepResult");
    return response as any;
  }

  async stepOut(): Promise<{
    paused: boolean;
    current_function?: string;
    step_count: number;
  }> {
    const response = await this.sendRequest({ type: "StepOut" });
    this.expectResponse(response, "StepResult");
    return response as any;
  }

  async continueExecution(): Promise<DebuggerContinueResult> {
    const response = await this.sendRequest({ type: "Continue" });
    this.expectResponse(response, "ContinueResult");
    if (response.error) {
      throw new Error(response.error);
    }
    return {
      completed: response.completed,
      output: response.output,
      paused: response.paused,
    };
  }

  async inspect(options?: RequestOptions): Promise<DebuggerInspection> {
    const response = await this.sendRequest({ type: "Inspect" }, options);
    this.expectResponse(response, "InspectionResult");
    return {
      function: response.function,
      args: response.args,
      stepCount: response.step_count,
      paused: response.paused,
      callStack: response.call_stack,
    };
  }

  async getStorage(options?: RequestOptions): Promise<Record<string, unknown>> {
    const response = await this.sendRequest({ type: "GetStorage" }, options);
    this.expectResponse(response, "StorageState");
    const parsed = JSON.parse(response.storage_json);
    if (parsed && typeof parsed === "object") {
      return parsed as Record<string, unknown>;
    }
    return {};
  }

  async ping(): Promise<void> {
    const response = await this.sendRequest({ type: "Ping" });
    this.expectResponse(response, "Pong");
  }

  async getCapabilities(): Promise<BackendBreakpointCapabilities> {
    const response = await this.sendRequest({ type: "GetCapabilities" });
    this.expectResponse(response, "Capabilities");
    return {
      conditionalBreakpoints: response.breakpoints.conditional_breakpoints,
      hitConditionalBreakpoints:
        response.breakpoints.hit_conditional_breakpoints,
      logPoints: response.breakpoints.log_points,
    };
  }

  async setBreakpoint(breakpoint: {
    id: string;
    functionName: string;
    condition?: string;
    hitCondition?: string;
    logMessage?: string;
  }): Promise<void> {
    const response = await this.sendRequest({
      type: "SetBreakpoint",
      id: breakpoint.id,
      function: breakpoint.functionName,
      condition: breakpoint.condition,
      hit_condition: breakpoint.hitCondition,
      log_message: breakpoint.logMessage
    }as any);
    this.expectResponse(response, 'BreakpointSet');
  }

  async clearBreakpoint(breakpointId: string): Promise<void> {
    const response = await this.sendRequest({
      type: 'ClearBreakpoint',
      id: breakpointId
    } as any);
    this.expectResponse(response, 'BreakpointCleared');
  }

  async evaluate(
    expression: string,
    frameId?: number,
    options?: RequestOptions,
  ): Promise<{ result: string; type?: string; variablesReference: number }> {
    const response = await this.sendRequest(
      {
        type: "Evaluate",
        expression,
        frame_id: frameId,
      },
      options,
    );
    this.expectResponse(response, "EvaluateResult");
    return {
      result: response.result,
      type: response.result_type,
      variablesReference: response.variables_reference,
    };
  }

  async getContractFunctions(): Promise<Set<string>> {
    const binaryPath = resolveDebuggerBinaryPath(this.config);

    const output = await new Promise<string>((resolve, reject) => {
      const child = execFile(
        binaryPath,
        ["inspect", "--contract", this.config.contractPath, "--functions"],
        { env: process.env },
        (error: Error | null, stdout: string, stderr: string) => {
          clearTimeout(timer);
          if (error) {
            reject(new Error(stderr || stdout || String(error)));
            return;
          }
          resolve(stdout);
        },
      );

      const timer = setTimeout(() => {
        child.kill();
        reject(
          new DebuggerTimeoutError(
            "InspectFunctions",
            this.defaultRequestTimeoutMs,
          ),
        );
      }, this.defaultRequestTimeoutMs);
    });

    const functions = new Set<string>();
    for (const line of output.split(/\r?\n/)) {
      const match = line.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\(/);
      if (match) {
        functions.add(match[1]);
      }
    }

    return functions;
  }

  async resolveSourceBreakpoints(
    sourcePath: string,
    lines: number[],
    exportedFunctions: Set<string>,
    options?: RequestOptions,
  ): Promise<
    Array<{
      requestedLine: number;
      line: number;
      verified: boolean;
      functionName?: string;
      reasonCode: string;
      message: string;
    }>
  > {
    const response = await this.sendRequest(
      {
        type: "ResolveSourceBreakpoints",
        source_path: sourcePath,
        lines,
        exported_functions: Array.from(exportedFunctions),
      },
      options,
    );

    this.expectResponse(response, "SourceBreakpointsResolved");

    return response.breakpoints.map((bp) => ({
      requestedLine: bp.requested_line,
      line: bp.line,
      verified: bp.verified,
      functionName: bp.function,
      reasonCode: bp.reason_code,
      message: bp.message,
      setBreakpoint: shouldPromoteToFunctionBreakpoint(bp.verified, bp.function, bp.reason_code),
    }));
  }

  async stop(): Promise<void> {
    try {
      if (this.socket && !this.socket.destroyed) {
        await this.sendRequest({ type: "Disconnect" }).catch(() => undefined);
      }
    } finally {
      this.socket?.destroy();
      this.socket = null;
    }

    if (!this.childProcess) {
      return;
    }

    if (this.childProcess.killed) {
      this.childProcess = null;
      return;
    }

    await new Promise<void>((resolve) => {
      if (!this.childProcess) {
        resolve();
        return;
      }

      const child = this.childProcess;
      const timeout = setTimeout(() => {
        if (!child.killed) {
          child.kill("SIGKILL");
        }
      }, 5000);

      child.once("exit", () => {
        clearTimeout(timeout);
        resolve();
      });
      child.kill("SIGTERM");
    });

    this.childProcess = null;
  }

  getOutputStream() {
    return this.childProcess?.stdout;
  }

  getErrorStream() {
    return this.childProcess?.stderr;
  }

  private buildArgs(port: number): string[] {
    const args = ["server", "--port", String(port)];

    if (this.config.token) {
      args.push("--token", this.config.token);
    }

    if (this.config.storageFilter && this.config.storageFilter.length > 0) {
      for (const filter of this.config.storageFilter) {
        args.push("--storage-filter", filter);
      }
    }

    if (this.config.repeat && this.config.repeat > 1) {
      args.push("--repeat", String(this.config.repeat));
    }

    return args;
  }

  isRunning(): boolean {
    return (
      this.childProcess !== null &&
      this.socket !== null &&
      !this.socket.destroyed
    );
  }

  private async findAvailablePort(): Promise<number> {
    return await new Promise<number>((resolve, reject) => {
      const server = net.createServer();
      server.listen(0, "127.0.0.1", () => {
        const address = server.address();
        if (!address || typeof address === "string") {
          reject(new Error("Failed to determine an available port"));
          return;
        }

        const port = address.port;
        server.close((error: Error | undefined) => {
          if (error) {
            reject(error);
            return;
          }
          resolve(port);
        });
      });
      server.on("error", reject);
    });
  }

  private async waitForServer(port: number): Promise<void> {
    const deadline = Date.now() + this.defaultConnectTimeoutMs;

    while (Date.now() < deadline) {
      if (this.childProcess && this.childProcess.exitCode !== null) {
        throw new Error(
          `Debugger server exited with code ${this.childProcess.exitCode}`,
        );
      }

      if (await this.canConnect(port)) {
        return;
      }

      await new Promise((resolve) => setTimeout(resolve, 100));
    }

    throw new Error(`Timed out waiting for debugger server on ${this.config.host ?? "127.0.0.1"}:${port}`);
  }

  private async canConnect(port: number): Promise<boolean> {
    const host = this.config.host ?? "127.0.0.1";
    return await new Promise<boolean>((resolve) => {
      const socket = net.createConnection({ host, port }, () => {
        socket.destroy();
        resolve(true);
      });

      socket.on("error", () => {
        socket.destroy();
        resolve(false);
      });
    });
  }

  private async connect(port: number): Promise<void> {
    const host = this.config.host ?? "127.0.0.1";
    await new Promise<void>((resolve, reject) => {
      const socket = net.createConnection({ host, port }, () => {
        this.socket = socket;
        resolve();
      });

      socket.setEncoding("utf8");
      socket.on("data", (chunk: string) => {
        this.buffer += chunk;
        this.consumeMessages();
      });
      socket.on("error", reject);
      socket.on("close", () => {
        this.rejectPendingRequests(new Error("Debugger connection closed"));
        this.socket = null;
      });
    });
  }

  private consumeMessages(): void {
    while (true) {
      const newlineIndex = this.buffer.indexOf("\n");
      if (newlineIndex === -1) {
        return;
      }

      const line = this.buffer.slice(0, newlineIndex).trim();
      this.buffer = this.buffer.slice(newlineIndex + 1);

      if (!line) {
        continue;
      }

      let message: DebugMessage;
      try {
        message = JSON.parse(line) as DebugMessage;
      } catch (err) {
        this.logManager?.log(
          LogLevel.Error,
          LogPhase.Connect,
          `Failed to parse backend message: ${err}\nLine: ${line}`,
        );
        continue;
      }
      const pending = this.pendingRequests.get(message.id);
      if (!pending || !message.response) {
        continue;
      }

      this.pendingRequests.delete(message.id);
      pending.cleanup();
      pending.resolve(message.response);
    }
  }

  private async sendRequest(
    request: DebugRequest,
    options?: RequestOptions,
  ): Promise<DebugResponse> {
    if (!this.socket) {
      throw new Error("Debugger connection is not established");
    }

    if (options?.signal?.aborted) {
      throw new RequestAbortedError();
    }

    this.requestId += 1;
    const id = this.requestId;
    const message: DebugMessage = { id, request };

    const responsePromise = new Promise<DebugResponse>((resolve, reject) => {
      const cleanup = () => {
        if (timeout) {
          clearTimeout(timeout);
          timeout = undefined;
        }
        if (abortHandler && options?.signal) {
          options.signal.removeEventListener("abort", abortHandler);
        }
      };

      let timeout: NodeJS.Timeout | undefined;
      let abortHandler: (() => void) | undefined;

      const timeoutMs = options?.timeoutMs ?? this.defaultRequestTimeoutMs;

      if (timeoutMs > 0) {
        timeout = setTimeout(() => {
          const pending = this.pendingRequests.get(id);
          if (!pending) {
            return;
          }
          this.pendingRequests.delete(id);
          pending.cleanup();
          pending.reject(new DebuggerTimeoutError(request.type, timeoutMs));
        }, timeoutMs);
      }

      if (options?.signal) {
        abortHandler = () => {
          const pending = this.pendingRequests.get(id);
          if (!pending) {
            return;
          }
          this.pendingRequests.delete(id);
          pending.cleanup();
          pending.reject(new RequestAbortedError());
        };
        options.signal.addEventListener("abort", abortHandler, { once: true });
      }

      this.pendingRequests.set(id, { resolve, reject, cleanup });
    });

    this.logManager?.log(
      LogLevel.Debug,
      LogPhase.Connect,
      `Backend request [${id}]: ${JSON.stringify(request)}`,
    );
    this.socket.write(`${JSON.stringify(message)}\n`);
    const response = await responsePromise;
    if (response.type === "Error") {
      throw new Error(response.message);
    }
    return response;
  }

  private getExtensionVersion(): string {
    try {
      const packageJsonPath = path.resolve(
        __dirname,
        "..",
        "..",
        "package.json",
      );
      const pkg = JSON.parse(fs.readFileSync(packageJsonPath, "utf8")) as {
        version?: string;
      };
      return pkg.version || "unknown";
    } catch {
      return "unknown";
    }
  }

  private async negotiateProtocol(): Promise<void> {
    const extensionVersion = this.getExtensionVersion();

    let response: DebugResponse;
    try {
      response = await this.sendRequest(
        {
          type: "Handshake",
          client_name: "vscode-extension",
          client_version: extensionVersion,
          protocol_min: WIRE_PROTOCOL_MIN_VERSION,
          protocol_max: WIRE_PROTOCOL_MAX_VERSION,
        },
        { timeoutMs: 2_500 },
      );
    } catch (error) {
      throw new Error(
        formatProtocolMismatchMessage({
          extensionVersion,
          extra: String(error),
        }),
      );
    }

    if (response.type === "HandshakeAck") {
      this.negotiatedProtocolVersion = response.selected_version;
      return;
    }

    if (response.type === "IncompatibleProtocol") {
      throw new Error(
        formatProtocolMismatchMessage({
          extensionVersion,
          backendName: response.server_name,
          backendVersion: response.server_version,
          backendProtocolMin: response.protocol_min,
          backendProtocolMax: response.protocol_max,
          extra: response.message,
        }),
      );
    }

    throw new Error(
      formatProtocolMismatchMessage({
        extensionVersion,
        extra: `Unexpected handshake response: ${response.type}`,
      }),
    );
  }

  private rejectPendingRequests(error: Error): void {
    for (const pending of this.pendingRequests.values()) {
      pending.cleanup();
      pending.reject(error);
    }
    this.pendingRequests.clear();
  }

  private expectResponse<T extends DebugResponse["type"]>(
    response: DebugResponse,
    type: T,
  ): asserts response is Extract<DebugResponse, { type: T }> {
    if (response.type !== type) {
      throw new Error(
        `Unexpected debugger response: expected ${type}, got ${response.type}`,
      );
    }
  }

  private emitLaunchLifecycle(event: LaunchLifecycleEvent): void {
    this.launchLifecycleReporter?.(event);
  }
}

function debuggerBinaryName(): string {
  return process.platform === "win32" ? "soroban-debug.exe" : "soroban-debug";
}

function looksLikeVariableReference(value: string): boolean {
  return value.includes("${");
}

export function resolveDebuggerBinaryPath(
  config: DebuggerProcessConfig,
): string {
  const configured = config.binaryPath?.trim();
  if (configured) {
    return configured;
  }

  const envOverride = process.env.SOROBAN_DEBUG_BIN?.trim();
  if (envOverride) {
    return envOverride;
  }

  return debuggerBinaryName();
}

export async function validateLaunchConfig(
  config: DebuggerProcessConfig,
): Promise<LaunchPreflightResult> {
  const issues: LaunchPreflightIssue[] = [];
  const resolvedBinaryPath = resolveDebuggerBinaryPath(config);

  if (!looksLikeVariableReference(resolvedBinaryPath)) {
    pushFileIssue(
      issues,
      "binaryPath",
      resolvedBinaryPath,
      "a readable soroban-debug binary path or a command available on PATH.",
      ["pickBinary", "openLaunchConfig", "openSettings"],
    );
  }

  if (!config.contractPath || config.contractPath.trim().length === 0) {
    issues.push({
      field: "contractPath",
      message:
        "Launch config field 'contractPath' must point to a readable contract WASM file.",
      expected: "A readable .wasm file.",
      quickFixes: ["pickContract", "openLaunchConfig", "generateLaunchConfig"],
    });
  } else if (!looksLikeVariableReference(config.contractPath)) {
    pushFileIssue(
      issues,
      "contractPath",
      config.contractPath,
      "a readable contract WASM file.",
      ["pickContract", "openLaunchConfig", "generateLaunchConfig"],
    );
  }

  if (config.snapshotPath && !looksLikeVariableReference(config.snapshotPath)) {
    pushFileIssue(
      issues,
      "snapshotPath",
      config.snapshotPath,
      "a readable snapshot JSON file.",
      ["pickSnapshot", "openLaunchConfig", "generateLaunchConfig"],
    );
  }

  if (
    config.entrypoint !== undefined &&
    config.entrypoint.trim().length === 0
  ) {
    issues.push({
      field: "entrypoint",
      message: "Launch config field 'entrypoint' must be a non-empty string.",
      expected: "A Soroban function name such as 'main' or 'transfer'.",
      quickFixes: ["openLaunchConfig", "generateLaunchConfig"],
    });
  }

  const argsIssue = validateArgs(config.args ?? []);
  if (argsIssue) {
    issues.push(argsIssue);
  }

  if (config.port !== undefined) {
    if (
      !Number.isInteger(config.port) ||
      config.port < 1 ||
      config.port > 65_535
    ) {
      issues.push({
        field: "port",
        message: `Launch config field 'port' must be an integer between 1 and 65535; received ${String(config.port)}.`,
        expected: "An available TCP port between 1 and 65535.",
        quickFixes: ["openLaunchConfig"],
      });
    } else if (config.spawnServer !== false && !(await isPortAvailable(config.port))) {
      // Only check port availability when we are spawning a local server.
      // In attach mode (spawnServer: false) the port must already be in use.
      issues.push({
        field: "port",
        message: `Launch config field 'port' is set to ${config.port}, but that port is already in use on 127.0.0.1.`,
        expected: "An available TCP port between 1 and 65535.",
        quickFixes: ["openLaunchConfig"],
      });
    }
  }

  if (config.host !== undefined) {
    if (typeof config.host !== "string" || config.host.trim().length === 0) {
      issues.push({
        field: "host",
        message: "Launch config field 'host' must be a non-empty string.",
        expected: "A hostname or IP address such as '192.168.1.10' or 'debug.example.com'.",
        quickFixes: ["openLaunchConfig"],
      });
    }
  }

  if (config.token !== undefined) {
    if (config.token.trim().length === 0 || /[\r\n]/.test(config.token)) {
      issues.push({
        field: "token",
        message:
          "Launch config field 'token' must be a single-line non-empty string.",
        expected: "A non-empty authentication token without line breaks.",
        quickFixes: ["openLaunchConfig"],
      });
    }
  }

  return {
    ok: issues.length === 0,
    issues,
    resolvedBinaryPath,
  };
}

function pushFileIssue(
  issues: LaunchPreflightIssue[],
  field: "binaryPath" | "contractPath" | "snapshotPath",
  filePath: string | undefined,
  expected: string,
  quickFixes: LaunchPreflightQuickFix[],
): void {
  if (!filePath || filePath.trim().length === 0) {
    issues.push({
      field,
      message: `Launch config field '${field}' must point to ${expected}.`,
      expected,
      quickFixes,
    });
    return;
  }

  if (field === "binaryPath" && isCommandOnPath(filePath)) {
    return;
  }

  try {
    const stat = fs.statSync(filePath);
    if (!stat.isFile()) {
      issues.push({
        field,
        message: `Launch config field '${field}' points to '${filePath}', but that path is not a file.`,
        expected,
        quickFixes,
      });
      return;
    }

    fs.accessSync(filePath, fs.constants.R_OK);
  } catch {
    issues.push({
      field,
      message: `Launch config field '${field}' points to '${filePath}', but the file does not exist or is not readable.`,
      expected,
      quickFixes,
    });
  }
}

function isCommandOnPath(command: string): boolean {
  if (
    path.isAbsolute(command) ||
    command.includes(path.sep) ||
    command.includes("/")
  ) {
    return false;
  }

  const pathValue = process.env.PATH;
  if (!pathValue) {
    return false;
  }

  const extensions =
    process.platform === "win32"
      ? (process.env.PATHEXT || ".EXE;.CMD;.BAT;.COM")
          .split(";")
          .filter((ext) => ext.length > 0)
      : [""];

  for (const directory of pathValue.split(path.delimiter)) {
    for (const extension of extensions) {
      const candidate = path.join(
        directory,
        command.endsWith(extension) ? command : `${command}${extension}`,
      );
      if (fs.existsSync(candidate)) {
        return true;
      }
    }
  }

  return false;
}

function validateArgs(args: unknown): LaunchPreflightIssue | undefined {
  if (!Array.isArray(args)) {
    return {
      field: "args",
      message: `Launch config field 'args' must be an array; received ${describeValue(args)}.`,
      expected: 'A JSON array such as [] or ["alice", 10].',
      quickFixes: ["openLaunchConfig", "generateLaunchConfig"],
    };
  }

  const seen = new Set<unknown>();
  const invalidPath = findNonSerializableValue(args, "$", seen);
  if (invalidPath) {
    return {
      field: "args",
      message: `Launch config field 'args' contains a non-JSON-serializable value at ${invalidPath}.`,
      expected:
        "Only JSON-serializable values: strings, numbers, booleans, null, arrays, and objects.",
      quickFixes: ["openLaunchConfig", "generateLaunchConfig"],
    };
  }

  return undefined;
}

function findNonSerializableValue(
  value: unknown,
  pathLabel: string,
  seen: Set<unknown>,
): string | undefined {
  if (
    value === null ||
    typeof value === "string" ||
    typeof value === "boolean"
  ) {
    return undefined;
  }

  if (typeof value === "number") {
    return Number.isFinite(value) ? undefined : pathLabel;
  }

  if (
    typeof value === "undefined" ||
    typeof value === "function" ||
    typeof value === "symbol" ||
    typeof value === "bigint"
  ) {
    return pathLabel;
  }

  if (Array.isArray(value)) {
    if (seen.has(value)) {
      return pathLabel;
    }
    seen.add(value);
    for (let index = 0; index < value.length; index += 1) {
      const found = findNonSerializableValue(
        value[index],
        `${pathLabel}[${index}]`,
        seen,
      );
      if (found) {
        return found;
      }
    }
    seen.delete(value);
    return undefined;
  }

  if (typeof value === "object") {
    const record = value as Record<string, unknown>;
    if (seen.has(record)) {
      return pathLabel;
    }
    seen.add(record);
    for (const [key, item] of Object.entries(record)) {
      const found = findNonSerializableValue(item, `${pathLabel}.${key}`, seen);
      if (found) {
        return found;
      }
    }
    seen.delete(record);
    return undefined;
  }

  return pathLabel;
}

function describeValue(value: unknown): string {
  if (Array.isArray(value)) {
    return "an array";
  }
  if (value === null) {
    return "null";
  }
  return typeof value;
}

async function isPortAvailable(port: number): Promise<boolean> {
  return await new Promise<boolean>((resolve) => {
    const server = net.createServer();
    server.once("error", () => {
      server.close();
      resolve(false);
    });
    server.once("listening", () => {
      server.close(() => resolve(true));
    });
    server.listen(port, "127.0.0.1");
  });
}
