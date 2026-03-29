import { runDapE2ESuite } from './suites';

type IterationResult =
  | { iteration: number; status: 'pass' }
  | { iteration: number; status: 'fail'; error: unknown };

function parseRepeatCount(): number {
  const argIndex = process.argv.indexOf('--repeat');
  if (argIndex !== -1 && process.argv[argIndex + 1]) {
    const n = parseInt(process.argv[argIndex + 1], 10);
    if (!isNaN(n) && n >= 1) {
      return n;
    }
  }
  const envVal = process.env.DAP_E2E_REPEAT;
  if (envVal) {
    const n = parseInt(envVal, 10);
    if (!isNaN(n) && n >= 1) {
      return n;
    }
  }
  return 1;
}

async function runRepeat(repeat: number): Promise<void> {
  console.log(`[repeat-mode] Running DAP e2e suite ${repeat} times to detect flakiness...`);

  const results: IterationResult[] = [];

  for (let i = 1; i <= repeat; i++) {
    try {
      await runDapE2ESuite();
      results.push({ iteration: i, status: 'pass' });
      console.log(`[repeat-mode] Iteration ${i}/${repeat}: PASS`);
    } catch (error) {
      results.push({ iteration: i, status: 'fail', error });
      console.log(`[repeat-mode] Iteration ${i}/${repeat}: FAIL`);
    }
  }

  const passed = results.filter((r) => r.status === 'pass').length;
  const failed = results.filter((r) => r.status === 'fail').length;
  const firstFailure = results.find((r): r is Extract<IterationResult, { status: 'fail' }> => r.status === 'fail');

  console.log('');
  console.log('=== DAP e2e repeat-mode summary ===');
  console.log(`  Iterations: ${repeat}`);
  console.log(`  Passed:     ${passed}`);
  console.log(`  Failed:     ${failed}`);

  if (firstFailure) {
    console.log(`  First failure: iteration ${firstFailure.iteration}`);
    console.log('  First failure context:');
    console.error(firstFailure.error);
    process.exit(1);
  }

  console.log('  Result: all iterations passed.');
}

async function main(): Promise<void> {
  const repeat = parseRepeatCount();

  if (repeat === 1) {
    await runDapE2ESuite();
    return;
  }

  await runRepeat(repeat);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
