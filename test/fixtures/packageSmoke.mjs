import assert from 'node:assert/strict';
import fs from 'node:fs';

import { Store } from 'kvzip';

const directory = fs.mkdtempSync('store-');
try {
  const store = new Store(directory);
  store.put('key', 'value');
  assert.equal(store.get('key')?.toString(), 'value');
  assert.equal(store.has('key'), true);
  assert.equal(store.size, 1);
  assert.deepEqual(store.keys().map((key) => key.toString()), ['key']);
  const reopened = new Store(directory);
  assert.equal(reopened.get('key')?.toString(), 'value');
  store.put('key', 'updated');
  reopened.refresh();
  assert.equal(reopened.get('key')?.toString(), 'updated');
  assert.equal(reopened.get('missing'), undefined);
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}
console.log(`Installed kvzip works on ${process.platform}-${process.arch} with ${process.versions.bun ? 'Bun' : 'Node.js'}.`);
