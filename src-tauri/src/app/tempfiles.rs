//! Temporary files for region drag-out. Receivers may read a dropped file after the drag
//! call returns, so files are never deleted immediately: they are purged when older than
//! `MAX_AGE`, and everything is purged at startup and shutdown.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

const MAX_AGE: Duration = Duration::from_secs(10 * 60);

pub struct TempFiles {
    dir: PathBuf,
    counter: AtomicU64,
}

impl TempFiles {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            counter: AtomicU64::new(0),
        }
    }

    /// Write `contents` to a fresh per-drag folder as `file_name` and return its path.
    pub fn write(&self, file_name: &str, contents: &[u8]) -> std::io::Result<PathBuf> {
        self.cleanup_older_than(MAX_AGE);
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let folder = self.dir.join(format!("drag-{stamp}-{n}"));
        std::fs::create_dir_all(&folder)?;
        let path = folder.join(sanitize(file_name));
        std::fs::write(&path, contents)?;
        Ok(path)
    }

    pub fn cleanup_older_than(&self, age: Duration) {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let now = SystemTime::now();
        for e in rd.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .map(|t| now.duration_since(t).unwrap_or_default() > age)
                .unwrap_or(true);
            if old {
                remove(&e.path());
            }
        }
    }

    pub fn purge_all(&self) {
        self.cleanup_older_than(Duration::ZERO);
    }
}

fn remove(p: &Path) {
    if p.is_dir() {
        let _ = std::fs::remove_dir_all(p);
    } else {
        let _ = std::fs::remove_file(p);
    }
}

/// Keep file names valid on Windows.
pub fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim_end_matches(['.', ' ']).to_string();
    if s.is_empty() {
        "selection.svg".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn write_and_purge() {
        let dir = std::env::temp_dir().join(format!("svgtmp-{}", std::process::id()));
        let t = TempFiles::new(dir.clone());
        let p = t.write("a:b?.svg", b"<svg/>").unwrap();
        assert!(p.ends_with("a_b_.svg"));
        assert!(p.exists());
        t.purge_all();
        assert!(!p.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
