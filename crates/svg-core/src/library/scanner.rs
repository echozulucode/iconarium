//! Progressive recursive discovery (plan §8) and size+mtime reconciliation (plan §11).
//!
//! Discovery never reads file contents and never collects the whole tree before
//! reporting: SVGs are emitted in batches as the walk proceeds, and the walk checks a
//! cancel flag between entries.

use std::collections::{HashMap, HashSet};
use std::fs::Metadata;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, UNIX_EPOCH};

use walkdir::{DirEntry, WalkDir};

use crate::error::{CoreError, Result};
use crate::model::{AssetId, DiscoveredFile};

/// Discovery options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    /// Files per `on_batch` call (the final batch may be smaller).
    pub batch_size: usize,
    /// Skip dot-prefixed files/directories (and, on Windows, entries with the hidden attribute).
    pub ignore_hidden: bool,
    /// Follow symbolic links / junctions (walkdir detects loops).
    pub follow_links: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            batch_size: 250,
            ignore_hidden: true,
            follow_links: false,
        }
    }
}

/// Outcome of one discovery pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanStats {
    pub files_found: usize,
    pub dirs_visited: usize,
    pub errors: usize,
    /// True when the walk stopped early because the cancel flag was set. A cancelled
    /// pass must not be used for deletion detection ([`Reconciler::finish`]).
    pub cancelled: bool,
    pub elapsed_ms: u64,
}

/// Whether a file name has the `.svg` extension (case-insensitive).
pub fn is_svg_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() > 4 && b[b.len() - 4..].eq_ignore_ascii_case(b".svg")
}

/// Whether any component of a '/'-separated relative path is dot-prefixed.
/// Useful for watcher events, which are not filtered by the walker.
pub fn is_hidden_relative(relative_path: &str) -> bool {
    relative_path
        .split('/')
        .any(|c| c.starts_with('.') && c != "." && c != "..")
}

fn is_hidden_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false; // never filter the root itself
    }
    if entry
        .file_name()
        .to_str()
        .map(|n| n.starts_with('.'))
        .unwrap_or(false)
    {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if let Ok(md) = entry.metadata() {
            if md.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0 {
                return true;
            }
        }
    }
    false
}

fn mtime_ns(md: &Metadata) -> i64 {
    match md.modified() {
        Ok(t) => match t.duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
            Err(e) => -i64::try_from(e.duration().as_nanos()).unwrap_or(i64::MAX),
        },
        Err(_) => 0,
    }
}

