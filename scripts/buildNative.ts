import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const root = path.join(import.meta.dirname, '..');
const libraryNames: Partial<Record<NodeJS.Platform, string>> = {
  darwin: 'libkvzip_node.dylib',
  linux: 'libkvzip_node.so',
};
const libraryName = libraryNames[process.platform];
if (!libraryName) throw new Error(`kvzip does not support ${process.platform}`);

const { status } = spawnSync(
  'cargo',
  ['build', '--release', '--locked', '--manifest-path', path.join(root, 'rust', 'Cargo.toml'), '-p', 'kvzip-node'],
  { stdio: 'inherit' }
);
if (status !== 0) process.exit(status ?? 1);

fs.mkdirSync(path.join(root, 'native'), { recursive: true });
fs.copyFileSync(path.join(root, 'rust', 'target', 'release', libraryName), path.join(root, 'native', 'kvzip.node'));
