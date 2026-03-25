import { ChildProcess, execFile, spawn } from 'child_process';
import * as fs from 'fs';
import * as net from 'net';
import * as path from 'path';

export interface DebuggerProcessConfig {
  contractPath: string;
  snapshotPath?: string;
  entrypoint?: string;
  args?: string[];
  trace?: boolean;
  binaryPath?: string;
  port?: number;
  token?: string;
}

export interface DebuggerExecutionResult {
  output: string;
  paused: boolean;
  completed: boolean;
}

export interface DebuggerInspection {
  function?: string;
  args?: string;
  stepCount: number;
  paused: boolean;
  callStack: string[];
}

export interface DebuggerContinueResult {
  completed: boolean;
  output?: string;
  paused: boolean;
}

export interface DebuggerSessionInfo {
  backendVersion: string;
  protocolVersion: string;
}

type DebugRequest =
  | { type: 'Authenticate'; token: string }
  | { type: 'LoadContract'; contract_path: string }
  | { type: 'Execute'; function: string; args?: string }
  | { type: 'StepIn' }
  | { type: 'Next' }
  | { type: 'StepOut' }
  | { type: 'Continue' }
  | { type: 'Inspect' }
  | { type: 'GetStorage' }
  | { type: 'SetBreakpoint'; function: string }
  | { type: 'ClearBreakpoint'; function: string }
  | { type: 'Evaluate'; expression: string; frame_id?: number }
  | { type: 'Ping' }
  | { type: 'Disconnect' }
  | { type: 'LoadSnapshot'; snapshot_path: string };

type DebugResponse =
  | { type: 'Authenticated'; success: boolean; message: string }
  | { type: 'ContractLoaded'; size: number }
  | { type: 'ExecutionResult'; success: boolean; output: string; error?: string; paused: boolean; completed: boolean }
  | { type: 'StepResult'; paused: boolean; current_function?: string; step_count: number }
  | { type: 'ContinueResult'; completed: boolean; output?: string; error?: string; paused: boolean }
  | { type: 'InspectionResult'; function?: string; args?: string; step_count: number; paused: boolean; call_stack: string[] }
  | { type: 'StorageState'; storage_json: string }
  | { type: 'SnapshotLoaded'; summary: string }
  | { type: 'BreakpointSet'; function: string }
  | { type: 'BreakpointCleared'; function: string }
  | { type: 'EvaluateResult'; result: string; result_type?: string; variables_reference: number }
  | { type: 'Pong'; backend_version?: string; protocol_version?: string }
  | { type: 'Disconnected' }
  | { type: 'Error'; message: string };

type DebugMessage = {
  id: number;
  request?: DebugRequest;
  response?: DebugResponse;
};

type PendingRequest = {
  resolve: (response: DebugResponse) => void;
  reject: (error: Error) => void;
};

export class DebuggerProcess {
  private process: ChildProcess | null = null;
  private socket: net.Socket | null = null;
  private buffer = '';
  private requestId = 0;
  private pendingRequests = new Map<number, PendingRequest>();
  private config: DebuggerProcessConfig;
  private port: number | null = null;
  private sessionInfo: DebuggerSessionInfo = {
    backendVersion: 'unknown',
    protocolVersion: 'unknown'
  };

  constructor(config: DebuggerProcessConfig) {
    this.config = config;
  }

  async start(): Promise<void> {
    if (this.process || this.socket) {
      return;
    }

    const binaryPath = this.resolveBinaryPath();
    const port = this.config.port ?? await this.findAvailablePort();
    this.port = port;

    const child = spawn(binaryPath, this.buildArgs(port), {
      stdio: ['ignore', 'pipe', 'pipe'],
      env: {
        ...process.env,
        ...(this.config.trace ? { RUST_LOG: 'debug' } : {})
      }
    });
    this.process = child;

    child.once('exit', () => {
      this.rejectPendingRequests(new Error('Debugger server exited'));
      this.socket?.destroy();
      this.socket = null;
    });

    await this.waitForServer(port);
    await this.connect(port);

    if (this.config.token) {
      const response = await this.sendRequest({
        type: 'Authenticate',
        token: this.config.token
      });
      this.expectResponse(response, 'Authenticated');
      if (!response.success) {
        throw new Error(response.message);
      }
    }

    if (this.config.snapshotPath) {
      const response = await this.sendRequest({
        type: 'LoadSnapshot',
        snapshot_path: this.config.snapshotPath
      });
      this.expectResponse(response, 'SnapshotLoaded');
    }

    const contractResponse = await this.sendRequest({
      type: 'LoadContract',
      contract_path: this.config.contractPath
    });
    this.expectResponse(contractResponse, 'ContractLoaded');

    // Best-effort metadata fetch; do not fail startup if unavailable.
    await this.captureSessionInfo();
  }

