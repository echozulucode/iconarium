//! Persistent SQLite index (plan §10). One [`Database`] wraps one connection; callers
//! share it behind a mutex. All batch operations run inside a single transaction and
//! use cached prepared statements.

use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::config::Settings;
use crate::error::Result;
use crate::library::fingerprint::fast_fingerprint;
use crate::library::scanner::ExistingEntry;
use crate::model::{
    Analysis, AssetId, AssetRecord, DiscoveredFile, LibraryId, LibraryInfo, ProcessingState,
    SearchText, ViewBox,
};

use super::migrations;

const SETTINGS_KEY: &str = "app";

/// Columns selected for an [`AssetRecord`]; must match [`asset_from_row`].
const ASSET_COLS: &str = "id, library_id, relative_path, filename, file_size, mtime_ns, \
     fast_fingerprint, content_hash, width, height, vb_x, vb_y, vb_w, vb_h, \
     element_count, processing_state, parse_error";
const ASSET_COL_COUNT: usize = 17;

const LIBRARY_COLS: &str = "id, path, display_name, created_at, last_opened, last_scan";

/// Current time in Unix seconds.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn view_box(x: Option<f64>, y: Option<f64>, w: Option<f64>, h: Option<f64>) -> Option<ViewBox> {
    match (x, y, w, h) {
        (Some(min_x), Some(min_y), Some(width), Some(height)) => Some(ViewBox {
            min_x,
            min_y,
            width,
            height,
        }),
        _ => None,
    }
}

fn asset_from_row(r: &Row<'_>) -> rusqlite::Result<AssetRecord> {
    let state: String = r.get(15)?;
    Ok(AssetRecord {
        id: r.get::<_, i64>(0)? as AssetId,
        library_id: r.get(1)?,
        relative_path: r.get(2)?,
        filename: r.get(3)?,
        file_size: r.get::<_, i64>(4)? as u64,
        mtime_ns: r.get(5)?,
        fast_fingerprint: r.get(6)?,
        content_hash: r.get(7)?,
        width: r.get(8)?,
        height: r.get(9)?,
        view_box: view_box(r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?),
        element_count: r.get::<_, Option<i64>>(14)?.map(|v| v as u32),
        state: ProcessingState::parse(&state),
        parse_error: r.get(16)?,
    })
}

fn library_from_row(r: &Row<'_>) -> rusqlite::Result<LibraryInfo> {
    Ok(LibraryInfo {
        id: r.get(0)?,
        path: r.get(1)?,
        display_name: r.get(2)?,
        created_at: r.get(3)?,
        last_opened: r.get(4)?,
        last_scan: r.get(5)?,
    })
}

fn text_from_cols(r: &Row<'_>, first: usize) -> rusqlite::Result<Option<SearchText>> {
    let title: Option<String> = r.get(first)?;
    let Some(title) = title else { return Ok(None) };
    Ok(Some(SearchText {
        title,
        description: r.get(first + 1)?,
        visible_text: r.get(first + 2)?,
        identifiers: r.get(first + 3)?,
    }))
}

/// The persistent index.
pub struct Database {
    conn: Connection,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.conn.path())
            .finish()
    }
}

