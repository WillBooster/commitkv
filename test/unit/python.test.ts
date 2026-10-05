import { expect, test } from 'bun:test';
import { spawnSync } from 'node:child_process';
import path from 'node:path';

const PYTEST_TIMEOUT_MS = 10 * 60 * 1000;

// Runs the Python binding's tests so that `wb test` covers every binding.
test(
  'the Python binding passes its pytest suite',
  () => {
    // The call blocks this thread, so only the child's own timeout can end a run that hangs.
    const { status, stdout, stderr } = spawnSync('uv', ['run', '--frozen', 'pytest', 'test/unit'], {
      cwd: path.join(import.meta.dirname, '..', '..'),
      encoding: 'utf8',
      timeout: PYTEST_TIMEOUT_MS,
    });
    expect(`${stdout}${stderr}\nexit status: ${status}`).toEndWith('exit status: 0');
  },
  PYTEST_TIMEOUT_MS + 60 * 1000
);