  async execute(): Promise<DebuggerExecutionResult> {
    const response = await this.sendRequest({
      type: 'Execute',
      function: this.config.entrypoint || 'main',
      args: this.config.args && this.config.args.length > 0
        ? JSON.stringify(this.config.args)
        : undefined
    });
    this.expectResponse(response, 'ExecutionResult');

    if (!response.success) {
      throw new Error(response.error || 'Execution failed');
    }

    return {
      output: response.output,
      paused: response.paused,
      completed: response.completed
    };
  }

  async stepIn(): Promise<{ paused: boolean; current_function?: string; step_count: number }> {
    const response = await this.sendRequest({ type: 'StepIn' });
    this.expectResponse(response, 'StepResult');
    return response as any;
  }

  async next(): Promise<{ paused: boolean; current_function?: string; step_count: number }> {
    const response = await this.sendRequest({ type: 'Next' });
    this.expectResponse(response, 'StepResult');
    return response as any;
  }

  async stepOut(): Promise<{ paused: boolean; current_function?: string; step_count: number }> {
    const response = await this.sendRequest({ type: 'StepOut' });
    this.expectResponse(response, 'StepResult');
    return response as any;
  }

  async continueExecution(): Promise<DebuggerContinueResult> {
    const response = await this.sendRequest({ type: 'Continue' });
    this.expectResponse(response, 'ContinueResult');
    if (response.error) {
      throw new Error(response.error);
    }
    return {
      completed: response.completed,
      output: response.output,
      paused: response.paused
    };
  }

  async inspect(): Promise<DebuggerInspection> {
    const response = await this.sendRequest({ type: 'Inspect' });
    this.expectResponse(response, 'InspectionResult');
    return {
      function: response.function,
      args: response.args,
      stepCount: response.step_count,
      paused: response.paused,
      callStack: response.call_stack
    };
  }

  async getStorage(): Promise<Record<string, unknown>> {
    const response = await this.sendRequest({ type: 'GetStorage' });
    this.expectResponse(response, 'StorageState');
    const parsed = JSON.parse(response.storage_json);
    if (parsed && typeof parsed === 'object') {
      return parsed as Record<string, unknown>;
    }
    return {};
  }

  async ping(): Promise<void> {
    const response = await this.sendRequest({ type: 'Ping' });
    this.expectResponse(response, 'Pong');
    this.sessionInfo = extractSessionInfoFromPong(response);
  }

  getSessionInfo(): DebuggerSessionInfo {
    return { ...this.sessionInfo };
  }

  async setBreakpoint(functionName: string): Promise<void> {
    const response = await this.sendRequest({
      type: 'SetBreakpoint',
      function: functionName
    });
    this.expectResponse(response, 'BreakpointSet');
  }

  async clearBreakpoint(functionName: string): Promise<void> {
    const response = await this.sendRequest({
      type: 'ClearBreakpoint',
      function: functionName
    });
    this.expectResponse(response, 'BreakpointCleared');
  }

  async evaluate(expression: string, frameId?: number): Promise<{ result: string; type?: string; variablesReference: number }> {
    const response = await this.sendRequest({
      type: 'Evaluate',
      expression,
      frame_id: frameId
    });
    this.expectResponse(response, 'EvaluateResult');
    return {
      result: response.result,
      type: response.result_type,
      variablesReference: response.variables_reference
    };
  }

