//! Versioned schema migrations, tracked with `PRAGMA user_version`.
//!
//! Each entry in [`MIGRATIONS`] upgrades the schema from version `i` to `i + 1`.
//! Running [`migrate`] on an up-to-date database is a no-op, so it is safe to call on
//! every open. Never edit a released migration; append a new one.

use rusqlite::Connection;

use crate::error::{CoreError, Result};

/// Ordered list of migration scripts. `MIGRATIONS[0]` creates schema version 1.
pub const MIGRATIONS: &[&str] = &[
    // ---- v1: plan §10 tables ----
    r#"
    CREATE TABLE libraries (
        id            INTEGER PRIMARY KEY,
        path          TEXT NOT NULL UNIQUE,
        display_name  TEXT NOT NULL,
        created_at    INTEGER NOT NULL,
        last_opened   INTEGER NOT NULL,
        last_scan     INTEGER,
        open_seq      INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE assets (
        id               INTEGER PRIMARY KEY,
        library_id       INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
        relative_path    TEXT NOT NULL,
        filename         TEXT NOT NULL,
        file_size        INTEGER NOT NULL,
        mtime_ns         INTEGER NOT NULL,
        fast_fingerprint TEXT NOT NULL,
        content_hash     TEXT,
        width            REAL,
        height           REAL,
        vb_x             REAL,
        vb_y             REAL,
        vb_w             REAL,
        vb_h             REAL,
        element_count    INTEGER,
        processing_state TEXT NOT NULL DEFAULT 'discovered',
        parse_error      TEXT,
        UNIQUE (library_id, relative_path)
    );
    CREATE INDEX idx_assets_state ON assets(library_id, processing_state);
    CREATE INDEX idx_assets_hash ON assets(content_hash) WHERE content_hash IS NOT NULL;

    CREATE TABLE svg_metadata (
        asset_id              INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
        doc_x                 REAL,
        doc_y                 REAL,
        doc_w                 REAL,
        doc_h                 REAL,
        node_count            INTEGER NOT NULL DEFAULT 0,
        embedded_raster_bytes INTEGER NOT NULL DEFAULT 0,
        max_text_node_chars   INTEGER NOT NULL DEFAULT 0,
        has_scripts           INTEGER NOT NULL DEFAULT 0,
        has_external_refs     INTEGER NOT NULL DEFAULT 0,
        analyzed_at           INTEGER NOT NULL
    );

    CREATE TABLE svg_text (
        asset_id         INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
        title_text       TEXT NOT NULL DEFAULT '',
        description_text TEXT NOT NULL DEFAULT '',
        visible_text     TEXT NOT NULL DEFAULT '',
        identifier_text  TEXT NOT NULL DEFAULT ''
    );

    CREATE TABLE thumbnails (
        asset_id   INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
        cache_key  TEXT NOT NULL,
        size       INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        PRIMARY KEY (asset_id, size)
    );

    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    "#,
];

/// Latest schema version this build understands.
pub fn latest_version() -> u32 {
    MIGRATIONS.len() as u32
}

/// Current `PRAGMA user_version`.
pub fn current_version(conn: &Connection) -> Result<u32> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as u32)
}

/// Apply all pending migrations, each in its own transaction. Idempotent.
/// Refuses to open a database created by a newer build.
pub fn migrate(conn: &mut Connection) -> Result<u32> {
    let mut version = current_version(conn)?;
    let latest = latest_version();
    if version > latest {
        return Err(CoreError::Other(format!(
            "index database schema v{version} is newer than this build (v{latest})"
        )));
    }
    while version < latest {
        let tx = conn.transaction()?;
        tx.execute_batch(MIGRATIONS[version as usize])?;
        version += 1;
        tx.pragma_update(None, "user_version", version as i64)?;
        tx.commit()?;
        tracing::info!(version, "index schema migrated");
    }
    Ok(version)
}
