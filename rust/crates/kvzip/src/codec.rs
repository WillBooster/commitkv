//! zstd coding of a group: one frame whose values are flushed one by one, so every value can
//! refer to the values before it and to the prefix the group starts from.

use std::sync::Arc;

use zstd_safe::{
    zstd_sys::ZSTD_EndDirective, CCtx, CParameter, DCtx, DParameter, InBuffer, OutBuffer,
    ResetDirective,
};

use crate::error::{Error, Result};

/// Higher levels gain little on values coded against a megabyte of similar ones: 19 stores about
/// a tenth less than 9 at a twentieth of the speed.
const LEVEL: i32 = 9;
/// Covers `BASE_MAX` plus a group of `GROUP_RAW_TARGET` (see store.rs), so a value can refer to
/// the whole prefix wherever it sits in the group.
const WINDOW_LOG: u32 = 22;

pub struct GroupEncoder {
    // Declared before `prefix` so that it is dropped first: it refers to `prefix`'s bytes.
    cctx: CCtx<'static>,
    prefix: Arc<[u8]>,
}

impl GroupEncoder {
    pub fn new() -> Self {
        let mut cctx = CCtx::create();
        for parameter in [
            CParameter::CompressionLevel(LEVEL),
            CParameter::WindowLog(WINDOW_LOG),
        ] {
            cctx.set_parameter(parameter)
                .expect("constant zstd parameters are valid");
        }
        Self {
            cctx,
            prefix: Arc::from([]),
        }
    }

    pub fn start_group(&mut self, prefix: Arc<[u8]>) {
        self.cctx
            .reset(ResetDirective::SessionOnly)
            .expect("resetting a zstd session cannot fail");
        // SAFETY: zstd reads the prefix until the frame ends, i.e. until the next reset. The
        // bytes live in an `Arc` this struct holds until the next `start_group` (which resets
        // first) or until drop (which drops `cctx` first).
        let bytes: &'static [u8] = unsafe { &*Arc::as_ptr(&prefix) };
        self.prefix = prefix;
        self.cctx
            .ref_prefix(bytes)
            .expect("referencing a prefix after a reset cannot fail");
    }

    /// Appends to `out` the bytes that let a decoder recover everything up to `value`.
    pub fn push(&mut self, value: &[u8], out: &mut Vec<u8>) {
        let mut input = InBuffer::around(value);
        loop {
            out.reserve(CCtx::out_size());
            let mut output = OutBuffer::around_pos(out, out.len());
            let pending = self
                .cctx
                .compress_stream2(&mut output, &mut input, ZSTD_EndDirective::ZSTD_e_flush)
                .expect("zstd compression into a growable buffer cannot fail");
            if pending == 0 {
                return;
            }
        }
    }
}

/// Decodes the values of a group from its records in order, each given as its payload and the
/// length of the value it declares.
pub fn decode_group(prefix: &[u8], records: &[(&[u8], u64)]) -> Result<Vec<u8>> {
    let raw_len = records
        .iter()
        .try_fold(0usize, |total, (_, value_len)| {
            total.checked_add(usize::try_from(*value_len).ok()?)
        })
        .ok_or_else(|| Error::Corrupt("a group declares more bytes than fit".into()))?;
    let mut dctx = DCtx::create();
    // Frames written by a store never declare more, and a larger declared window would size
    // the decoder's buffer before a single byte is checked.
    dctx.set_parameter(DParameter::WindowLogMax(WINDOW_LOG))
        .expect("a constant zstd parameter is valid");
    dctx.ref_prefix(prefix).map_err(zstd_error)?;
    let mut raw = Vec::new();
    raw.try_reserve_exact(raw_len)
        .map_err(|_| Error::Corrupt(format!("a group declares {raw_len} bytes")))?;
    for (payload, value_len) in records {
        let value_end = raw.len() + *value_len as usize;
        let mut input = InBuffer::around(payload);
        while input.pos() < payload.len() {
            let (consumed, produced) = (input.pos(), raw.len());
            let mut output = OutBuffer::around_pos(&mut raw, produced);
            dctx.decompress_stream(&mut output, &mut input)
                .map_err(zstd_error)?;
            if input.pos() == consumed && raw.len() == produced {
                return Err(Error::Corrupt("a group is longer than declared".into()));
            }
        }
        // Values are addressed by their declared lengths, so each payload must end exactly
        // where its value does.
        if raw.len() != value_end {
            return Err(Error::Corrupt("a value is not as long as declared".into()));
        }
    }
    Ok(raw)
}

fn zstd_error(code: usize) -> Error {
    Error::Corrupt(zstd_safe::get_error_name(code).into())
}
