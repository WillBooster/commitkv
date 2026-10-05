//! kvzip: a compressed key-value store for caches kept in git. Records are appended to segment
//! files that never exceed a size limit and are never rewritten; docs/format.md specifies the
//! layout.

mod cache;
mod codec;
mod error;
mod format;
mod store;

pub use error::{Error, Result};
pub use store::{Options, Store, DEFAULT_MAX_SEGMENT_BYTES, MAX_MAX_SEGMENT_BYTES};
