//! Content hashing for move/rename detection. Full-file hashing of multi-GB
//! videos is too slow, so we hash the file size plus a bounded head sample —
//! enough to give a stable identity that survives a move but changes when the
//! file content changes.
//!
//! The sample is deliberately small: the file *size* is already a near-unique
//! discriminator, and the head adds the container header + first frames on top,
//! so two distinct videos practically never collide. Keeping it at 1 MiB (down
//! from 8) cuts import disk I/O ~8x — important on HDDs, where reading 8 MiB off
//! the head of every lecture dominated import time. (A file both moved *and*
//! re-imported across this change may miss hash-based rename matching once and
//! fall back to path matching; content is unchanged, so nothing is lost on a
//! normal same-path re-import.)

use crate::error::Result;
use std::io::Read;
use std::path::Path;

/// Bytes of file head folded into the hash (1 MiB).
const SAMPLE_BYTES: u64 = 1024 * 1024;

/// blake3 over `size` + up to `SAMPLE_BYTES` of the file head. Returns a hex string.
pub fn content_hash(path: &Path, size: u64) -> Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&size.to_le_bytes());

    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 1024 * 1024];
    let mut read_total: u64 = 0;

    while read_total < SAMPLE_BYTES {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        read_total += n as u64;
    }

    Ok(hasher.finalize().to_hex().to_string())
}
