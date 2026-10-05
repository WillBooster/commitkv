# kvzip

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

kvzip runs on Linux and macOS. It is not published to PyPI or npm yet: build it from a checkout
(see [Development](#development)).

## Usage

A `str` / `string` key or value stands for its UTF-8 encoding; values are returned as bytes.

```python
from kvzip import Store

store = Store("cache")                 # or Store("cache", max_segment_bytes=16 * 1024 * 1024)
store.put("key", "value")
store.get("key")                       # b"value"; None when the key is absent
"key" in store, len(store), store.keys()
store.refresh()                        # pick up what other stores wrote to the directory
```

```ts
import { Store } from 'kvzip';

const store = new Store('cache'); // or new Store('cache', { maxSegmentBytes: 16 * 1024 * 1024 })
store.put('key', 'value');
store.get('key')?.toString(); // 'value'; undefined when the key is absent
(store.has('key'), store.size, store.keys());
store.refresh(); // pick up what other stores wrote to the directory
```

## Behavior

- `put` replaces every value of the key that the store has seen. The earlier records stay in
  their segments: kvzip never deletes or compacts.
- A store reads the directory when it opens, when `refresh` is called, and when a `put` starts
  writing (the first `put`, and the first after a failed one); records that other stores write
  stay invisible until one of those happens. Call `refresh` after
  replacing segments under an open store, for example by checking out another branch.
- Among records of a key that stores wrote without seeing each other's, which one wins is
  unspecified, but every reader picks the same one.
- `max_segment_bytes` / `maxSegmentBytes` is at most 100,000,000, which keeps every file under
  GitHub's 100 MB limit.
- A record is written with one `write` call and is not synced to disk. After a crash, a store
  ignores a record that was only partly written and everything else remains readable.
- Each record carries a CRC-32 that is checked whenever it is read.
- A store that starts writing continues the newest segment when the same directory wrote it
  last and no other store is writing to it, so short runs do not each leave a small file behind.
  The directory remembers this in `.kvzip-writer`, which kvzip lists in a `.gitignore` it keeps
  in the directory; commit that `.gitignore` with the segments.
- `get` decodes the values written around the requested one and those at the start of its
  segment (about 1 MiB each, more when single values are larger) and keeps recently decoded
  values in memory.

## Development

```bash
mise install               # bun, node, rust, uv
bun install
bun run build              # builds native/kvzip.node and the dist/ that `import 'kvzip'` resolves to
bun wb test                # Bun tests of the TypeScript API, and pytest through `uv run`
cargo test --release --manifest-path rust/Cargo.toml
```

The tests load `native/kvzip.node`: after changing Rust code, rerun `bun run build/native`,
which builds only that. `uv run` rebuilds the Python extension module by itself.

`cargo run --release --manifest-path rust/Cargo.toml --example lines -- <file>` stores every
line of a file as a value and reports the stored size and the throughput.

Layout: `rust/crates/kvzip` is the store, `rust/crates/kvzip-node` the Node-API addon that
`src/index.ts` wraps, and `rust/crates/kvzip-python` the extension module that `python/kvzip`
re-exports and types.

Read [docs/format.md](docs/format.md) before changing how segments are written or read: it is
the contract that keeps existing caches readable.
