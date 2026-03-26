import * as assert from "assert";
import { ChildProcess, spawn } from "child_process";
import * as fs from "fs";
import * as net from "net";
import * as path from "path";
import {
  DebuggerProcess,
  getDebuggerVersionInfo,
  validateLaunchConfig,
  formatProtocolMismatchMessage,
  DebuggerTimeoutError,
  LaunchLifecycleEvent,
} from "../cli/debuggerProcess";
import { toLaunchProgressMessage } from "../launchLifecycle";
import { resolveSourceBreakpoints } from "../dap/sourceBreakpoints";
import { VariableStore } from "../dap/variableStore";
import { DapClient } from "./dapClient";

type DebugMessage = {
  id: number;
  request?: { type: string; [key: string]: unknown };
  response?: { type: string; [key: string]: unknown };
};

type TestFixtures = {
  extensionRoot: string;
  repoRoot: string;
  contractPath: string;
  sourcePath: string;
  binaryPath: string;
};

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
              server_name: "mock-debugger",
              server_version: "0.0.0",
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
          case "GetCapabilities":
            respond({
              type: "Capabilities",
              breakpoints: {
                conditional_breakpoints: false,
                hit_conditional_breakpoints: false,
                log_points: false,
              },
            });
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

function resolveFixtures(): TestFixtures {
  const extensionRoot = path.resolve(__dirname, "..", "..");
  const repoRoot = path.resolve(extensionRoot, "..", "..");
  const contractPath = path.join(
    repoRoot,
    "tests",
    "fixtures",
    "wasm",
    "echo.wasm",
  );
  const sourcePath = path.join(
    repoRoot,
    "tests",
    "fixtures",
    "contracts",
    "echo",
    "src",
    "lib.rs",
  );
  const binaryPath =
    process.env.SOROBAN_DEBUG_BIN ||
    path.join(
      repoRoot,
      "target",
      "debug",
      process.platform === "win32" ? "soroban-debug.exe" : "soroban-debug",
    );

  return {
    extensionRoot,
    repoRoot,
    contractPath,
    sourcePath,
    binaryPath,
  };
}

export async function runSmokeSuite(): Promise<void> {
  {
    const mock = "backend: 1.2.3 protocol: 4.5.6";
    const result = getDebuggerVersionInfo(mock);
    assert.strictEqual(result.backendVersion, "1.2.3");
    assert.strictEqual(result.protocolVersion, "4.5.6");
  }

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
  await assertLaunchLifecycleReporting();

  const fixtures = resolveFixtures();

  assert.equal(
    toLaunchProgressMessage({
      phase: "connect",
      status: "started",
      message: "Connecting to backend...",
    }),
    "Connect: Connecting to backend...",
  );
  assert.equal(
    toLaunchProgressMessage({
      phase: "ready",
      status: "completed",
      message: "Debugger is ready.",
    }),
    "Ready complete: Debugger is ready.",
  );
  assert.equal(
    toLaunchProgressMessage({
      phase: "load",
      status: "failed",
      message: "contract missing",
    }),
    "Load failed: contract missing",
  );

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
    const mockServer = await startMockDebuggerServer({ evaluateDelayMs: 150 });
    const debuggerProcess = new DebuggerProcess({
      contractPath: "mock.wasm",
      port: mockServer.port,
      spawnServer: false,
    });

    await debuggerProcess.start();

    const controller = new AbortController();
    const evaluatePromise = debuggerProcess.evaluate("1", undefined, {
      signal: controller.signal,
    });
    setTimeout(() => controller.abort(), 10);
    await assert.rejects(evaluatePromise, { name: "AbortError" });

    await wait(250);
    assert.equal(
      ((debuggerProcess as any).pendingRequests as Map<number, unknown>).size,
      0,
    );
    await debuggerProcess.ping();

    const timedOut = debuggerProcess.evaluate("2", undefined, {
      timeoutMs: 20,
    });
    await assert.rejects(timedOut, { name: "DebuggerTimeoutError" });

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
    path.join(fixtures.extensionRoot, "dist", "extension.js"),
    path.join(fixtures.extensionRoot, "dist", "debugAdapter.js"),
    path.join(fixtures.extensionRoot, "dist", "cli", "debuggerProcess.js"),
  ];

  for (const file of emittedFiles) {
    assert.ok(fs.existsSync(file), `Missing compiled artifact: ${file}`);
  }

  const preflightBinaryPath = emittedFiles[0];
  assert.ok(
    fs.existsSync(fixtures.contractPath),
    `Missing fixture WASM: ${fixtures.contractPath}`,
  );
  assert.ok(
    fs.existsSync(fixtures.sourcePath),
    `Missing fixture source: ${fixtures.sourcePath}`,
  );
  const snapshotPath = path.join(
    fixtures.repoRoot,
    "extensions",
    "vscode",
    "package.json",
  );

  const goodPreflight = await validateLaunchConfig({
    binaryPath: preflightBinaryPath,
    contractPath: fixtures.contractPath,
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
    contractPath: path.join(fixtures.repoRoot, "missing-contract.wasm"),
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
    contractPath: fixtures.contractPath,
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
    contractPath: fixtures.contractPath,
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
    contractPath: fixtures.contractPath,
    entrypoint: "echo",
    args: [],
    token: "   ",
  });
  assert.equal(badToken.ok, false, "Expected blank token to fail preflight");
  assert.equal(badToken.issues[0].field, "token");

  if (!fs.existsSync(fixtures.binaryPath)) {
    console.log(
      `Skipping debugger smoke test because the CLI binary was not found at ${fixtures.binaryPath}`,
    );
    return;
  }

  const debuggerProcess = new DebuggerProcess({
    binaryPath: fixtures.binaryPath,
    contractPath: fixtures.contractPath,
    entrypoint: "echo",
    args: ["7"],
  });

  await debuggerProcess.start();
  await debuggerProcess.ping();

  const exportedFunctions = await debuggerProcess.getContractFunctions();
  const resolvedBreakpoints = resolveSourceBreakpoints(
    fixtures.sourcePath,
    [10],
    exportedFunctions,
  );
  assert.equal(
    resolvedBreakpoints[0].verified,
    true,
    "Expected echo breakpoint to resolve",
  );
  assert.equal(resolvedBreakpoints[0].functionName, "echo");

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
}

