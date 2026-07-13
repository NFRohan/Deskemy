//! Content hashing for move/rename detection. Full-file hashing of multi-GB
//! videos is too slow, so we hash the file size plus a bounded head sample —
//! enough to give a stable identity that survives a move but changes when the
//! file content changes.

use crate::error::Result;
use std::io::Read;
use std::path::Path;

/// Bytes of file head folded into the hash (8 MiB).
const SAMPLE_BYTES: u64 = 8 * 1024 * 1024;

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