/// '/'-separated path of `abs` relative to `root`. `None` if `abs` is not strictly
/// inside `root`, contains `..`, or is not valid UTF-8.
pub fn to_relative(root: &Path, abs: &Path) -> Option<String> {
    let rel = abs.strip_prefix(root).ok()?;
    let mut out = String::new();
    for c in rel.components() {
        match c {
            Component::Normal(s) => {
                if !out.is_empty() {
                    out.push('/');
                }
                out.push_str(s.to_str()?);
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Absolute path for a '/'-separated relative path.
pub fn to_absolute(root: &Path, relative_path: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in relative_path.split('/').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p
}

/// Stat a single library-relative path (watcher-driven reconcile). Returns `None` when
/// the path does not exist, is not a regular file, or is not an `.svg`.
/// Hidden filtering is the caller's choice (see [`is_hidden_relative`]).
pub fn stat_one(root: &Path, relative_path: &str) -> Option<DiscoveredFile> {
    if relative_path.split('/').any(|c| c == "..") {
        return None;
    }
    let filename = relative_path.rsplit('/').next()?.to_string();
    if !is_svg_name(&filename) {
        return None;
    }
    let md = std::fs::metadata(to_absolute(root, relative_path)).ok()?;
    if !md.is_file() {
        return None;
    }
    Some(DiscoveredFile {
        relative_path: relative_path.to_string(),
        filename,
        file_size: md.len(),
        mtime_ns: mtime_ns(&md),
    })
}

/// Walk `root` recursively, emitting `.svg` files (case-insensitive) in batches of
/// `opts.batch_size` as they are found. Order follows the filesystem (unsorted).
pub fn scan(
    root: &Path,
    opts: &ScanOptions,
    cancel: &AtomicBool,
    mut on_batch: impl FnMut(Vec<DiscoveredFile>),
) -> Result<ScanStats> {
    let started = Instant::now();
    let root_md = std::fs::metadata(root)?;
    if !root_md.is_dir() {
        return Err(CoreError::Invalid(format!(
            "library root is not a directory: {}",
            root.display()
        )));
    }
    let batch_size = opts.batch_size.max(1);
    let mut stats = ScanStats::default();
    let mut batch: Vec<DiscoveredFile> = Vec::with_capacity(batch_size);
    let ignore_hidden = opts.ignore_hidden;

    let walker = WalkDir::new(root)
        .follow_links(opts.follow_links)
        .into_iter()
        .filter_entry(move |e| !(ignore_hidden && is_hidden_entry(e)));

    for item in walker {
        if cancel.load(Ordering::Relaxed) {
            stats.cancelled = true;
            break;
        }
        let entry = match item {
            Ok(e) => e,
            Err(err) => {
                stats.errors += 1;
                tracing::debug!(error = %err, "scan: skipping unreadable entry");
                continue;
            }
        };
        let ft = entry.file_type();
        if ft.is_dir() {
            stats.dirs_visited += 1;
            continue;
        }
        if !ft.is_file() {
            continue; // symlink not followed, device, socket, ...
        }
        let Some(name) = entry.file_name().to_str() else {
            stats.errors += 1;
            tracing::debug!(path = %entry.path().display(), "scan: skipping non-UTF-8 file name");
            continue;
        };
        if !is_svg_name(name) {
            continue;
        }
        let Some(relative_path) = to_relative(root, entry.path()) else {
            stats.errors += 1;
            tracing::debug!(path = %entry.path().display(), "scan: skipping non-UTF-8 path");
            continue;
        };
        let md = match entry.metadata() {
            Ok(m) => m,
            Err(err) => {
                stats.errors += 1;
                tracing::debug!(path = %entry.path().display(), error = %err, "scan: metadata failed");
                continue;
            }
        };
        batch.push(DiscoveredFile {
            filename: name.to_string(),
            relative_path,
            file_size: md.len(),
            mtime_ns: mtime_ns(&md),
        });
        stats.files_found += 1;
        if batch.len() >= batch_size {
            on_batch(std::mem::replace(
                &mut batch,
                Vec::with_capacity(batch_size),
            ));
        }
    }
    // Files already found are real even when cancelled; deliver them.
    if !batch.is_empty() {
        on_batch(batch);
    }
    stats.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(stats)
}

/// What the index already knows about a path (from [`crate::index::Database::library_snapshot`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExistingEntry {
    pub id: AssetId,
    pub file_size: u64,
    pub mtime_ns: i64,
}

/// Classification of one discovery batch against the index snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchDiff {
    pub new: Vec<DiscoveredFile>,
    pub changed: Vec<(AssetId, DiscoveredFile)>,
    pub unchanged: usize,
}

/// Streams discovery batches against the persisted snapshot: new / changed (size or
/// mtime differs) / unchanged; whatever was never seen is deleted ([`Reconciler::finish`]).
/// Renames are treated as delete + new.
#[derive(Debug, Default)]
pub struct Reconciler {
    existing: HashMap<String, ExistingEntry>,
    seen: HashSet<String>,
}

impl Reconciler {
    pub fn new(existing: HashMap<String, ExistingEntry>) -> Self {
        Self {
            existing,
            seen: HashSet::new(),
        }
    }

    pub fn classify(&mut self, batch: Vec<DiscoveredFile>) -> BatchDiff {
        let mut diff = BatchDiff::default();
        for f in batch {
            if !self.seen.insert(f.relative_path.clone()) {
                continue; // duplicate report of the same path within one pass
            }
            match self.existing.get(&f.relative_path) {
                None => diff.new.push(f),
                Some(e) if e.file_size == f.file_size && e.mtime_ns == f.mtime_ns => {
                    diff.unchanged += 1
                }
                Some(e) => diff.changed.push((e.id, f)),
            }
        }
        diff
    }

    /// IDs of indexed assets that were not seen during the pass (sorted).
    /// Only meaningful after a complete (non-cancelled) scan.
    pub fn finish(self) -> Vec<AssetId> {
        let mut gone: Vec<AssetId> = self
            .existing
            .iter()
            .filter(|(p, _)| !self.seen.contains(*p))
            .map(|(_, e)| e.id)
            .collect();
        gone.sort_unstable();
        gone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_name_and_hidden_helpers() {
        assert!(is_svg_name("a.svg"));
        assert!(is_svg_name("A.SVG"));
        assert!(is_svg_name("x.SvG"));
        assert!(!is_svg_name(".svg"));
        assert!(!is_svg_name("a.svgz"));
        assert!(!is_svg_name("a.png"));
        assert!(is_hidden_relative(".git/a.svg"));
        assert!(is_hidden_relative("a/.b.svg"));
        assert!(!is_hidden_relative("a/b.svg"));
    }

    #[test]
    fn relative_paths() {
        let root = Path::new("/lib/root");
        assert_eq!(
            to_relative(root, Path::new("/lib/root/a/b.svg")).as_deref(),
            Some("a/b.svg")
        );
        assert_eq!(to_relative(root, Path::new("/lib/root")), None);
        assert_eq!(to_relative(root, Path::new("/elsewhere/b.svg")), None);
        assert_eq!(to_absolute(root, "a/b.svg"), Path::new("/lib/root/a/b.svg"));
    }
}