export async function runDapE2ESuite(): Promise<void> {
  const fixtures = resolveFixtures();
  const debugAdapterPath = path.join(
    fixtures.extensionRoot,
    "dist",
    "debugAdapter.js",
  );
  assert.ok(
    fs.existsSync(debugAdapterPath),
    `Missing debug adapter entrypoint: ${debugAdapterPath}`,
  );
  assert.ok(
    fs.existsSync(fixtures.contractPath),
    `Missing fixture WASM: ${fixtures.contractPath}`,
  );
  assert.ok(
    fs.existsSync(fixtures.sourcePath),
    `Missing fixture source: ${fixtures.sourcePath}`,
  );

  if (!fs.existsSync(fixtures.binaryPath)) {
    console.log(
      `Skipping VS Code DAP end-to-end tests because the CLI binary was not found at ${fixtures.binaryPath}`,
    );
    return;
  }

  await runDapHappyPathE2E(debugAdapterPath, fixtures);
  await runDapLaunchErrorE2E(debugAdapterPath, {
    ...fixtures,
    contractPath: path.join(
      fixtures.repoRoot,
      "tests",
      "fixtures",
      "wasm",
      "does-not-exist.wasm",
    ),
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

async function assertLaunchLifecycleReporting(): Promise<void> {
  const mockServer = await startMockDebuggerServer({ evaluateDelayMs: 0 });
  const lifecycleEvents: LaunchLifecycleEvent[] = [];
  const debuggerProcess = new DebuggerProcess(
    {
      contractPath: "mock.wasm",
      snapshotPath: "snapshot.json",
      token: "debug-token",
      port: mockServer.port,
      spawnServer: false,
    },
    undefined,
    (event) => lifecycleEvents.push(event),
  );

  await debuggerProcess.start();
  await debuggerProcess.stop();
  await mockServer.close();

  assert.deepEqual(
    lifecycleEvents.map((event) => `${event.phase}:${event.status}`),
    [
      "connect:started",
      "connect:completed",
      "authenticate:started",
      "authenticate:completed",
      "load:started",
      "load:completed",
      "ready:completed",
    ],
    "Expected debugger launch lifecycle events for each major phase",
  );
}

async function runDapHappyPathE2E(
  debugAdapterPath: string,
  fixtures: Pick<TestFixtures, "contractPath" | "sourcePath" | "binaryPath">,
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
      true,
      "Expected breakpoint to verify",
    );

    const configDone = await client.request("configurationDone", {});
    assert.equal(
      configDone.success,
      true,
      `configurationDone failed: ${configDone.message || ""}`,
    );

    await client.waitForEvent("stopped", (e) => e.body?.reason === "entry");

    const cont = await client.request("continue", { threadId: 1 }, 30_000);
    assert.equal(cont.success, true, `continue failed: ${cont.message || ""}`);

    await client.waitForEvent(
      "stopped",
      (e) => e.body?.reason === "breakpoint",
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
  fixtures: Pick<TestFixtures, "contractPath" | "sourcePath" | "binaryPath">,
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
