import { createRequire } from 'node:module';

interface NativeStore {
  get(key: Uint8Array): Buffer | null;
  put(key: Uint8Array, value: Uint8Array): void;
  has(key: Uint8Array): boolean;
  readonly size: number;
  keys(): Buffer[];
  refresh(): void;
}

// The addon is built by `bun run build/native`; this path is the same from `src/` and `dist/`.
const native = createRequire(import.meta.url)('../native/kvzip.node') as {
  Store: new (directory: string, maxSegmentBytes?: number) => NativeStore;
};

/** A key or a value; a string stands for its UTF-8 encoding. */
export type Data = string | Uint8Array;

export interface StoreOptions {
  /** No segment file grows beyond this many bytes: 32 MiB by default, between 1,024 and 100,000,000. */
  maxSegmentBytes?: number;
}

/**
 * A directory of compressed segments, none of which exceeds `maxSegmentBytes`. Several stores,
 * in one process or many, may use the same directory at once.
 *
 * Errors carry a `code`: `'InvalidArg'` for an option out of range or a record that does not
 * fit in a segment, `'GenericFailure'` for an I/O failure or a corrupt store.
 */
export class Store {
  readonly #native: NativeStore;

  constructor(directory: string, options: StoreOptions = {}) {
    this.#native = new native.Store(directory, options.maxSegmentBytes);
  }

  get(key: Data): Buffer | undefined {
    return this.#native.get(toBytes(key)) ?? undefined;
  }

  /**
   * Stores a value, replacing an earlier one. Throws an error whose `code` is `'InvalidArg'`
   * when the record does not fit in a segment.
   */
  put(key: Data, value: Data): void {
    this.#native.put(toBytes(key), toBytes(value));
  }

  has(key: Data): boolean {
    return this.#native.has(toBytes(key));
  }

  get size(): number {
    return this.#native.size;
  }

  keys(): Buffer[] {
    return this.#native.keys();
  }

  /** Picks up records that other stores wrote to the directory. */
  refresh(): void {
    this.#native.refresh();
  }
}

function toBytes(data: Data): Uint8Array {
  return typeof data === 'string' ? Buffer.from(data) : data;
}
