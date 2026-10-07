//! Cheap change fingerprints and full content hashes (plan §11).
//!
//! * [`fast_fingerprint`] uses only discovery data (path, size, mtime) — no file I/O.
//!   It doubles as the thumbnail cache key component.
//! * [`content_hash`] is the full BLAKE3 of the bytes, computed only when the bytes
//!   are already in memory for metadata extraction.

/// `blake3(relative_path ‖ 0x00 ‖ size_le ‖ mtime_ns_le)`, first 16 hex chars.
pub fn fast_fingerprint(relative_path: &str, size: u64, mtime_ns: i64) -> String {
    let mut h = blake3::Hasher::new();
    h.update(relative_path.as_bytes());
    h.update(&[0]);
    h.update(&size.to_le_bytes());
    h.update(&mtime_ns.to_le_bytes());
    let hex = h.finalize().to_hex();
    hex.as_str()[..16].to_string()
}

/// Full BLAKE3 hash of `bytes`, lowercase hex (64 chars).
pub fn content_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_stable_and_sensitive() {
        let a = fast_fingerprint("a/b.svg", 10, 1234);
        assert_eq!(a.len(), 16);
        assert_eq!(a, fast_fingerprint("a/b.svg", 10, 1234));
        assert_ne!(a, fast_fingerprint("a/b.svg", 11, 1234));
        assert_ne!(a, fast_fingerprint("a/b.svg", 10, 1235));
        assert_ne!(a, fast_fingerprint("a/c.svg", 10, 1234));
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn content_hash_is_blake3_hex() {
        let h = content_hash(b"hello");
        assert_eq!(h.len(), 64);
        assert_eq!(h, blake3::hash(b"hello").to_hex().to_string());
    }
}
