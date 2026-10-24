//! Operations over an [`IndexReader`]: search, details, tags, folders, previews, export
//! and tag edits. The CLI and the MCP server are thin layers over these.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use svg_core::model::{AssetId, AssetRecord, ProcessingState};
use svg_core::svg::render_thumbnail_pixmap;

use crate::dto::*;
use crate::error::{CatalogError, Result};
use crate::metadata::{self, normalize_key, normalize_tag};
use crate::reader::{IndexReader, LibraryData, Snapshot};
use crate::sheet::{self, SheetBackground, SheetCell};

/// Assets of one library selected for a tag edit: (library, [(id, relative path)]).
type LibraryAssets = (Arc<LibraryData>, Vec<(AssetId, String)>);

pub const DEFAULT_LIMIT: usize = 20;
pub const MAX_LIMIT: usize = 100;
const MAX_TITLE: usize = 200;
const MAX_DESC: usize = 1000;
const MAX_TEXT: usize = 2000;
const MAX_IDS: usize = 500;

/// What to search for. Empty `query` lists everything (in path order) that passes the
/// filters.
#[derive(Debug, Clone, Default)]
pub struct SearchRequest {
    /// Iconarium query syntax: words (AND), "phrases", -exclude, globs, re:regex.
    pub query: String,
    /// Library ids, root paths or (unique) names; empty = all libraries.
    pub libraries: Vec<String>,
    /// Folder (path prefix) inside the libraries.
    pub folder: Option<String>,
    /// Every tag must be present.
    pub tags: Vec<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

/// An asset by id, or by library + relative path.
#[derive(Debug, Clone, PartialEq)]
pub enum IconRef {
    Id(AssetId),
    Path { library: String, path: String },
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DetailOptions {
    pub text: bool,
    pub svg: bool,
    pub duplicates: bool,
    /// Cap on returned SVG markup (bytes).
    pub max_svg_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct PreviewOptions {
    pub cell: u32,
    pub background: SheetBackground,
    pub labels: bool,
}

impl Default for PreviewOptions {
    fn default() -> Self {
        Self {
            cell: 128,
            background: SheetBackground::White,
            labels: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LegendEntry {
    /// 1-based cell number.
    pub cell: usize,
    pub id: AssetId,
    pub name: String,
    pub library: String,
    pub path: String,
    pub problem: Option<String>,
}

#[derive(Debug)]
pub struct Preview {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub legend: Vec<LegendEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Svg,
    Png,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportNaming {
    /// The file's own name.
    Original,
    /// A slug of the display name.
    Name,
}

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub dest: PathBuf,
    pub format: ExportFormat,
    /// PNG: longest side in px.
    pub size: u32,
    pub overwrite: bool,
    pub naming: ExportNaming,
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// Strip control characters from third-party text before it reaches a model.
fn clean(s: &str, max: usize) -> Option<String> {
    let t: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let t = t.trim();
    (!t.is_empty()).then(|| truncate_chars(t, max))
}

fn same_path(a: &str, b: &str) -> bool {
    let n = |s: &str| {
        let s = s.trim().replace('\\', "/");
        let s = s.trim_end_matches('/').to_string();
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    };
    n(a) == n(b)
}

/// Resolve library selectors (id, root path, or unique name). Empty → all libraries.
pub fn resolve_libraries<'a>(
    snap: &'a Snapshot,
    selectors: &[String],
) -> Result<Vec<&'a Arc<LibraryData>>> {
    if selectors.is_empty() {
        return Ok(snap.libraries.iter().collect());
    }
    let mut out: Vec<&Arc<LibraryData>> = Vec::new();
    for sel in selectors {
        let sel = sel.trim();
        let found: Vec<&Arc<LibraryData>> = if let Ok(id) = sel.parse::<i64>() {
            snap.library(id).into_iter().collect()
        } else {
            let by_path: Vec<_> = snap
                .libraries
                .iter()
                .filter(|l| same_path(&l.info.path, sel))
                .collect();
            if !by_path.is_empty() {
                by_path
            } else {
                snap.libraries
                    .iter()
                    .filter(|l| {
                        l.info.display_name.eq_ignore_ascii_case(sel)
                            || snap.library_name(l.info.id).eq_ignore_ascii_case(sel)
                    })
                    .collect()
            }
        };
        match found.len() {
            0 => {
                return Err(CatalogError::NotFound(format!(
                    "no library {sel:?}; known: {}",
                    snap.libraries
                        .iter()
                        .map(|l| format!("{} [id {}]", snap.library_name(l.info.id), l.info.id))
                        .collect::<Vec<_>>()
                        .join(", ")
                )))
            }
            1 => {
                if !out.iter().any(|l| l.info.id == found[0].info.id) {
                    out.push(found[0]);
                }
            }
            _ => {
                return Err(CatalogError::Invalid(format!(
                    "library name {sel:?} is ambiguous; use an id or root path: {}",
                    found
                        .iter()
                        .map(|l| format!("{} [id {}]", l.info.path, l.info.id))
                        .collect::<Vec<_>>()
                        .join(", ")
                )))
            }
        }
    }
    Ok(out)
}

fn parse_cursor(cursor: &str, generation: u64) -> Result<usize> {
    let bad = || CatalogError::Invalid(format!("invalid cursor {cursor:?}"));
    let rest = cursor.strip_prefix('g').ok_or_else(bad)?;
    let (g, off) = rest.split_once(':').ok_or_else(bad)?;
    let g: u64 = g.parse().map_err(|_| bad())?;
    if g != generation {
        return Err(CatalogError::StaleCursor);
    }
    off.parse().map_err(|_| bad())
}

fn state_str(s: ProcessingState) -> String {
    s.as_str().to_string()
}

fn hit(snap: &Snapshot, lib: &LibraryData, rec: &AssetRecord, query: &str) -> SearchHit {
    let matched = if query.trim().is_empty() {
        None
    } else {
        lib.catalog.describe_match(rec.id, query).map(|m| MatchDto {
            field: format!("{:?}", m.field)
                .to_lowercase()
                .replace("idclass", "id_class"),
            snippet: clean(&m.snippet, 160),
        })
    };
    SearchHit {
        id: rec.id,
        name: lib.asset_name(rec),
        file: rec.filename.clone(),
        library: snap.library_name(lib.info.id).to_string(),
        library_id: lib.info.id,
        path: rec.relative_path.clone(),
        abs_path: lib
            .absolute(&rec.relative_path)
            .to_string_lossy()
            .into_owned(),
        tags: lib.tag_names(rec.id),
        width: rec.width,
        height: rec.height,
        matched,
        state: state_str(rec.state),
    }
}

fn coverage_warnings(
    snap: &Snapshot,
    libs: &[&Arc<LibraryData>],
    content_query: bool,
) -> Vec<String> {
    let mut w = Vec::new();
    for l in libs {
        let name = snap.library_name(l.info.id);
        if !l.available {
            w.push(format!(
                "library '{name}' is not reachable ({})",
                l.info.path
            ));
        }
        if content_query && l.stats.assets > 0 && l.stats.analyzed < l.stats.assets {
            let pct = l.stats.analyzed * 100 / l.stats.assets;
            w.push(format!(
                "library '{name}': content (titles, text) indexed for {pct}% of files; open it in Iconarium to finish — filename and path matches are complete"
            ));
        }
        for m in &l.warnings {
            w.push(format!("library '{name}': {m}"));
        }
    }
    w
}

impl IndexReader {
    pub fn list_libraries(&self) -> Vec<LibraryDto> {
        let snap = self.snapshot();
        snap.libraries
            .iter()
            .map(|l| LibraryDto {
                id: l.info.id,
                name: snap.library_name(l.info.id).to_string(),
                root: l.info.path.clone(),
                assets: l.stats.assets,
                analyzed: l.stats.analyzed,
                last_scan: l.info.last_scan,
                available: l.available,
                metadata_file: l
                    .metadata
                    .as_ref()
                    .map(|m| m.path.to_string_lossy().into_owned()),
                warnings: l.warnings.clone(),
            })
            .collect()
    }

    pub fn search(&self, req: &SearchRequest) -> Result<SearchResponse> {
        let snap = self.snapshot();
        let libs = resolve_libraries(&snap, &req.libraries)?;
        let limit = req.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let offset = match &req.cursor {
            Some(c) if !c.is_empty() => parse_cursor(c, snap.generation)?,
            _ => 0,
        };
        let folder = req
            .folder
            .as_deref()
            .map(normalize_key)
            .filter(|f| !f.is_empty())
            .map(|f| format!("{}/", f.to_lowercase()));
        let tags: Vec<String> = req
            .tags
            .iter()
            .map(|t| {
                normalize_tag(t).ok_or_else(|| CatalogError::Invalid(format!("invalid tag {t:?}")))
            })
            .collect::<Result<_>>()?;

        // (score, library order, position in library, id, library index)
        let mut merged: Vec<(i64, usize, usize, AssetId)> = Vec::new();
        for (li, lib) in libs.iter().enumerate() {
            let hits = lib
                .catalog
                .search_scored(&req.query, None)
                .map_err(CatalogError::Core)?;
            for (pos, (id, score)) in hits.into_iter().enumerate() {
                if let Some(f) = &folder {
                    let Some(rec) = lib.catalog.get(id) else {
                        continue;
                    };
                    if !rec.relative_path.to_lowercase().starts_with(f.as_str()) {
                        continue;
                    }
                }
                if !tags.is_empty() {
                    let have = lib.tags.get(&id);
                    if !tags
                        .iter()
                        .all(|t| have.is_some_and(|h| h.iter().any(|(x, _)| x == t)))
                    {
                        continue;
                    }
                }
                merged.push((score, li, pos, id));
            }
        }
        merged.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let total = merged.len();
        let page: Vec<SearchHit> = merged
            .iter()
            .skip(offset)
            .take(limit)
            .filter_map(|&(_, li, _, id)| {
                let lib = libs[li];
                lib.catalog
                    .get(id)
                    .map(|rec| hit(&snap, lib, rec, &req.query))
            })
            .collect();
        let next = offset + page.len();
        let mut warnings = coverage_warnings(&snap, &libs, !req.query.trim().is_empty());
        if let Some(h) = self.health() {
            warnings.push(format!("index refresh failed, results may be stale: {h}"));
        }
        Ok(SearchResponse {
            total,
            returned: page.len(),
            next_cursor: (next < total).then(|| format!("g{}:{next}", snap.generation)),
            hits: page,
            warnings,
        })
    }

    /// Find an asset in a snapshot.
    pub fn resolve<'a>(
        &self,
        snap: &'a Snapshot,
        r: &IconRef,
    ) -> Result<(&'a Arc<LibraryData>, &'a AssetRecord)> {
        match r {
            IconRef::Id(id) => snap
                .find_asset(*id)
                .ok_or_else(|| CatalogError::NotFound(format!("no icon with id {id}"))),
            IconRef::Path { library, path } => {
                let libs = resolve_libraries(snap, std::slice::from_ref(library))?;
                let lib = libs[0];
                lib.asset_by_path(path)
                    .map(|rec| (lib, rec))
                    .ok_or_else(|| {
                        CatalogError::NotFound(format!(
                            "no icon {path:?} in library {}",
                            snap.library_name(lib.info.id)
                        ))
                    })
            }
        }
    }

    pub fn detail(&self, r: &IconRef, opts: DetailOptions) -> Result<IconDetail> {
        let snap = self.snapshot();
        let (lib, rec) = self.resolve(&snap, r)?;
        let abs = lib.absolute(&rec.relative_path);
        let text = lib.catalog.get_text(rec.id);
        let meta_asset = lib
            .metadata
            .as_ref()
            .and_then(|m| m.asset(&rec.relative_path));
        let mut d = IconDetail {
            id: rec.id,
            name: lib.asset_name(rec),
            file: rec.filename.clone(),
            library: snap.library_name(lib.info.id).to_string(),
            library_id: lib.info.id,
            path: rec.relative_path.clone(),
            abs_path: abs.to_string_lossy().into_owned(),
            exists: abs.is_file(),
            state: state_str(rec.state),
            problem: rec.parse_error.clone(),
            file_size: rec.file_size,
            modified: rec.mtime_ns.div_euclid(1_000_000_000),
            width: rec.width,
            height: rec.height,
            view_box: rec.view_box.map(|v| [v.min_x, v.min_y, v.width, v.height]),
            element_count: rec.element_count,
            content_hash: rec.content_hash.clone(),
            tags: lib
                .tags
                .get(&rec.id)
                .map(|v| {
                    v.iter()
                        .map(|(t, s)| TagDto {
                            tag: t.clone(),
                            source: *s,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            metadata_description: meta_asset
                .and_then(|a| a.description.as_deref())
                .and_then(|s| clean(s, MAX_DESC)),
            title: None,
            description: None,
            visible_text: None,
            identifiers: None,
            svg: None,
            svg_truncated: false,
            duplicates: None,
        };
        if opts.text {
            if let Some(t) = text {
                d.title = clean(&t.title, MAX_TITLE);
                d.description = clean(&t.description, MAX_DESC);
                d.visible_text = clean(&t.visible_text, MAX_TEXT);
                d.identifiers = clean(&t.identifiers, MAX_IDS);
            }
        }
        if opts.svg {
            let bytes = read_svg(&snap, lib, rec)?;
            let max = if opts.max_svg_bytes == 0 {
                20_000
            } else {
                opts.max_svg_bytes
            };
            let s = String::from_utf8_lossy(&bytes);
            if s.len() > max {
                let mut cut = max;
                while !s.is_char_boundary(cut) {
                    cut -= 1;
                }
                d.svg = Some(s[..cut].to_string());
                d.svg_truncated = true;
            } else {
                d.svg = Some(s.into_owned());
            }
        }
        if opts.duplicates {
            let mut dups = Vec::new();
            if let Some(h) = &rec.content_hash {
                for other in self.db()?.assets_with_hash(h)? {
                    if other.id == rec.id {
                        continue;
                    }
                    let library = snap
                        .library(other.library_id)
                        .map(|l| snap.library_name(l.info.id).to_string())
                        .unwrap_or_default();
                    dups.push(DuplicateDto {
                        id: other.id,
                        library,
                        path: other.relative_path,
                    });
                }
            }
            d.duplicates = Some(dups);
        }
        Ok(d)
    }

    pub fn list_tags(
        &self,
        libraries: &[String],
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<TagList> {
        let snap = self.snapshot();
        let libs = resolve_libraries(&snap, libraries)?;
        let prefix = prefix
            .map(|p| p.trim().to_lowercase())
            .filter(|p| !p.is_empty());
        let mut agg: HashMap<&str, (usize, BTreeSet<crate::tags::TagSource>)> = HashMap::new();
        for lib in &libs {
            for tags in lib.tags.values() {
                for (t, s) in tags {
                    if prefix.as_deref().is_some_and(|p| !t.starts_with(p)) {
                        continue;
                    }
                    let e = agg.entry(t.as_str()).or_default();
                    e.0 += 1;
                    e.1.insert(*s);
                }
            }
        }
        let total = agg.len();
        let mut tags: Vec<TagCount> = agg
            .into_iter()
            .map(|(t, (count, sources))| TagCount {
                tag: t.to_string(),
                count,
                sources: sources.into_iter().collect(),
            })
            .collect();
        tags.sort_by(|a, b| b.count.cmp(&a.count).then(a.tag.cmp(&b.tag)));
        tags.truncate(limit.max(1));
        Ok(TagList { tags, total })
    }

    pub fn list_folders(
        &self,
        library: Option<&str>,
        folder: Option<&str>,
        max_files: usize,
    ) -> Result<FolderListing> {
        let snap = self.snapshot();
        let lib = match library {
            Some(sel) => resolve_libraries(&snap, &[sel.to_string()])?[0],
            None if snap.libraries.len() == 1 => &snap.libraries[0],
            None => {
                return Err(CatalogError::Invalid(
                    "several libraries are indexed; say which one (id or root path)".into(),
                ))
            }
        };
        let folder = folder.map(normalize_key).unwrap_or_default();
        let prefix = if folder.is_empty() {
            String::new()
        } else {
            format!("{}/", folder.to_lowercase())
        };
        let folder_depth = if folder.is_empty() {
            0
        } else {
            folder.split('/').count()
        };
        let mut sub: HashMap<String, (String, usize)> = HashMap::new();
        let mut files: Vec<FileEntry> = Vec::new();
        let mut any = false;
        for id in lib.catalog.ids() {
            let Some(rec) = lib.catalog.get(id) else {
                continue;
            };
            let lc = rec.relative_path.to_lowercase();
            if !lc.starts_with(&prefix) {
                continue;
            }
            any = true;
            // Drop the folder's segments (not bytes of a lower-cased copy, whose length
            // can differ from the original).
            let rest = match folder_depth {
                0 => rec.relative_path.as_str(),
                n => rec
                    .relative_path
                    .splitn(n + 1, '/')
                    .nth(n)
                    .unwrap_or_default(),
            };
            match rest.split_once('/') {
                Some((dir, _)) => {
                    let e = sub
                        .entry(dir.to_lowercase())
                        .or_insert((dir.to_string(), 0));
                    e.1 += 1;
                }
                None => files.push(FileEntry {
                    id,
                    file: rec.filename.clone(),
                    name: lib.asset_name(rec),
                }),
            }
        }
        if !any && !folder.is_empty() {
            return Err(CatalogError::NotFound(format!(
                "no folder {folder:?} in library {}",
                snap.library_name(lib.info.id)
            )));
        }
        let mut folders: Vec<FolderEntry> = sub
            .into_values()
            .map(|(name, assets)| FolderEntry {
                path: if folder.is_empty() {
                    name.clone()
                } else {
                    format!("{folder}/{name}")
                },
                name,
                assets,
            })
            .collect();
        folders.sort_by_key(|f| f.name.to_lowercase());
        files.sort_by_key(|f| f.file.to_lowercase());
        let more_files = files.len().saturating_sub(max_files);
        files.truncate(max_files);
        Ok(FolderListing {
            library: snap.library_name(lib.info.id).to_string(),
            library_id: lib.info.id,
            folder,
            folders,
            files,
            more_files,
        })
    }

    /// Render a contact sheet of `ids` (1–48).
    pub fn preview(&self, ids: &[AssetId], opts: PreviewOptions) -> Result<Preview> {
        if ids.is_empty() || ids.len() > sheet::MAX_CELLS {
            return Err(CatalogError::Invalid(format!(
                "preview takes 1–{} icons (got {})",
                sheet::MAX_CELLS,
                ids.len()
            )));
        }
        let snap = self.snapshot();
        let mut cells = Vec::with_capacity(ids.len());
        let mut legend = Vec::with_capacity(ids.len());
        for (i, id) in ids.iter().enumerate() {
            let Ok((lib, rec)) = self.resolve(&snap, &IconRef::Id(*id)) else {
                cells.push(SheetCell {
                    label: format!("#{id}?"),
                    svg: Err(format!("no icon with id {id}")),
                    resources_dir: None,
                });
                legend.push(LegendEntry {
                    cell: i + 1,
                    id: *id,
                    name: String::new(),
                    library: String::new(),
                    path: String::new(),
                    problem: None,
                });
                continue;
            };
            let name = lib.asset_name(rec);
            let abs = lib.absolute(&rec.relative_path);
            cells.push(SheetCell {
                label: name.clone(),
                svg: read_svg(&snap, lib, rec).map_err(|e| e.to_string()),
                resources_dir: abs.parent().map(Path::to_path_buf),
            });
            legend.push(LegendEntry {
                cell: i + 1,
                id: *id,
                name,
                library: snap.library_name(lib.info.id).to_string(),
                path: rec.relative_path.clone(),
                problem: None,
            });
        }
        let out = sheet::render_sheet(
            cells,
            opts.cell,
            opts.background,
            opts.labels,
            &snap.settings.limits,
        )?;
        for (l, p) in legend.iter_mut().zip(out.problems) {
            l.problem = p;
        }
        Ok(Preview {
            png: out.png,
            width: out.width,
            height: out.height,
            legend,
        })
    }

    /// Copy (SVG) or render (PNG) icons into `opts.dest`. With `allowed_roots`, the
    /// destination must be inside one of them.
    pub fn export(
        &self,
        ids: &[AssetId],
        opts: &ExportOptions,
        allowed_roots: Option<&[PathBuf]>,
    ) -> Result<ExportResult> {
        let dest = check_destination(&opts.dest, allowed_roots)?;
        std::fs::create_dir_all(&dest)?;
        let snap = self.snapshot();
        let mut used: BTreeSet<String> = BTreeSet::new();
        let mut result = ExportResult {
            written: Vec::new(),
            skipped: Vec::new(),
        };
        for id in ids {
            let (lib, rec) = match self.resolve(&snap, &IconRef::Id(*id)) {
                Ok(x) => x,
                Err(e) => {
                    result.skipped.push(SkippedFile {
                        id: *id,
                        reason: e.to_string(),
                    });
                    continue;
                }
            };
            let ext = match opts.format {
                ExportFormat::Svg => "svg",
                ExportFormat::Png => "png",
            };
            let stem = match opts.naming {
                ExportNaming::Original => {
                    let f = &rec.filename;
                    f.rfind('.').map(|i| &f[..i]).unwrap_or(f).to_string()
                }
                ExportNaming::Name => slug(&lib.asset_name(rec)),
            };
            let stem = sanitize_file_stem(&stem);
            let mut name = format!("{stem}.{ext}");
            if !used.insert(name.to_lowercase()) {
                name = format!("{stem}-{id}.{ext}");
                used.insert(name.to_lowercase());
            }
            let target = dest.join(&name);
            if target.exists() && !opts.overwrite {
                result.skipped.push(SkippedFile {
                    id: *id,
                    reason: format!(
                        "{} exists (set overwrite to replace it)",
                        display_path(&target)
                    ),
                });
                continue;
            }
            let bytes = match opts.format {
                ExportFormat::Svg => std::fs::read(lib.absolute(&rec.relative_path)).map_err(|e| {
                    CatalogError::Unavailable(format!("cannot read {}: {e}", rec.relative_path))
                }),
                ExportFormat::Png => read_svg(&snap, lib, rec).and_then(|svg| {
                    let abs = lib.absolute(&rec.relative_path);
                    let dir = abs.parent().map(Path::to_path_buf);
                    let limits = snap.settings.limits.clone();
                    let size = opts.size.clamp(16, 4096);
                    sheet::with_render_stack(move || {
                        render_thumbnail_pixmap(&svg, size, &limits, dir.as_deref()).and_then(
                            |pm| {
                                pm.encode_png().map_err(|e| {
                                    svg_core::CoreError::Render(format!("PNG encoding failed: {e}"))
                                })
                            },
                        )
                    })
                    .map_err(|e| CatalogError::Unavailable(e.to_string()))
                }),
            };
            let written = bytes.and_then(|b| {
                if opts.overwrite {
                    write_atomic(&target, &b)
                } else {
                    write_new(&target, &b)
                }
            });
            match written {
                Ok(()) => result.written.push(ExportedFile {
                    id: *id,
                    path: display_path(&target),
                }),
                Err(CatalogError::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    result.skipped.push(SkippedFile {
                        id: *id,
                        reason: format!(
                            "{} exists (set overwrite to replace it)",
                            display_path(&target)
                        ),
                    })
                }
                Err(e) => result.skipped.push(SkippedFile {
                    id: *id,
                    reason: e.to_string(),
                }),
            }
        }
        Ok(result)
    }

    /// Add/remove tags on assets via their libraries' iconarium.yaml, then refresh.
    pub fn update_tags(
        &self,
        ids: &[AssetId],
        add: &[String],
        remove: &[String],
    ) -> Result<TagUpdateResult> {
        if add.is_empty() && remove.is_empty() {
            return Err(CatalogError::Invalid(
                "nothing to change: give tags to add or remove".into(),
            ));
        }
        let snap = self.snapshot();
        let mut by_lib: Vec<LibraryAssets> = Vec::new();
        for id in ids {
            let (lib, rec) = self.resolve(&snap, &IconRef::Id(*id))?;
            if !lib.available {
                return Err(CatalogError::Unavailable(format!(
                    "library folder {} is not reachable",
                    lib.info.path
                )));
            }
            match by_lib.iter_mut().find(|(l, _)| l.info.id == lib.info.id) {
                Some((_, v)) => v.push((*id, rec.relative_path.clone())),
                None => by_lib.push((lib.clone(), vec![(*id, rec.relative_path.clone())])),
            }
        }
        let mut out = TagUpdateResult {
            updated: Vec::new(),
            files: Vec::new(),
        };
        for (lib, assets) in by_lib {
            let paths: Vec<String> = assets.iter().map(|(_, p)| p.clone()).collect();
            let res = metadata::update_tags(&lib.root, &paths, add, remove)?;
            out.files.push(
                lib.root
                    .join(metadata::METADATA_FILE)
                    .to_string_lossy()
                    .into_owned(),
            );
            for ((id, path), (_, tags)) in assets.into_iter().zip(res) {
                out.updated.push(TaggedAsset {
                    id,
                    path,
                    metadata_tags: tags,
                });
            }
        }
        // The files are written; a failed refresh only delays visibility.
        if let Err(e) = self.refresh_now() {
            tracing::warn!("refresh after tag update failed: {e}");
        }
        Ok(out)
    }
}

/// Read an asset's SVG for rendering, honouring its state and the size limit.
pub fn read_svg(snap: &Snapshot, lib: &LibraryData, rec: &AssetRecord) -> Result<Vec<u8>> {
    match rec.state {
        ProcessingState::LimitExceeded => {
            return Err(CatalogError::Unavailable(format!(
                "{} exceeds the complexity limits{}",
                rec.relative_path,
                rec.parse_error
                    .as_deref()
                    .map(|e| format!(": {e}"))
                    .unwrap_or_default()
            )))
        }
        ProcessingState::ParseError => {
            return Err(CatalogError::Unavailable(format!(
                "{} is not a valid SVG{}",
                rec.relative_path,
                rec.parse_error
                    .as_deref()
                    .map(|e| format!(": {e}"))
                    .unwrap_or_default()
            )))
        }
        _ => {}
    }
    let abs = lib.absolute(&rec.relative_path);
    let len = std::fs::metadata(&abs)
        .map_err(|_| CatalogError::Unavailable(format!("{} is missing on disk", abs.display())))?
        .len();
    if len > snap.settings.limits.max_file_bytes {
        return Err(CatalogError::Unavailable(format!(
            "{} is larger than the processing limit",
            rec.relative_path
        )));
    }
    Ok(std::fs::read(&abs)?)
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "icon".into()
    } else {
        out
    }
}

fn sanitize_file_stem(s: &str) -> String {
    const RESERVED: [&str; 22] = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let mut t: String = s
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    t = t.trim_end_matches(['.', ' ']).to_string();
    if t.is_empty() {
        t = "icon".into();
    }
    let base = t.split('.').next().unwrap_or_default().to_lowercase();
    if RESERVED.contains(&base.as_str()) {
        t.insert(0, '_');
    }
    t.chars().take(120).collect()
}

/// Canonical destination, checked against `allowed` (when given). The destination may
/// not exist yet; its nearest existing ancestor is what gets checked.
pub fn check_destination(dest: &Path, allowed: Option<&[PathBuf]>) -> Result<PathBuf> {
    if !dest.is_absolute() {
        return Err(CatalogError::Invalid(format!(
            "destination must be an absolute path: {}",
            dest.display()
        )));
    }
    if dest
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(CatalogError::Invalid(
            "destination must not contain '..'".into(),
        ));
    }
    let Some(allowed) = allowed else {
        return Ok(dest.to_path_buf());
    };
    // Canonicalize the nearest existing ancestor, then re-append the rest.
    let mut existing = dest.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_os_string());
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut canon = existing.canonicalize()?;
    for t in tail.iter().rev() {
        canon.push(t);
    }
    let ok = allowed.iter().any(|root| {
        root.canonicalize()
            .map(|r| canon.starts_with(&r))
            .unwrap_or(false)
    });
    if ok {
        Ok(canon)
    } else {
        Err(CatalogError::Denied(format!(
            "{} is outside the folders this server may write to ({})",
            dest.display(),
            allowed
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }
}

static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A temp name next to `target` that can't collide with real files or other writers.
fn temp_path(target: &Path) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.{}-{n}.iconarium-tmp", std::process::id()))
}

/// Replace `target` atomically (temp file + rename).
fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = temp_path(target);
    std::fs::write(&tmp, bytes)?;
    if let Err(e) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// Create `target` only if it doesn't exist (`AlreadyExists` otherwise), so a file that
/// appears concurrently is never clobbered. A partial file is removed on failure.
fn write_new(target: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    if let Err(e) = f.write_all(bytes).and_then(|()| f.sync_all()) {
        drop(f);
        let _ = std::fs::remove_file(target);
        return Err(e.into());
    }
    Ok(())
}

/// Path for display: without Windows' verbatim `\\?\` prefix on drive paths.
fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
        _ => s.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trip_and_staleness() {
        assert_eq!(parse_cursor("g7:40", 7).unwrap(), 40);
        assert!(matches!(
            parse_cursor("g6:40", 7),
            Err(CatalogError::StaleCursor)
        ));
        assert!(matches!(
            parse_cursor("40", 7),
            Err(CatalogError::Invalid(_))
        ));
    }

    #[test]
    fn names_for_export() {
        assert_eq!(slug("Edge Router (v2)"), "edge-router-v2");
        assert_eq!(slug("***"), "icon");
        assert_eq!(sanitize_file_stem("a:b?c"), "a_b_c");
        assert_eq!(sanitize_file_stem("CON"), "_CON");
        assert_eq!(sanitize_file_stem("con.backup"), "_con.backup");
        assert_eq!(sanitize_file_stem("x. "), "x");
    }

    #[test]
    fn destination_checks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("allowed");
        std::fs::create_dir_all(&root).unwrap();
        let roots = vec![root.clone()];
        assert!(check_destination(&root.join("new/sub"), Some(&roots)).is_ok());
        assert!(matches!(
            check_destination(&dir.path().join("elsewhere"), Some(&roots)),
            Err(CatalogError::Denied(_))
        ));
        assert!(check_destination(&root.join("../escape"), Some(&roots)).is_err());
        assert!(check_destination(Path::new("relative"), None).is_err());
    }
}