impl Database {
    /// Open (creating if needed) the index at `path` and apply migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        Self::init(Connection::open(path)?)
    }

    /// Private in-memory index (tests, benchmarks).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        // journal_mode returns a row; in-memory databases report "memory".
        let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA temp_store = MEMORY;
             PRAGMA busy_timeout = 5000;",
        )?;
        conn.set_prepared_statement_cache_capacity(64);
        migrations::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// Schema version after migrations.
    pub fn schema_version(&self) -> Result<u32> {
        migrations::current_version(&self.conn)
    }

    // ------------------------------------------------------------------ libraries

    /// Insert or touch a library root; sets `last_opened = now` and moves it to the
    /// front of [`Database::recent_libraries`].
    pub fn upsert_library(&mut self, path: &str, display_name: &str) -> Result<LibraryInfo> {
        let now = unix_now();
        let sql = format!(
            "INSERT INTO libraries (path, display_name, created_at, last_opened, open_seq)
             VALUES (?1, ?2, ?3, ?3, (SELECT COALESCE(MAX(open_seq), 0) + 1 FROM libraries))
             ON CONFLICT(path) DO UPDATE SET
                display_name = excluded.display_name,
                last_opened  = excluded.last_opened,
                open_seq     = excluded.open_seq
             RETURNING {LIBRARY_COLS}"
        );
        let mut st = self.conn.prepare_cached(&sql)?;
        Ok(st.query_row(params![path, display_name, now], library_from_row)?)
    }

    pub fn get_library(&self, id: LibraryId) -> Result<Option<LibraryInfo>> {
        let sql = format!("SELECT {LIBRARY_COLS} FROM libraries WHERE id = ?1");
        let mut st = self.conn.prepare_cached(&sql)?;
        Ok(st.query_row([id], library_from_row).optional()?)
    }

    /// Most recently opened first.
    pub fn recent_libraries(&self, limit: usize) -> Result<Vec<LibraryInfo>> {
        let sql = format!(
            "SELECT {LIBRARY_COLS} FROM libraries ORDER BY open_seq DESC, last_opened DESC, id DESC LIMIT ?1"
        );
        let mut st = self.conn.prepare_cached(&sql)?;
        let rows = st.query_map([limit.min(i64::MAX as usize) as i64], library_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_library_scanned(&mut self, id: LibraryId, ts: i64) -> Result<()> {
        self.conn
            .prepare_cached("UPDATE libraries SET last_scan = ?2 WHERE id = ?1")?
            .execute(params![id, ts])?;
        Ok(())
    }

    /// Delete a library and (by cascade) all of its assets, metadata, text and thumbnail rows.
    pub fn remove_library(&mut self, id: LibraryId) -> Result<()> {
        self.conn
            .prepare_cached("DELETE FROM libraries WHERE id = ?1")?
            .execute([id])?;
        Ok(())
    }

    // ------------------------------------------------------------------ assets

    /// `relative_path → (id, size, mtime)` for reconciliation.
    pub fn library_snapshot(
        &self,
        library_id: LibraryId,
    ) -> Result<HashMap<String, ExistingEntry>> {
        let mut st = self.conn.prepare_cached(
            "SELECT relative_path, id, file_size, mtime_ns FROM assets WHERE library_id = ?1",
        )?;
        let mut rows = st.query([library_id])?;
        let mut map = HashMap::new();
        while let Some(r) = rows.next()? {
            map.insert(
                r.get::<_, String>(0)?,
                ExistingEntry {
                    id: r.get::<_, i64>(1)? as AssetId,
                    file_size: r.get::<_, i64>(2)? as u64,
                    mtime_ns: r.get(3)?,
                },
            );
        }
        Ok(map)
    }

    pub fn asset_count(&self, library_id: LibraryId) -> Result<usize> {
        let n: i64 = self
            .conn
            .prepare_cached("SELECT COUNT(*) FROM assets WHERE library_id = ?1")?
            .query_row([library_id], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Insert newly discovered files (state `Discovered`) in one transaction.
    /// A path that already exists is updated in place (keeps its id) and reset to
    /// `Discovered`, so racing watcher/scan inserts cannot violate uniqueness.
    pub fn insert_assets(
        &mut self,
        library_id: LibraryId,
        files: &[DiscoveredFile],
    ) -> Result<Vec<AssetRecord>> {
        let tx = self.conn.transaction()?;
        let mut out = Vec::with_capacity(files.len());
        {
            let mut st = tx.prepare_cached(
                "INSERT INTO assets (library_id, relative_path, filename, file_size, mtime_ns, fast_fingerprint, processing_state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'discovered')
                 ON CONFLICT(library_id, relative_path) DO UPDATE SET
                    filename = excluded.filename,
                    file_size = excluded.file_size,
                    mtime_ns = excluded.mtime_ns,
                    fast_fingerprint = excluded.fast_fingerprint,
                    content_hash = NULL, width = NULL, height = NULL,
                    vb_x = NULL, vb_y = NULL, vb_w = NULL, vb_h = NULL,
                    element_count = NULL, processing_state = 'discovered', parse_error = NULL
                 RETURNING id",
            )?;
            for f in files {
                let fp = fast_fingerprint(&f.relative_path, f.file_size, f.mtime_ns);
                let id: i64 = st.query_row(
                    params![
                        library_id,
                        f.relative_path,
                        f.filename,
                        f.file_size as i64,
                        f.mtime_ns,
                        fp
                    ],
                    |r| r.get(0),
                )?;
                out.push(AssetRecord {
                    id: id as AssetId,
                    library_id,
                    relative_path: f.relative_path.clone(),
                    filename: f.filename.clone(),
                    file_size: f.file_size,
                    mtime_ns: f.mtime_ns,
                    fast_fingerprint: fp,
                    content_hash: None,
                    width: None,
                    height: None,
                    view_box: None,
                    element_count: None,
                    state: ProcessingState::Discovered,
                    parse_error: None,
                });
            }
        }
        tx.commit()?;
        Ok(out)
    }

    /// Record new size/mtime for changed files, reset them to `Discovered` and drop
    /// their derived rows (metadata, text, thumbnails). Unknown ids are skipped.
    pub fn update_changed(
        &mut self,
        changed: &[(AssetId, DiscoveredFile)],
    ) -> Result<Vec<AssetRecord>> {
        let tx = self.conn.transaction()?;
        let mut out = Vec::with_capacity(changed.len());
        {
            let sql = format!(
                "UPDATE assets SET filename = ?2, file_size = ?3, mtime_ns = ?4, fast_fingerprint = ?5,
                    content_hash = NULL, width = NULL, height = NULL,
                    vb_x = NULL, vb_y = NULL, vb_w = NULL, vb_h = NULL,
                    element_count = NULL, processing_state = 'discovered', parse_error = NULL
                 WHERE id = ?1
                 RETURNING {ASSET_COLS}"
            );
            let mut up = tx.prepare_cached(&sql)?;
            let mut del_meta = tx.prepare_cached("DELETE FROM svg_metadata WHERE asset_id = ?1")?;
            let mut del_text = tx.prepare_cached("DELETE FROM svg_text WHERE asset_id = ?1")?;
            let mut del_thumb = tx.prepare_cached("DELETE FROM thumbnails WHERE asset_id = ?1")?;
            for (id, f) in changed {
                // The stored relative_path is authoritative for the fingerprint.
                let fp = fast_fingerprint(&f.relative_path, f.file_size, f.mtime_ns);
                let rec = up
                    .query_row(
                        params![*id as i64, f.filename, f.file_size as i64, f.mtime_ns, fp],
                        asset_from_row,
                    )
                    .optional()?;
                if let Some(rec) = rec {
                    del_meta.execute([*id as i64])?;
                    del_text.execute([*id as i64])?;
                    del_thumb.execute([*id as i64])?;
                    out.push(rec);
                }
            }
        }
        tx.commit()?;
        Ok(out)
    }

    /// Delete assets (cascades to metadata, text and thumbnail rows).
    pub fn remove_assets(&mut self, ids: &[AssetId]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut st = tx.prepare_cached("DELETE FROM assets WHERE id = ?1")?;
            for id in ids {
                st.execute([*id as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_asset(&self, id: AssetId) -> Result<Option<AssetRecord>> {
        let sql = format!("SELECT {ASSET_COLS} FROM assets WHERE id = ?1");
        let mut st = self.conn.prepare_cached(&sql)?;
        Ok(st.query_row([id as i64], asset_from_row).optional()?)
    }

    pub fn get_asset_by_path(
        &self,
        library_id: LibraryId,
        relative_path: &str,
    ) -> Result<Option<AssetRecord>> {
        let sql =
            format!("SELECT {ASSET_COLS} FROM assets WHERE library_id = ?1 AND relative_path = ?2");
        let mut st = self.conn.prepare_cached(&sql)?;
        Ok(st
            .query_row(params![library_id, relative_path], asset_from_row)
            .optional()?)
    }

    /// Assets still awaiting analysis (state `discovered`), by id.
    pub fn pending_analysis(&self, library_id: LibraryId) -> Result<Vec<AssetId>> {
        let mut st = self.conn.prepare_cached(
            "SELECT id FROM assets WHERE library_id = ?1 AND processing_state = 'discovered' ORDER BY id",
        )?;
        let rows = st.query_map([library_id], |r| r.get::<_, i64>(0).map(|v| v as AssetId))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ------------------------------------------------------------------ analysis

    /// Persist one analysis result.
    pub fn save_analysis(&mut self, id: AssetId, analysis: &Analysis) -> Result<()> {
        self.save_analyses_iter(std::iter::once((id, analysis)))
    }

    /// Persist many analysis results in one transaction. Ids that no longer exist
    /// (deleted while being analyzed) are skipped.
    pub fn save_analyses(&mut self, items: &[(AssetId, Analysis)]) -> Result<()> {
        self.save_analyses_iter(items.iter().map(|(id, a)| (*id, a)))
    }

    fn save_analyses_iter<'a>(
        &mut self,
        items: impl Iterator<Item = (AssetId, &'a Analysis)>,
    ) -> Result<()> {
        let now = unix_now();
        let tx = self.conn.transaction()?;
        {
            let mut up = tx.prepare_cached(
                "UPDATE assets SET content_hash = ?2, width = ?3, height = ?4,
                    vb_x = ?5, vb_y = ?6, vb_w = ?7, vb_h = ?8,
                    element_count = ?9, processing_state = ?10, parse_error = ?11
                 WHERE id = ?1",
            )?;
            let mut meta = tx.prepare_cached(
                "INSERT OR REPLACE INTO svg_metadata (asset_id, doc_x, doc_y, doc_w, doc_h, node_count,
                    embedded_raster_bytes, max_text_node_chars, has_scripts, has_external_refs, analyzed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            let mut text = tx.prepare_cached(
                "INSERT OR REPLACE INTO svg_text (asset_id, title_text, description_text, visible_text, identifier_text)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let mut del_text = tx.prepare_cached("DELETE FROM svg_text WHERE asset_id = ?1")?;

            for (id, a) in items {
                let id = id as i64;
                let m = &a.meta;
                let vb = m.view_box;
                let element_count =
                    if a.state == ProcessingState::ParseError && m.element_count == 0 {
                        None
                    } else {
                        Some(m.element_count as i64)
                    };
                let hash = if a.content_hash.is_empty() {
                    None
                } else {
                    Some(a.content_hash.as_str())
                };
                let n = up.execute(params![
                    id,
                    hash,
                    m.width,
                    m.height,
                    vb.map(|v| v.min_x),
                    vb.map(|v| v.min_y),
                    vb.map(|v| v.width),
                    vb.map(|v| v.height),
                    element_count,
                    a.state.as_str(),
                    a.error,
                ])?;
                if n == 0 {
                    continue;
                }
                let db = m.doc_box;
                let c = &a.complexity;
                meta.execute(params![
                    id,
                    db.map(|v| v.min_x),
                    db.map(|v| v.min_y),
                    db.map(|v| v.width),
                    db.map(|v| v.height),
                    c.node_count as i64,
                    c.embedded_raster_bytes as i64,
                    c.max_text_node_chars as i64,
                    c.has_scripts,
                    c.has_external_refs,
                    now,
                ])?;
                if a.text.is_empty() {
                    del_text.execute([id])?;
                } else {
                    let t = &a.text;
                    text.execute(params![
                        id,
                        t.title,
                        t.description,
                        t.visible_text,
                        t.identifiers
                    ])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every asset of a library with its searchable text (if analyzed), for building
    /// the in-memory search catalog at startup. Single query, sequential row decode.
    pub fn load_catalog(
        &self,
        library_id: LibraryId,
    ) -> Result<Vec<(AssetRecord, Option<SearchText>)>> {
        let sql = format!(
            "SELECT {ASSET_COLS}, t.title_text, t.description_text, t.visible_text, t.identifier_text
             FROM assets a LEFT JOIN svg_text t ON t.asset_id = a.id
             WHERE a.library_id = ?1"
        );
        let mut st = self.conn.prepare_cached(&sql)?;
        let mut rows = st.query([library_id])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push((asset_from_row(r)?, text_from_cols(r, ASSET_COL_COUNT)?));
        }
        Ok(out)
    }

    pub fn get_search_text(&self, id: AssetId) -> Result<Option<SearchText>> {
        let mut st = self.conn.prepare_cached(
            "SELECT title_text, description_text, visible_text, identifier_text FROM svg_text WHERE asset_id = ?1",
        )?;
        Ok(st
            .query_row([id as i64], |r| text_from_cols(r, 0))
            .optional()?
            .flatten())
    }

    /// Effective document box (region coordinate system) from the last analysis.
    pub fn get_doc_box(&self, id: AssetId) -> Result<Option<ViewBox>> {
        let mut st = self.conn.prepare_cached(
            "SELECT doc_x, doc_y, doc_w, doc_h FROM svg_metadata WHERE asset_id = ?1",
        )?;
        Ok(st
            .query_row([id as i64], |r| {
                Ok(view_box(r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()?
            .flatten())
    }

    // ------------------------------------------------------------------ thumbnails

    pub fn record_thumbnail(
        &mut self,
        asset_id: AssetId,
        size: u32,
        cache_key: &str,
    ) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT OR REPLACE INTO thumbnails (asset_id, cache_key, size, created_at) VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![asset_id as i64, cache_key, size as i64, unix_now()])?;
        Ok(())
    }

    pub fn thumbnail_key(&self, asset_id: AssetId, size: u32) -> Result<Option<String>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT cache_key FROM thumbnails WHERE asset_id = ?1 AND size = ?2")?;
        Ok(st
            .query_row(params![asset_id as i64, size as i64], |r| r.get(0))
            .optional()?)
    }

    pub fn clear_thumbnails(&mut self, asset_id: AssetId) -> Result<()> {
        self.conn
            .prepare_cached("DELETE FROM thumbnails WHERE asset_id = ?1")?
            .execute([asset_id as i64])?;
        Ok(())
    }

    // ------------------------------------------------------------------ settings

    /// Stored settings (defaults when absent or unreadable), sanitized.
    pub fn load_settings(&self) -> Result<Settings> {
        let raw: Option<String> = self
            .conn
            .prepare_cached("SELECT value FROM settings WHERE key = ?1")?
            .query_row([SETTINGS_KEY], |r| r.get(0))
            .optional()?;
        let settings = match raw {
            None => Settings::default(),
            Some(json) => serde_json::from_str::<Settings>(&json).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "stored settings unreadable; using defaults");
                Settings::default()
            }),
        };
        Ok(settings.sanitized())
    }

    pub fn save_settings(&mut self, settings: &Settings) -> Result<()> {
        let json =
            serde_json::to_string(settings).map_err(|e| crate::CoreError::Other(e.to_string()))?;
        self.conn
            .prepare_cached("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)")?
            .execute(params![SETTINGS_KEY, json])?;
        Ok(())
    }
}
