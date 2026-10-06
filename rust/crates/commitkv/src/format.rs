//! On-disk layout of a segment; docs/format.md is the specification.

/// The last byte is the format version. The NUL makes git treat every segment as binary;
/// without it, a segment that is mostly text (long keys, little payload) would have its CRLFs
/// rewritten in repositories that normalize line endings.
pub const HEADER: [u8; 10] = *b"commitkv\0\x01";

const KIND_CONTINUE: u8 = 0;
const KIND_NEW_GROUP: u8 = 1;
const CRC_LEN: usize = 4;

pub struct Record<'a> {
    pub new_group: bool,
    pub key: &'a [u8],
    pub raw_len: u64,
    pub payload: &'a [u8],
    /// Bytes the record occupies in the segment.
    pub len: usize,
}

pub fn encode_record(new_group: bool, key: &[u8], raw_len: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 3 * 10 + key.len() + payload.len() + CRC_LEN);
    out.push(if new_group {
        KIND_NEW_GROUP
    } else {
        KIND_CONTINUE
    });
    push_varint(&mut out, key.len() as u64);
    out.extend_from_slice(key);
    push_varint(&mut out, raw_len);
    push_varint(&mut out, payload.len() as u64);
    out.extend_from_slice(payload);
    let crc = crc32fast::hash(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

/// Parses the record at the start of `buf`; `None` when it is incomplete or fails its checksum.
pub fn parse_record(buf: &[u8]) -> Option<Record<'_>> {
    let new_group = match *buf.first()? {
        KIND_CONTINUE => false,
        KIND_NEW_GROUP => true,
        _ => return None,
    };
    let mut pos = 1;
    let key = read_bytes(buf, &mut pos)?;
    let raw_len = read_varint(buf, &mut pos)?;
    let payload = read_bytes(buf, &mut pos)?;
    let crc = buf.get(pos..pos.checked_add(CRC_LEN)?)?;
    if crc != crc32fast::hash(&buf[..pos]).to_le_bytes() {
        return None;
    }
    Some(Record {
        new_group,
        key,
        raw_len,
        payload,
        len: pos + CRC_LEN,
    })
}

fn read_bytes<'a>(buf: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    let len = usize::try_from(read_varint(buf, pos)?).ok()?;
    let bytes = buf.get(*pos..pos.checked_add(len)?)?;
    *pos += len;
    Some(bytes)
}

fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *buf.get(*pos)?;
        *pos += 1;
        let bits = u64::from(byte & 0x7f);
        // The tenth byte holds the one remaining bit.
        if bits << shift >> shift != bits {
            return None;
        }
        value |= bits << shift;
        if byte < 0x80 {
            return Some(value);
        }
    }
    None
}
