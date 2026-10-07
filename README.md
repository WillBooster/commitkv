# commitkv

A compressed key-value store for caches that are committed to git, usable from Python,
Node.js, and Bun.

- **No file ever exceeds a size limit.** Records are appended to segment files; before a record
  is written, a segment that it would push past the limit (32 MiB by default) is closed and a
  new one is started. A record that cannot fit even in an empty segment is rejected.
- **Git-friendly.** Segments are only ever appended to, and only by the checkout that created
  them: a segment that arrived through git (a clone, another worktree, a checkout of another
  branch) is never changed. Branches that both add records therefore add different files and
  merge without conflicts.
- **Compressed without training.** Every value is coded with zstd against the values stored
  before it in the same segment, so values that resemble each other (prompts built from one
  template, API responses of one shape) cost little after the first few. There is no
  dictionary to train and nothing to configure.
- **Safe to share.** One store may be used from several threads, and several stores, in one
  process or many, may use the same directory at once.

commitkv supports Linux (glibc 2.17 or newer) and macOS, on x86_64 and arm64.

## Installation

For Node.js and Bun, install the npm package. It includes prebuilt Node-API addons for all
four supported platforms; Rust is not required.

```bash
npm install commitkv
# or
bun add commitkv
```

For Python 3.10 or newer, install the PyPI package. Wheels for the same four platforms do
not require Rust; installing from source requires a Rust toolchain.

```bash
pip install commitkv
```

## Usage

A `str` / `string` key or value stands for its UTF-8 encoding; values are returned as bytes.

```python
from commitkv import Store

store = Store("cache")                 # or Store("cache", max_segment_bytes=16 * 1024 * 1024)
store.put("key", "value")
store.get("key")                       # b"value"; None when the key is absent
"key" in store, len(store), store.keys()
store.refresh()                        # pick up what other stores wrote to the directory
```

```ts
import { Store } from 'commitkv';

const store = new Store('cache'); // or new Store('cache', { maxSegmentBytes: 16 * 1024 * 1024 })
store.put('key', 'value');
store.get('key')?.toString(); // 'value'; undefined when the key is absent
(store.has('key'), store.size, store.keys());
store.refresh(); // pick up what other stores wrote to the directory
```

## Behavior

- `put` replaces every value of the key that the store has seen. The earlier records stay in
  their segments: commitkv never deletes or compacts.
- A store reads the directory when it opens, when `refresh` is called, and when a `put` starts
  writing (the first `put`, and the first after a failed one); records that other stores write
  stay invisible until one of those happens. Call `refresh` after
  replacing segments under an open store, for example by checking out another branch.
- Stores that write to a directory at the same time each append to a segment of their own, and
  the next run continues at most one of them. Open one store per directory in a process and share
  it; a store per use leaves a small segment behind on every run, and a small segment
  compresses worse because its values have few earlier ones to refer to.
- `keys` lists the keys in the order their current values lie in the segments, oldest segment
  first. One store's writes are listed in the order it made them; the writes of several stores
  are grouped by segment, so their order does not tell which was made first. Read every value
  in that order: a scattered order decodes the surrounding values again for most of them.
- Among records of a key that stores wrote without seeing each other's, which one wins is
  unspecified, but every reader picks the same one.
- `max_segment_bytes` / `maxSegmentBytes` must be between 1,024 and 100,000,000; the upper
  bound keeps every file under GitHub's 100 MB limit.
- An option out of range and a record that does not fit in a segment raise `ValueError` in
  Python and an error whose `code` is `'InvalidArg'` in TypeScript. An I/O failure raises
  `OSError` and a corrupt store `RuntimeError`; both have the `code` `'GenericFailure'`.
- A record is written with one `write` call and is not synced to disk. After a crash, a store
  ignores a record that was only partly written and everything else remains readable.
- Each record carries a CRC-32 that is checked whenever it is read.
- A store that starts writing continues the newest segment when the same directory wrote it
  last and no other store is writing to it, so short runs do not each leave a small file behind.
  The directory remembers this in `.commitkv-writer`, which commitkv lists in a `.gitignore` it keeps
  in the directory; commit that `.gitignore` with the segments.
- `get` decodes the values written around the requested one and those at the start of its
  segment (about 1 MiB each, more when single values are larger) and keeps recently decoded
  values in memory.

## Development

```bash
mise install               # bun, node, rust, uv
bun install
bun run build              # builds the platform addon and the dist/ that `import 'commitkv'` resolves to
bun wb test                # Bun tests of the TypeScript API, and pytest through `uv run`
cargo test --release --manifest-path rust/Cargo.toml
```

The tests load the platform addon from `native/`: after changing Rust code, rerun `bun run build/native`,
which builds only that. `uv run` rebuilds the Python extension module by itself.

`cargo run --release --manifest-path rust/Cargo.toml --example lines -- <file>` stores every
line of a file as a value and reports the stored size and the throughput.

Layout: `rust/crates/commitkv` is the store, `rust/crates/commitkv-node` the Node-API addon that
`src/index.ts` wraps, and `rust/crates/commitkv-python` the extension module that `python/commitkv`
re-exports and types.

Read [docs/format.md](docs/format.md) before changing how segments are written or read: it is
the contract that keeps existing caches readable.

Read [docs/releasing.md](docs/releasing.md) when publishing locally or retrying a Python release.
