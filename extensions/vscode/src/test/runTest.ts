import * as assert from "assert";
import { spawn } from "child_process";
import * as fs from "fs";
import * as net from "net";
import * as os from "os";
import * as path from "path";
import {
  DebuggerProcess,
  DebuggerTimeoutError,
  formatProtocolMismatchMessage,
  validateLaunchConfig,
} from "../cli/debuggerProcess";
import { resolveSourceBreakpoints } from "../dap/sourceBreakpoints";
import { VariableStore } from "../dap/variableStore";
import {
  collectSorobanLaunchConfigs,
  formatPreflightFailureMessage,
  runLaunchPreflightCommand,
} from "../preflightCommand";
import { DapClient } from "./dapClient";
import { isLoopbackAvailable } from "./networkHelper";
import { runDapE2ESuite, runSmokeSuite } from "./suites";

type DebugMessage = {
  id: number;
  request?: { type: string; [key: string]: unknown };
  response?: { type: string; [key: string]: unknown };
};

const BREAKPOINT_SYNC_TEST_LOG_ENV = "SOROBAN_DEBUG_BREAKPOINT_SYNC_TEST_LOG";

async function startMockDebuggerServer(options: {
  evaluateDelayMs: number;
}): Promise<{ port: number; close: () => Promise<void> }> {
  const server = net.createServer();
  const sockets = new Set<net.Socket>();

  server.on("connection", (socket) => {
    sockets.add(socket);
    socket.setEncoding("utf8");

    let buffer = "";
    socket.on("data", (chunk: string) => {
      buffer += chunk;
      while (true) {
        const newlineIndex = buffer.indexOf("\n");
        if (newlineIndex === -1) {
          return;
        }

        const line = buffer.slice(0, newlineIndex).trim();
        buffer = buffer.slice(newlineIndex + 1);
        if (!line) {
          continue;
        }

        const message = JSON.parse(line) as DebugMessage;
        if (!message.request) {
          continue;
        }

        const respond = (response: DebugMessage["response"], delayMs = 0) => {
          setTimeout(() => {
            if (socket.destroyed) {
              return;
            }
            socket.write(`${JSON.stringify({ id: message.id, response })}\n`);
          }, delayMs);
        };

        switch (message.request.type) {
          case "Handshake":
            respond({
              type: "HandshakeAck",
              server_name: "mock-soroban-debug",
              server_version: "0.1.0",
              protocol_min: 1,
              protocol_max: 1,
              selected_version: 1,
            });
            break;
          case "Authenticate":
            respond({ type: "Authenticated", success: true, message: "ok" });
            break;
          case "LoadSnapshot":
            respond({ type: "SnapshotLoaded", summary: "ok" });
            break;
          case "LoadContract":
            respond({ type: "ContractLoaded", size: 0 });
            break;
          case "Ping":
            respond({ type: "Pong" });
            break;
          case "Evaluate":
            respond(
              {
                type: "EvaluateResult",
                result: "ok",
                result_type: "string",
                variables_reference: 0,
              },
              options.evaluateDelayMs,
            );
            break;
          case "Inspect":
            respond(
              {
                type: "InspectionResult",
                function: "main",
                args: "[]",
                step_count: 0,
                paused: true,
                call_stack: ["main"],
              },
              options.evaluateDelayMs,
            );
            break;
          case "GetStorage":
            respond(
              { type: "StorageState", storage_json: "{}" },
              options.evaluateDelayMs,
            );
            break;
          case "Disconnect":
            respond({ type: "Disconnected" });
            break;
          default:
            respond({
              type: "Error",
              message: `Unhandled request type: ${message.request.type}`,
            });
            break;
        }
      }
    });

    socket.on("close", () => sockets.delete(socket));
    socket.on("error", () => sockets.delete(socket));
  });

  const port = await new Promise<number>((resolve, reject) => {
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        reject(new Error("Failed to allocate mock server port"));
        return;
      }
      resolve(address.port);
    });
    server.on("error", reject);
  });

  return {
    port,
    close: async () => {
      for (const socket of sockets) {
        socket.destroy();
      }
      await new Promise<void>((resolve) => server.close(() => resolve()));
    },
  };
}

