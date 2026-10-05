import { expect, test } from 'bun:test';
import { spawnSync } from 'node:child_process';
import path from 'node:path';

// Runs the Python binding's tests so that `wb test` covers every binding.
test(
  'the Python binding passes its pytest suite',
  () => {
    const { status, stdout, stderr } = spawnSync('uv', ['run', '--frozen', 'pytest', 'test/unit'], {
      cwd: path.join(import.meta.dirname, '..', '..'),
      encoding: 'utf8',
    });
    expect(`${stdout}${stderr}\nexit status: ${status}`).toEndWith('exit status: 0');
  },
  10 * 60 * 1000
);
