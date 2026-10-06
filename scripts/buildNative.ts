import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const root = path.join(import.meta.dirname, '..');
const libraryNames: Partial<Record<NodeJS.Platform, string>> = {
  darwin: 'libcommitkv_node.dylib',
  linux: 'libcommitkv_node.so',
};
const libraryName = libraryNames[process.platform];
if (!libraryName) throw new Error(`commitkv does not support ${process.platform}`);

const { error, status } = spawnSync(
  'cargo',
  ['build', '--release', '--locked', '--manifest-path', path.join(root, 'rust', 'Cargo.toml'), '-p', 'commitkv-node'],
  { stdio: 'inherit' }
);
if (error) throw error;
if (status !== 0) process.exit(status ?? 1);

fs.mkdirSync(path.join(root, 'native'), { recursive: true });
fs.copyFileSync(
  path.join(root, 'rust', 'target', 'release', libraryName),
  path.join(root, 'native', `commitkv-${process.platform}-${process.arch}.node`)
);