async function wait(ms: number): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, ms));
}

async function waitForProcessLog(
  getOutput: () => string,
  pattern: RegExp,
  timeoutMs = 5_000,
): Promise<string> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const output = getOutput();
    if (pattern.test(output)) {
      return output;
    }
    await wait(25);
  }

  throw new Error(`Timed out waiting for process log matching ${pattern}`);
}

async function main(): Promise<void> {
  if (!(await isLoopbackAvailable())) {
    console.warn(
      "⚠️ Skipping VS Code E2E tests: Network loopback is restricted in this environment.",
    );
    process.exit(0); // Exit successfully, treating it as a graceful skip
  }

  await runSmokeSuite();
  console.log(
    "Running DAP E2E suite (includes first-continue stop-reason regression assertion)",
  );
  await runDapE2ESuite();

  const compatibilityMessage = formatProtocolMismatchMessage({
    extensionVersion: "0.1.0",
    backendName: "soroban-debug",
    backendVersion: "0.0.0",
    backendProtocolMin: 0,
    backendProtocolMax: 0,
    extra:
      "Protocol mismatch: client supports [1..=1], server supports [0..=0]",
  });
  assert.match(
    compatibilityMessage,
    /Extension version:/,
    "Expected protocol mismatch message to mention extension version",
  );
  assert.match(
    compatibilityMessage,
    /supports protocol/,
    "Expected protocol mismatch message to mention backend protocol range",
  );
  assert.match(
    compatibilityMessage,
    /Remediation:/,
    "Expected protocol mismatch message to include remediation guidance",
  );

  await assertPerRequestTimeoutBehavior();

  const extensionRoot = process.cwd();
  const repoRoot = path.resolve(extensionRoot, "..", "..");

  {
    const store = new VariableStore({
      pageSize: 3,
      maxStringPreview: 6,
      maxHexPreviewBytes: 2,
    });

    const bigArray = [1, 2, 3, 4, 5, 6, 7];
    const arrayVar = store.toVariable("arr", bigArray);
    assert.ok(
      arrayVar.variablesReference && arrayVar.variablesReference > 0,
      "Expected array to be expandable",
    );
    assert.equal(arrayVar.indexedVariables, bigArray.length);

    const firstPage = store.getVariables(arrayVar.variablesReference as number);
    assert.deepEqual(
      firstPage.slice(0, 3).map((v) => v.name),
      ["[0]", "[1]", "[2]"],
    );
    assert.equal(firstPage[0].value, "1");

    const pager = firstPage[3];
    assert.match(pager.name, /show more/i);
    assert.ok(
      pager.variablesReference && pager.variablesReference > 0,
      "Expected pager to be expandable",
    );

    const secondPage = store.getVariables(pager.variablesReference as number);
    assert.deepEqual(
      secondPage.slice(0, 3).map((v) => v.name),
      ["[3]", "[4]", "[5]"],
    );
    assert.equal(secondPage[2].value, "6");

    const thirdPager = secondPage[3];
    const thirdPage = store.getVariables(
      thirdPager.variablesReference as number,
    );
    assert.deepEqual(
      thirdPage.map((v) => v.name),
      ["[6]"],
    );

    const longString = store.toVariable("s", "1234567890");
    assert.ok(
      longString.variablesReference && longString.variablesReference > 0,
      "Expected long string to be expandable",
    );
    assert.match(longString.value, /truncated/i);
    const fullString = store.getVariables(
      longString.variablesReference as number,
    );
    assert.equal(fullString[0].name, "(full)");
    assert.equal(fullString[0].value, "1234567890");

    const bytesVar = store.toVariable("b", {
      type: "bytes",
      value: "0x01020304",
    });
    assert.ok(
      bytesVar.variablesReference && bytesVar.variablesReference > 0,
      "Expected bytes to be expandable",
    );
    assert.match(bytesVar.value, /bytes\(\d+\)/);
    const bytesDetails = store.getVariables(
      bytesVar.variablesReference as number,
    );
    assert.ok(
      bytesDetails.some((v) => v.name === "hex"),
      "Expected bytes details to include hex",
    );
    assert.ok(
      bytesDetails.some((v) => v.name === "base64"),
      "Expected bytes details to include base64",
    );

    const addr = "G" + "A".repeat(55);
    const addrVar = store.toVariable("a", addr);
    assert.equal(addrVar.type, "address");

    console.log("Variable rendering unit tests passed");
  }

  {
    // Breakpoint re-anchoring across source edits
    const tmpFile = path.join(os.tmpdir(), `bp_reanchor_${Date.now()}.rs`);
    const originalSource = [
      "pub fn helper() {",
      "  let x = 1;",
      "}",
      "",
      "pub fn foo(env: Env) -> u32 {",
      "  let y = 2;",
      "  y",
      "}",
      "",
    ].join("\n");
    fs.writeFileSync(tmpFile, originalSource);

    // foo spans lines 5-8; line 7 is inside foo
    const exportedFns = new Set(["foo"]);
    const initial = resolveSourceBreakpoints(tmpFile, [7], exportedFns);
    assert.equal(
      initial[0].functionName,
      "foo",
      "Expected line 7 to map to foo",
    );

    // Build history: line 7 was in foo
    const history = new Map([[7, "foo"]]);

    // Edit: insert 5 comment lines between helper and foo
    const editedSource = [
      "pub fn helper() {",
      "  let x = 1;",
      "}",
      "",
      "// added line 1",
      "// added line 2",
      "// added line 3",
      "// added line 4",
      "// added line 5",
      "",
      "pub fn foo(env: Env) -> u32 {",
      "  let y = 2;",
      "  y",
      "}",
      "",
    ].join("\n");
    fs.writeFileSync(tmpFile, editedSource);

    // Without history: line 7 is now a comment, not inside any function
    const withoutHistory = resolveSourceBreakpoints(tmpFile, [7], exportedFns);
    assert.equal(
      withoutHistory[0].reasonCode,
      "HEURISTIC_NO_FUNCTION",
      "Expected line 7 to not map to any function after edit without history",
    );

    // With history: line 7 re-anchors to foo's new start (line 11)
    const withHistory = resolveSourceBreakpoints(
      tmpFile,
      [7],
      exportedFns,
      history,
    );
    assert.equal(
      withHistory[0].functionName,
      "foo",
      "Expected re-anchored breakpoint to map to foo",
    );
    assert.equal(
      withHistory[0].reasonCode,
      "HEURISTIC_REANCHORED",
      "Expected HEURISTIC_REANCHORED reason code",
    );
    assert.equal(
      withHistory[0].line,
      11,
      "Expected re-anchored line to be new foo start",
    );
    assert.equal(
      withHistory[0].requestedLine,
      7,
      "Expected requestedLine to remain the original line",
    );
    assert.equal(
      withHistory[0].setBreakpoint,
      true,
      "Expected re-anchored breakpoint to set runtime breakpoint",
    );

    fs.unlinkSync(tmpFile);
    console.log("Breakpoint re-anchoring tests passed");
  }

  {
    const mockServer = await startMockDebuggerServer({ evaluateDelayMs: 150 });
    const debuggerProcess = new DebuggerProcess({
      contractPath: "mock.wasm",
      port: mockServer.port,
      spawnServer: false,
    });

    await debuggerProcess.start();

    // Cancel-before-response: abort removes pending entry and ignores late responses.
    const controller = new AbortController();
    const evaluatePromise = debuggerProcess.evaluate("1", undefined, {
      signal: controller.signal,
    });
    setTimeout(() => controller.abort(), 10);
    await assert.rejects(
      evaluatePromise,
      (error: any) => error?.name === "AbortError",
    );

    await wait(250);
    assert.equal(
      ((debuggerProcess as any).pendingRequests as Map<number, unknown>).size,
      0,
    );
    await debuggerProcess.ping();

    // Cancel-after-timeout: timeout removes pending entry and ignores late responses.
    const timedOut = debuggerProcess.evaluate("2", undefined, {
      timeoutMs: 20,
    });
    await assert.rejects(
      timedOut,
      (error: any) => error?.name === "TimeoutError",
    );

    await wait(250);
    assert.equal(
      ((debuggerProcess as any).pendingRequests as Map<number, unknown>).size,
      0,
    );
    await debuggerProcess.ping();

    await debuggerProcess.stop();
    await mockServer.close();
    console.log("Cancellation tests passed");
  }

  const emittedFiles = [
    path.join(extensionRoot, "dist", "extension.js"),
    path.join(extensionRoot, "dist", "debugAdapter.js"),
    path.join(extensionRoot, "dist", "cli", "debuggerProcess.js"),
  ];

  for (const file of emittedFiles) {
    assert.ok(fs.existsSync(file), `Missing compiled artifact: ${file}`);
  }

  const preflightBinaryPath = emittedFiles[0];
  const contractPath = path.join(
    repoRoot,
    "tests",
    "fixtures",
    "wasm",
    "echo.wasm",
  );
  assert.ok(
    fs.existsSync(contractPath),
    `Missing fixture WASM: ${contractPath}`,
  );
  const snapshotPath = path.join(
    repoRoot,
    "extensions",
    "vscode",
    "package.json",
  );

  const goodPreflight = await validateLaunchConfig({
    binaryPath: preflightBinaryPath,
    contractPath,
    snapshotPath,
    entrypoint: "echo",
    args: ["7"],
    token: "debug-token",
  });
  assert.equal(
    goodPreflight.ok,
    true,
    "Expected valid launch configuration to pass preflight",
  );

  const missingContract = await validateLaunchConfig({
    binaryPath: preflightBinaryPath,
    contractPath: path.join(repoRoot, "missing-contract.wasm"),
    entrypoint: "echo",
    args: [],
  });
  assert.equal(
    missingContract.ok,
    false,
    "Expected missing contract path to fail preflight",
  );
  assert.equal(missingContract.issues[0].field, "contractPath");
  assert.match(missingContract.issues[0].message, /contractPath/);

  const badArgs = await validateLaunchConfig({
    binaryPath: preflightBinaryPath,
    contractPath,
    entrypoint: "echo",
    args: [{ nested: undefined }],
  });
  assert.equal(
    badArgs.ok,
    false,
    "Expected non-serializable args to fail preflight",
  );
  assert.equal(badArgs.issues[0].field, "args");
  assert.match(badArgs.issues[0].message, /\$\[0\]\.nested/);

  const badPort = await validateLaunchConfig({
    binaryPath: preflightBinaryPath,
    contractPath,
    entrypoint: "echo",
    args: [],
    port: 70000,
  });
  assert.equal(
    badPort.ok,
    false,
    "Expected out-of-range port to fail preflight",
  );
  assert.equal(badPort.issues[0].field, "port");

  const badToken = await validateLaunchConfig({
    binaryPath: preflightBinaryPath,
    contractPath,
    entrypoint: "echo",
    args: [],
    token: "   ",
  });
  assert.equal(badToken.ok, false, "Expected blank token to fail preflight");
  assert.equal(badToken.issues[0].field, "token");

  {
    const candidates = collectSorobanLaunchConfigs([
      {
        folder: { name: "workspace-a" },
        configurations: [
          {
            name: "Soroban: First",
            type: "soroban",
            request: "launch",
            contractPath: contractPath,
          },
          { name: "Ignored", type: "node", request: "launch" },
        ],
      },
      {
        folder: { name: "workspace-b" },
        configurations: [
          {
            name: "Soroban: Second",
            type: "soroban",
            request: "launch",
            contractPath: "contract-b.wasm",
          },
        ],
      },
    ]);

    assert.equal(
      candidates.length,
      2,
      "Expected only Soroban launch configs to be collected",
    );
    assert.equal(candidates[0].description, "workspace-a");
    assert.equal(candidates[1].description, "workspace-b");
  }

  {
    const applyQuickFixCalls: string[] = [];
    let infoMessage = "";

    const outcome = await runLaunchPreflightCommand({
      launchConfigSources: [
        {
          folder: { name: "workspace-a" },
          configurations: [
            {
              name: "Soroban: Happy Path",
              type: "soroban",
              request: "launch",
              contractPath,
            },
          ],
        },
      ],
      selectLaunchConfig: async () => {
        throw new Error(
          "Did not expect configuration picker for a single config",
        );
      },
      validateLaunchConfig: async () => ({
        ok: true,
        issues: [],
        resolvedBinaryPath: preflightBinaryPath,
      }),
      showInformationMessage: async (message: string) => {
        infoMessage = message;
        return undefined;
      },
      showWarningMessage: async () => undefined,
      showErrorMessage: async () => undefined,
      applyQuickFix: async (quickFix) => {
        applyQuickFixCalls.push(quickFix);
      },
    });

    assert.equal(outcome, "passed");
    assert.match(infoMessage, /backend was not started/i);
    assert.deepEqual(applyQuickFixCalls, []);
  }

  {
    const applyQuickFixCalls: string[] = [];
    let errorMessage = "";

    const outcome = await runLaunchPreflightCommand({
      launchConfigSources: [
        {
          folder: { name: "workspace-a" },
          configurations: [
            {
              name: "Soroban: Broken",
              type: "soroban",
              request: "launch",
              contractPath,
            },
          ],
        },
      ],
      selectLaunchConfig: async () => {
        throw new Error(
          "Did not expect configuration picker for a single config",
        );
      },
      validateLaunchConfig: async () => ({
        ok: false,
        issues: [
          {
            field: "contractPath",
            message:
              "Launch config field 'contractPath' points to a missing file.",
            expected: "A readable .wasm file.",
            quickFixes: ["pickContract", "openLaunchConfig"],
          },
          {
            field: "args",
            message: "Launch config field 'args' must be an array.",
            expected: "A JSON array.",
            quickFixes: ["openLaunchConfig"],
          },
        ],
        resolvedBinaryPath: preflightBinaryPath,
      }),
      showInformationMessage: async () => undefined,
      showWarningMessage: async () => undefined,
      showErrorMessage: async (message: string, ...actions: string[]) => {
        errorMessage = message;
        assert.deepEqual(actions, ["Select Contract", "Open launch.json"]);
        return "Select Contract";
      },
      applyQuickFix: async (quickFix) => {
        applyQuickFixCalls.push(quickFix);
      },
    });

    assert.equal(outcome, "failed");
    assert.match(errorMessage, /found 2 issues/i);
    assert.deepEqual(applyQuickFixCalls, ["pickContract"]);
    assert.match(
      formatPreflightFailureMessage(
        { label: "Soroban: Broken", config: { contractPath } },
        [
          {
            field: "contractPath",
            message: "missing",
            expected: "expected",
            quickFixes: ["pickContract"],
          },
        ],
      ),
      /Soroban: Broken/,
    );
  }

  {
    const applyQuickFixCalls: string[] = [];
    let warningMessage = "";

    const outcome = await runLaunchPreflightCommand({
      launchConfigSources: [
        {
          folder: { name: "workspace-a" },
          configurations: [],
        },
      ],
      selectLaunchConfig: async () => undefined,
      validateLaunchConfig: async () => {
        throw new Error("Did not expect validation with no launch config");
      },
      showInformationMessage: async () => undefined,
      showWarningMessage: async (message: string, ...actions: string[]) => {
        warningMessage = message;
        assert.deepEqual(actions, [
          "Generate Launch Config",
          "Open launch.json",
        ]);
        return "Generate Launch Config";
      },
      showErrorMessage: async () => undefined,
      applyQuickFix: async (quickFix) => {
        applyQuickFixCalls.push(quickFix);
      },
    });

    assert.equal(outcome, "no-config");
    assert.match(warningMessage, /No Soroban launch configurations were found/);
    assert.deepEqual(applyQuickFixCalls, ["generateLaunchConfig"]);
  }

  const binaryPath =
    process.env.SOROBAN_DEBUG_BIN ||
    path.join(
      repoRoot,
      "target",
      "debug",
      process.platform === "win32" ? "soroban-debug.exe" : "soroban-debug",
    );

  if (!fs.existsSync(binaryPath)) {
    console.log(
      `Skipping debugger smoke test because the CLI binary was not found at ${binaryPath}`,
    );
    return;
  }

  const debuggerProcess = new DebuggerProcess({
    binaryPath,
    contractPath,
    entrypoint: "echo",
    args: ["7"],
  });

  await debuggerProcess.start();
  await debuggerProcess.ping();

  const sourcePath = path.join(
    repoRoot,
    "tests",
    "fixtures",
    "contracts",
    "echo",
    "src",
    "lib.rs",
  );
  assert.ok(fs.existsSync(sourcePath), `Missing fixture source: ${sourcePath}`);
  const exportedFunctions = await debuggerProcess.getContractFunctions();
  const resolvedBreakpoints = resolveSourceBreakpoints(
    sourcePath,
    [10],
    exportedFunctions,
  );
  assert.equal(
    resolvedBreakpoints[0].verified,
    false,
    "Expected heuristic source mapping to be unverified",
  );
  assert.equal(resolvedBreakpoints[0].functionName, "echo");
  assert.equal(
    resolvedBreakpoints[0].setBreakpoint,
    true,
    "Expected heuristic mapping to still set a function breakpoint",
  );

  // Test HEURISTIC_NO_FUNCTION behavior for lines outside any function
  const noFunctionBreakpoints = resolveSourceBreakpoints(
    sourcePath,
    [1, 2, 13], // Lines outside any function in lib.rs
    exportedFunctions,
  );

  for (let i = 0; i < noFunctionBreakpoints.length; i++) {
    const bp = noFunctionBreakpoints[i];
    assert.equal(
      bp.verified,
      false,
      `Expected line ${bp.requestedLine} to be unverified`,
    );
    assert.equal(
      bp.reasonCode,
      "HEURISTIC_NO_FUNCTION",
      `Expected line ${bp.requestedLine} to have HEURISTIC_NO_FUNCTION reason code`,
    );
    assert.equal(
      bp.setBreakpoint,
      false,
      `Expected line ${bp.requestedLine} to not set runtime breakpoint`,
    );
    assert.equal(
      bp.message,
      "Line is not inside a detectable Rust function",
      `Expected line ${bp.requestedLine} to have clear diagnostic message`,
    );
  }

  await debuggerProcess.setBreakpoint({
    id: "echo",
    functionName: "echo",
  });
  const paused = await debuggerProcess.execute();
  assert.equal(
    paused.paused,
    true,
    "Expected breakpoint to pause before execution",
  );

  const pausedInspection = await debuggerProcess.inspect();
  assert.match(
    pausedInspection.args || "",
    /7/,
    "Expected paused inspection to include call args",
  );

  const resumed = await debuggerProcess.continueExecution();
  assert.match(
    resumed.output || "",
    /7/,
    "Expected continue() to finish echo()",
  );
  await debuggerProcess.clearBreakpoint("echo");

  const result = await debuggerProcess.execute();
  assert.match(
    result.output,
    /7/,
    "Expected second echo() to return the input",
  );

  const inspection = await debuggerProcess.inspect();
  assert.ok(
    Array.isArray(inspection.callStack),
    "Expected call stack array from inspection",
  );
  assert.match(
    inspection.args || "",
    /7/,
    "Expected inspection to include args",
  );

  const storage = await debuggerProcess.getStorage();
  assert.ok(
    typeof storage === "object" && storage !== null,
    "Expected storage snapshot object",
  );

  await debuggerProcess.stop();
  console.log("VS Code extension smoke tests passed");

  // DAP end-to-end tests (adapter <-> backend).
  const debugAdapterPath = path.join(extensionRoot, "dist", "debugAdapter.js");
  assert.ok(
    fs.existsSync(debugAdapterPath),
    `Missing debug adapter entrypoint: ${debugAdapterPath}`,
  );

  await runDapHappyPathE2E(debugAdapterPath, {
    contractPath,
    sourcePath,
    binaryPath,
  });
  await runDapLaunchErrorE2E(debugAdapterPath, {
    contractPath: path.join(
      repoRoot,
      "tests",
      "fixtures",
      "wasm",
      "does-not-exist.wasm",
    ),
    sourcePath,
    binaryPath,
  });

  console.log("VS Code DAP end-to-end tests passed");
}

