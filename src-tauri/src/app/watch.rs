//! Filesystem watcher (plan §31): debounced notify events are forwarded to a coalescing
//! thread that applies small change sets per path and falls back to one full reconcile
//! pass for bursts (git checkout), directory operations or watcher overflow.

use super::AppCore;
use crossbeam_channel::{unbounded, Receiver};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer_opt, DebounceEventResult, Debouncer, NoCache};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use svg_core::library::scanner::{is_hidden_relative, is_svg_name, to_relative};
use svg_core::model::LibraryId;

/// Above this many changed paths in one coalesced burst, do a full reconcile instead.
const PER_PATH_LIMIT: usize = 200;
const DEBOUNCE: Duration = Duration::from_millis(500);

pub struct WatchHandle {
    _debouncer: Debouncer<notify::RecommendedWatcher, NoCache>,
}

enum Msg {
    Paths(Vec<PathBuf>),
    Rescan,
}

impl AppCore {
    pub(crate) fn start_watcher(self: &Arc<Self>, lib_id: LibraryId, root: &Path, gen: u64) {
        let (tx, rx) = unbounded::<Msg>();
        let handler = move |res: DebounceEventResult| match res {
            Ok(events) => {
                let mut paths = Vec::new();
                let mut rescan = false;
                for ev in events {
                    rescan |= ev.need_rescan();
                    // Access events (inotify IN_OPEN / IN_CLOSE_NOWRITE on Linux) are caused by
                    // our own reads: the scanner opening directories would classify as
                    // "structural" and trigger a full reconcile, which opens the directories
                    // again — an endless rescan loop. Reads never change the library.
                    if matches!(ev.event.kind, notify::EventKind::Access(_)) {
                        continue;
                    }
                    paths.extend(ev.event.paths.iter().cloned());
                }
                if rescan {
                    let _ = tx.send(Msg::Rescan);
                } else if !paths.is_empty() {
                    let _ = tx.send(Msg::Paths(paths));
                }
            }
            Err(errs) => {
                tracing::warn!("watcher errors: {errs:?}");
                let _ = tx.send(Msg::Rescan);
            }
        };
        let mut debouncer = match new_debouncer_opt::<_, notify::RecommendedWatcher, NoCache>(
            DEBOUNCE,
            None,
            handler,
            NoCache,
            notify::Config::default(),
        ) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("file watcher unavailable: {e}");
                return;
            }
        };
        if let Err(e) = debouncer.watch(root, RecursiveMode::Recursive) {
            tracing::warn!("cannot watch {}: {e}", root.display());
            return;
        }
        let core = self.clone();
        let root_buf = root.to_path_buf();
        {
            // Install only if this library is still the active one (checked under the lock
            // that open_library uses to drop the previous watcher).
            let mut slot = self.watcher.lock();
            if self.generation() != gen {
                return;
            }
            *slot = Some(WatchHandle {
                _debouncer: debouncer,
            });
        }
        std::thread::Builder::new()
            .name("watch-apply".into())
            .spawn(move || apply_loop(core, lib_id, root_buf, gen, rx))
            .ok();
        tracing::info!("watching {}", root.display());
    }
}

fn apply_loop(core: Arc<AppCore>, lib_id: LibraryId, root: PathBuf, gen: u64, rx: Receiver<Msg>) {
    while let Ok(first) = rx.recv() {
        if core.generation() != gen {
            break;
        }
        // Coalesce everything that is already queued.
        let ignore_hidden = core.settings.read().ignore_hidden;
        let mut rescan = false;
        let mut rels: BTreeSet<String> = BTreeSet::new();
        let absorb = |m: Msg, rescan: &mut bool, rels: &mut BTreeSet<String>| match m {
            Msg::Rescan => *rescan = true,
            Msg::Paths(ps) => {
                for p in ps {
                    match classify(&root, &p, ignore_hidden) {
                        Change::Svg(rel) => {
                            rels.insert(rel);
                        }
                        Change::Structural => *rescan = true,
                        // A removed path that isn't an SVG may have been a folder (also one
                        // with a dot in its name, e.g. "icons.v2"): reconcile if we index
                        // anything below it.
                        Change::MaybeDir(rel) => {
                            if core.has_assets_under(&rel) {
                                *rescan = true;
                            }
                        }
                        Change::Ignore => {}
                    }
                }
            }
        };
        absorb(first, &mut rescan, &mut rels);
        while let Ok(m) = rx.try_recv() {
            absorb(m, &mut rescan, &mut rels);
        }
        if rescan || rels.len() > PER_PATH_LIMIT {
            tracing::info!(
                "watcher: full reconcile ({} paths, rescan={rescan})",
                rels.len()
            );
            core.reconcile_now(lib_id, &root, gen);
        } else if !rels.is_empty() {
            let rels: Vec<String> = rels.into_iter().collect();
            tracing::debug!("watcher: applying {} path changes", rels.len());
            core.apply_path_changes(lib_id, &root, &rels, gen);
        }
    }
}

enum Change {
    Svg(String),
    Structural,
    MaybeDir(String),
    Ignore,
}

fn classify(root: &Path, p: &Path, ignore_hidden: bool) -> Change {
    let Some(rel) = to_relative(root, p) else {
        return Change::Ignore;
    };
    if rel.is_empty() {
        return Change::Structural;
    }
    if ignore_hidden && is_hidden_relative(&rel) {
        return Change::Ignore;
    }
    let name = rel.rsplit('/').next().unwrap_or(&rel);
    if is_svg_name(name) {
        return Change::Svg(rel);
    }
    // A directory was created/renamed/removed (or something we can't tell apart from one):
    // its contents may have moved, so reconcile the tree.
    if p.is_dir() || (!p.exists() && Path::new(name).extension().is_none()) {
        Change::Structural
    } else if !p.exists() {
        Change::MaybeDir(rel)
    } else {
        Change::Ignore
    }
}
