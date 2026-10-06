import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

import manifest from '../package.json';

const root = path.join(import.meta.dirname, '..');
const directory = path.join(root, '.tmp', 'npm-package');
fs.rmSync(directory, { recursive: true, force: true });
fs.mkdirSync(directory, { recursive: true });
const packageManifest = {
  ...manifest,
  scripts: undefined,
  devDependencies: undefined,
  gitHead: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
};
fs.writeFileSync(path.join(directory, 'package.json'), `${JSON.stringify(packageManifest, undefined, 2)}\n`);
for (const name of ['dist', 'LICENSE', 'README.md']) {
  fs.cpSync(path.join(root, name), path.join(directory, name), { recursive: true });
}
fs.mkdirSync(path.join(directory, 'native'));
for (const platform of ['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64']) {
  const filename = `commitkv-${platform}.node`;
  fs.copyFileSync(path.join(root, 'native', filename), path.join(directory, 'native', filename));
}
execFileSync('npm', ['pack', directory, '--ignore-scripts', '--pack-destination', path.join(root, '.tmp')], {
  cwd: root,
  stdio: 'inherit',
});
