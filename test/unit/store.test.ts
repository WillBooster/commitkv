import { expect, test } from 'bun:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { Store } from '../../src/index.ts';

test('values written as strings and bytes survive reopening', () => {
  const directory = temporaryDirectory();
  const binary = new Uint8Array([0, 255, 128, 10]);
  const store = new Store(directory);
  store.put('text', 'こんにちは');
  store.put(binary, binary);
  store.put('text', 'replaced');

  const reopened = new Store(directory);
  expect(reopened.get('text')?.toString()).toBe('replaced');
  expect(new Uint8Array(reopened.get(binary) ?? [])).toEqual(binary);
  expect(reopened.get('missing')).toBeUndefined();
  expect(reopened.has('text')).toBe(true);
  expect(reopened.has('missing')).toBe(false);
  expect(reopened.size).toBe(2);
  expect(
    reopened
      .keys()
      .map((key) => key.toString('hex'))
      .toSorted()
  ).toEqual(['00ff800a', '74657874']);
});

test('a store sees what another store wrote after refresh', () => {
  const directory = temporaryDirectory();
  const reader = new Store(directory);
  new Store(directory).put('key', 'value');
  expect(reader.get('key')).toBeUndefined();
  reader.refresh();
  expect(reader.get('key')?.toString()).toBe('value');
});

test('a record that does not fit in a segment is rejected and no segment exceeds the limit', () => {
  const directory = temporaryDirectory();
  const maxSegmentBytes = 4096;
  const store = new Store(directory, { maxSegmentBytes });
  expect(() => store.put('huge', crypto.getRandomValues(new Uint8Array(maxSegmentBytes)))).toThrow(/segment/);
  for (let i = 0; i < 20; i++) store.put(`key-${i}`, crypto.getRandomValues(new Uint8Array(1000)));
  const sizes = fs
    .readdirSync(directory)
    .filter((name) => name.endsWith('.kvz'))
    .map((name) => fs.statSync(path.join(directory, name)).size);
  expect(sizes.length).toBeGreaterThan(1);
  expect(Math.max(...sizes)).toBeLessThanOrEqual(maxSegmentBytes);
  expect(() => new Store(directory, { maxSegmentBytes: 1 })).toThrow(/max_segment_bytes/);
});

function temporaryDirectory(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'kvzip-'));
}
