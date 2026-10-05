# kvzip

A compressed key-value store for caches that are committed to git, usable from Python,
Node.js, and Bun.

- **No file ever exceeds a size limit.** Records are appended to segment files; before a record
  is written, a segment that it would push past the limit (32 MiB by default) is closed and a
  new one is started. A record that cannot fit even in an empty segment is rejected.
- **Git-friendly.** A closed segment is never rewritten, so git stores it once. Stores that
  write at the same time write to different segments, so branches that both add records merge
  without conflicts.
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

- `put` replaces an earlier value of the key. The earlier record stays in its segment: kvzip
  never deletes or compacts.
- A store reads the directory when it opens and when `refresh` is called; records that other
  stores write in between are invisible until then.
- When several stores write the same key at the same time, which value wins is unspecified, but
  every reader picks the same one.
- A record is written with one `write` call and is not synced to disk. After a crash, a store
  ignores a record that was only partly written and everything else remains readable.
- Each record carries a CRC-32 that is checked whenever it is read.
- A store that starts writing continues the newest segment when no other store is writing to it,
  so short runs do not each leave a small file behind.
- `get` decodes at most the values written around the requested one (about 1 MiB) and keeps
  recently decoded values in memory.

## Development

```bash
mise install               # bun, node, rust, uv
bun install
bun run build/native       # builds native/kvzip.node, which the TypeScript wrapper loads
bun wb test                # Bun tests of the TypeScript API, and pytest through `uv run`
cargo test --release --manifest-path rust/Cargo.toml
```

Rerun `bun run build/native` after changing Rust code; `uv run` rebuilds the Python extension
module by itself.

`cargo run --release --manifest-path rust/Cargo.toml --example lines -- <file>` stores every
line of a file as a value and reports the stored size and the throughput.

Layout: `rust/crates/kvzip` is the store, `rust/crates/kvzip-node` the Node-API addon that
`src/index.ts` wraps, and `rust/crates/kvzip-python` the extension module that `python/kvzip`
re-exports and types.

Read [docs/format.md](docs/format.md) before changing how segments are written or read: it is
the contract that keeps existing caches readable.