async function assertPerRequestTimeoutBehavior(): Promise<void> {
  const dp = new DebuggerProcess({
    contractPath: "placeholder.wasm",
    entrypoint: "main",
    args: [],
    requestTimeoutMs: 5,
  });

  (dp as any).socket = { write: () => undefined, destroyed: false };

  const sendRequest = (dp as any).sendRequest.bind(dp) as (
    req: any,
    opts?: any,
  ) => Promise<any>;

  for (const req of [
    {
      type: "Handshake",
      client_name: "test",
      client_version: "0.0.0",
      protocol_min: 1,
      protocol_max: 1,
    },
    { type: "Inspect" },
    { type: "GetStorage" },
    { type: "Continue" },
  ]) {
    let threwTimeout = false;
    try {
      await sendRequest(req, { timeoutMs: 5 });
    } catch (error) {
      threwTimeout = error instanceof DebuggerTimeoutError;
    }

    assert.equal(
      threwTimeout,
      true,
      `Expected ${req.type} to time out deterministically`,
    );
    assert.equal(
      (dp as any).pendingRequests.size,
      0,
      "Expected pending request map to be cleared after timeout",
    );
  }
}

async function runDapHappyPathE2E(
  debugAdapterPath: string,
  fixtures: { contractPath: string; sourcePath: string; binaryPath: string },
): Promise<void> {
  const proc = spawn(process.execPath, [debugAdapterPath], {
    env: { ...process.env, [BREAKPOINT_SYNC_TEST_LOG_ENV]: "1" },
    stdio: ["pipe", "pipe", "pipe"],
  });
  const client = new DapClient(proc);
  let stderrOutput = "";
  proc.stderr.setEncoding("utf8");
  proc.stderr.on("data", (chunk: string) => {
    stderrOutput += chunk;
  });

  try {
    const init = await client.request("initialize", {
      adapterID: "soroban",
      linesStartAt1: true,
      columnsStartAt1: true,
      pathFormat: "path",
    });
    assert.equal(
      init.success,
      true,
      `initialize failed: ${init.message || ""}`,
    );
    await client.waitForEvent("initialized");

    const launch = await client.request(
      "launch",
      {
        type: "soroban",
        request: "launch",
        name: "Soroban: E2E",
        contractPath: fixtures.contractPath,
        entrypoint: "echo",
        args: ["7"],
        trace: false,
        binaryPath: fixtures.binaryPath,
      },
      30_000,
    );
    assert.equal(
      launch.success,
      true,
      `launch failed: ${launch.message || ""}`,
    );

    const setBps = await client.request("setBreakpoints", {
      source: { path: fixtures.sourcePath },
      breakpoints: [{ line: 10 }],
    });
    assert.equal(
      setBps.success,
      true,
      `setBreakpoints failed: ${setBps.message || ""}`,
    );
    assert.equal(
      setBps.body?.breakpoints?.[0]?.verified,
      false,
      "Expected heuristic source mapping to be unverified",
    );
    const breakpointSyncLog = await waitForProcessLog(
      () => stderrOutput,
      /BREAKPOINT_SYNC_TEST .*"action":"set".*"functionName":"echo".*"success":true/,
    );
    assert.match(
      breakpointSyncLog,
      /BREAKPOINT_SYNC_TEST/,
      "Expected adapter test log to confirm runtime breakpoint installation",
    );

    const configDone = await client.request("configurationDone", {});
    assert.equal(
      configDone.success,
      true,
      `configurationDone failed: ${configDone.message || ""}`,
    );

    await client.waitForEvent(
      "stopped",
      (e: any) => e.body?.reason === "entry",
    );

    const cont = await client.request("continue", { threadId: 1 }, 30_000);
    assert.equal(cont.success, true, `continue failed: ${cont.message || ""}`);

    await client.waitForEvent(
      "stopped",
      (e: any) => e.body?.reason === "breakpoint",
      30_000,
    );

    const threads = await client.request("threads", {});
    assert.equal(threads.success, true);
    assert.equal(
      Array.isArray(threads.body?.threads),
      true,
      "Expected threads array",
    );

    const stack = await client.request("stackTrace", { threadId: 1 });
    assert.equal(stack.success, true);
    const frameId = stack.body?.stackFrames?.[0]?.id;
    assert.ok(frameId, "Expected at least one stack frame");

    const scopes = await client.request("scopes", { frameId });
    assert.equal(scopes.success, true);
    const argsScope = (scopes.body?.scopes || []).find(
      (s: any) => s.name === "Arguments",
    );
    assert.ok(argsScope?.variablesReference, "Expected Arguments scope");

    const argsVars = await client.request("variables", {
      variablesReference: argsScope.variablesReference,
    });
    assert.equal(argsVars.success, true);
    assert.match(
      JSON.stringify(argsVars.body?.variables || []),
      /7/,
      "Expected argument variable to include the input",
    );

    const evalArgs = await client.request("evaluate", {
      expression: "args",
      frameId,
    });
    assert.equal(evalArgs.success, true);
    assert.match(
      String(evalArgs.body?.result || ""),
      /7/,
      "Expected evaluate(args) to include the input",
    );

    const evalStorage = await client.request("evaluate", {
      expression: "storage",
      frameId,
    });
    assert.equal(evalStorage.success, true);
    assert.match(
      String(evalStorage.body?.result || ""),
      /^\{/,
      "Expected evaluate(storage) to return JSON",
    );

    // Exercise stepping commands (these may exit quickly depending on the contract).
    const stepIn = await client.request("stepIn", { threadId: 1 }, 30_000);
    assert.equal(stepIn.success, true);
    const afterStepIn = await client.waitForAnyEvent(
      ["stopped", "exited"],
      () => true,
      30_000,
    );
    let executionExited = afterStepIn.event === "exited";

    if (!executionExited) {
      const next = await client.request("next", { threadId: 1 }, 30_000);
      assert.equal(next.success, true);
      const afterNext = await client.waitForAnyEvent(
        ["stopped", "exited"],
        () => true,
        30_000,
      );

      executionExited = afterNext.event === "exited";

      if (!executionExited) {
        const stepOut = await client.request(
          "stepOut",
          { threadId: 1 },
          30_000,
        );
        assert.equal(stepOut.success, true);
        const afterStepOut = await client.waitForAnyEvent(
          ["stopped", "exited"],
          () => true,
          30_000,
        );
        executionExited = afterStepOut.event === "exited";
      }
    }

    if (!executionExited) {
      const cont2 = await client.request("continue", { threadId: 1 }, 30_000);
      assert.equal(cont2.success, true);
      await client.waitForEvent("exited", () => true, 30_000);
    }

    const disconnect = await client.request("disconnect", { restart: false });
    assert.equal(disconnect.success, true);
  } finally {
    client.dispose();
  }
}

async function runDapLaunchErrorE2E(
  debugAdapterPath: string,
  fixtures: { contractPath: string; sourcePath: string; binaryPath: string },
): Promise<void> {
  const proc = spawn(process.execPath, [debugAdapterPath], {
    stdio: ["pipe", "pipe", "pipe"],
  });
  const client = new DapClient(proc);

  try {
    const init = await client.request("initialize", {
      adapterID: "soroban",
      linesStartAt1: true,
      columnsStartAt1: true,
      pathFormat: "path",
    });
    assert.equal(init.success, true);
    await client.waitForEvent("initialized");

    const launch = await client.request(
      "launch",
      {
        type: "soroban",
        request: "launch",
        name: "Soroban: E2E (error)",
        contractPath: fixtures.contractPath,
        entrypoint: "echo",
        args: ["7"],
        trace: false,
        binaryPath: fixtures.binaryPath,
      },
      30_000,
    );
    assert.equal(
      launch.success,
      false,
      "Expected launch to fail for missing contract fixture",
    );

    const disconnect = await client.request("disconnect", { restart: false });
    assert.equal(disconnect.success, true);
  } finally {
    client.dispose();
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