  async getContractFunctions(): Promise<Set<string>> {
    const binaryPath = this.resolveBinaryPath();

    const output = await new Promise<string>((resolve, reject) => {
      execFile(
        binaryPath,
        ['inspect', '--contract', this.config.contractPath, '--functions'],
        { env: process.env },
        (error, stdout, stderr) => {
          if (error) {
            reject(new Error(stderr || stdout || String(error)));
            return;
          }
          resolve(stdout);
        }
      );
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

  async stop(): Promise<void> {
    try {
      if (this.socket && !this.socket.destroyed) {
        await this.sendRequest({ type: 'Disconnect' }).catch(() => undefined);
      }
    } finally {
      this.socket?.destroy();
      this.socket = null;
    }

    if (!this.process) {
      return;
    }

    if (this.process.killed) {
      this.process = null;
      return;
    }

    await new Promise<void>((resolve) => {
      if (!this.process) {
        resolve();
        return;
      }

      const child = this.process;
      const timeout = setTimeout(() => {
        if (!child.killed) {
          child.kill('SIGKILL');
        }
      }, 5000);

      child.once('exit', () => {
        clearTimeout(timeout);
        resolve();
      });
      child.kill('SIGTERM');
    });

    this.process = null;
  }

  getInputStream() {
    return null;
  }

  getOutputStream() {
    return this.process?.stdout;
  }

  getErrorStream() {
    return this.process?.stderr;
  }

  private buildArgs(port: number): string[] {
    const args = ['server', '--port', String(port)];

    if (this.config.token) {
      args.push('--token', this.config.token);
    }

    return args;
  }

  isRunning(): boolean {
    return this.process !== null && this.socket !== null && !this.socket.destroyed;
  }

  private resolveBinaryPath(): string {
    if (this.config.binaryPath) {
      return this.config.binaryPath;
    }

    if (process.env.SOROBAN_DEBUG_BIN) {
      return process.env.SOROBAN_DEBUG_BIN;
    }

    const repoRoot = path.resolve(__dirname, '..', '..', '..', '..');
    const candidates = [
      path.join(repoRoot, 'target', 'debug', process.platform === 'win32' ? 'soroban-debug.exe' : 'soroban-debug'),
      process.platform === 'win32' ? 'soroban-debug.exe' : 'soroban-debug'
    ];

    return candidates.find(candidate => fs.existsSync(candidate)) || candidates[candidates.length - 1];
  }

  private async findAvailablePort(): Promise<number> {
    return await new Promise<number>((resolve, reject) => {
      const server = net.createServer();
      server.listen(0, '127.0.0.1', () => {
        const address = server.address();
        if (!address || typeof address === 'string') {
          reject(new Error('Failed to determine an available port'));
          return;
        }

        const port = address.port;
        server.close((error) => {
          if (error) {
            reject(error);
            return;
          }
          resolve(port);
        });
      });
      server.on('error', reject);
    });
  }

  private async waitForServer(port: number): Promise<void> {
    const deadline = Date.now() + 10000;

    while (Date.now() < deadline) {
      if (this.process && this.process.exitCode !== null) {
        throw new Error(`Debugger server exited with code ${this.process.exitCode}`);
      }

      if (await this.canConnect(port)) {
        return;
      }

      await new Promise(resolve => setTimeout(resolve, 100));
    }

    throw new Error(`Timed out waiting for debugger server on port ${port}`);
  }

  private async canConnect(port: number): Promise<boolean> {
    return await new Promise<boolean>((resolve) => {
      const socket = net.createConnection({ host: '127.0.0.1', port }, () => {
        socket.destroy();
        resolve(true);
      });

      socket.on('error', () => {
        socket.destroy();
        resolve(false);
      });
    });
  }

  private async connect(port: number): Promise<void> {
    await new Promise<void>((resolve, reject) => {
      const socket = net.createConnection({ host: '127.0.0.1', port }, () => {
        this.socket = socket;
        resolve();
      });

      socket.setEncoding('utf8');
      socket.on('data', (chunk: string) => {
        this.buffer += chunk;
        this.consumeMessages();
      });
      socket.on('error', reject);
      socket.on('close', () => {
        this.rejectPendingRequests(new Error('Debugger connection closed'));
        this.socket = null;
      });
    });
  }

  private consumeMessages(): void {
    while (true) {
      const newlineIndex = this.buffer.indexOf('\n');
      if (newlineIndex === -1) {
        return;
      }

      const line = this.buffer.slice(0, newlineIndex).trim();
      this.buffer = this.buffer.slice(newlineIndex + 1);

      if (!line) {
        continue;
      }

      const message = JSON.parse(line) as DebugMessage;
      const pending = this.pendingRequests.get(message.id);
      if (!pending || !message.response) {
        continue;
      }

      this.pendingRequests.delete(message.id);
      pending.resolve(message.response);
    }
  }

  private async sendRequest(request: DebugRequest): Promise<DebugResponse> {
    if (!this.socket) {
      throw new Error('Debugger connection is not established');
    }

    this.requestId += 1;
    const id = this.requestId;
    const message: DebugMessage = { id, request };

    const responsePromise = new Promise<DebugResponse>((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });
    });

    this.socket.write(`${JSON.stringify(message)}\n`);
    const response = await responsePromise;
    if (response.type === 'Error') {
      throw new Error(response.message);
    }
    return response;
  }

  private async captureSessionInfo(): Promise<void> {
    try {
      const response = await this.sendRequest({ type: 'Ping' });
      this.expectResponse(response, 'Pong');
      this.sessionInfo = extractSessionInfoFromPong(response);
    } catch {
      this.sessionInfo = {
        backendVersion: 'unknown',
        protocolVersion: 'unknown'
      };
    }
  }

  private rejectPendingRequests(error: Error): void {
    for (const pending of this.pendingRequests.values()) {
      pending.reject(error);
    }
    this.pendingRequests.clear();
  }

  private expectResponse<T extends DebugResponse['type']>(
    response: DebugResponse,
    type: T
  ): asserts response is Extract<DebugResponse, { type: T }> {
    if (response.type !== type) {
      throw new Error(`Unexpected debugger response: expected ${type}, got ${response.type}`);
    }
  }
}

export function extractSessionInfoFromPong(response: Extract<DebugResponse, { type: 'Pong' }>): DebuggerSessionInfo {
  return {
    backendVersion: response.backend_version || 'unknown',
    protocolVersion: response.protocol_version || 'unknown'
  };
}
