//! On-disk thumbnail cache. Key = fingerprint (path+size+mtime) + renderer version + size,
//! so any file change or renderer upgrade naturally misses the cache.

use std::path::{Path, PathBuf};
use svg_core::model::AssetId;
use svg_core::svg::RENDERER_VERSION;

pub fn cache_key(fingerprint: &str, size: u32) -> String {
    format!("{fingerprint}-r{RENDERER_VERSION}-s{size}")
}

pub fn thumb_path(dir: &Path, id: AssetId, fingerprint: &str, size: u32) -> PathBuf {
    dir.join(format!("{:02x}", id % 256)).join(format!("{id}-{}.png", cache_key(fingerprint, size)))
}

/// Atomically write a thumbnail (write temp + rename) so readers never see partial files.
pub fn write_thumb(path: &Path, png: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, png)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            // Another worker may have won the race; that's fine if the target exists.
            if path.exists() { Ok(()) } else { Err(e) }
        }
    }
}

pub fn remove_thumb(dir: &Path, id: AssetId, fingerprint: &str, size: u32) {
    let _ = std::fs::remove_file(thumb_path(dir, id, fingerprint, size));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_and_path_layout() {
        let p = thumb_path(Path::new("/c"), 300, "abcd", 256);
        assert!(p.ends_with(format!("2c/300-abcd-r{RENDERER_VERSION}-s256.png")));
        let dir = std::env::temp_dir().join(format!("thumbtest-{}", std::process::id()));
        let p = thumb_path(&dir, 1, "f", 64);
        write_thumb(&p, b"png").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"png");
        remove_thumb(&dir, 1, "f", 64);
        assert!(!p.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
