pub mod events;
pub mod sessions;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

/// Unified SQLite-backed storage for reclaude.
///
/// Single `Mutex<Connection>` to `~/.reclaude/metadata.db` (WAL mode).
/// Events, sessions, transcripts, tags, and vectors all in one file.
/// `Mutex` makes this `Send + Sync` for use in `Arc` (web server).
pub struct Database {
    pub(crate) conn: Mutex<Connection>,
}

impl Drop for Database {
    fn drop(&mut self) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute_batch("PRAGMA optimize");
        }
    }
}

/// Register sqlite-vec extension globally (once per process).
fn init_sqlite_vec() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }
    });
}

impl Database {
    /// Open or create the database at `~/.reclaude/`.
    pub async fn open() -> anyhow::Result<Self> {
        let base = base_dir();
        std::fs::create_dir_all(&base)?;
        Self::open_at(&base)
    }

    /// Open or create the database at a specific base directory.
    pub fn open_at(base: &Path) -> anyhow::Result<Self> {
        init_sqlite_vec();

        let db_path = base.join("metadata.db");
        let conn = Connection::open(&db_path)?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA synchronous = normal;
             PRAGMA mmap_size = 268435456;
             PRAGMA cache_size = -16000;",
        )?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.ensure_schema()?;

        // Verify sqlite-vec is loaded
        {
            let conn = db.conn.lock().unwrap();
            let _ver: String =
                conn.query_row("SELECT vec_version()", [], |r| r.get(0))?;
        }

        Ok(db)
    }

    fn ensure_schema(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();

        // Events table, indexes, FTS5, and sync triggers
        events::create_schema(&conn)?;

        // Sessions, transcripts, tags, scan state, focus snapshots
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                session_id   TEXT PRIMARY KEY,
                started_at   TEXT NOT NULL,
                ended_at     TEXT,
                cwd          TEXT,
                repo_name    TEXT,
                remote_url   TEXT,
                branch       TEXT,
                event_count  INTEGER DEFAULT 0,
                is_active    INTEGER DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS session_transcripts (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id        TEXT NOT NULL UNIQUE,
                parent_session_id TEXT,
                archived_at       TEXT NOT NULL,
                transcript_path   TEXT,
                size_bytes        INTEGER NOT NULL,
                compressed_bytes  INTEGER,
                content           BLOB NOT NULL,
                metadata          TEXT
            );

            CREATE TABLE IF NOT EXISTS session_scan_state (
                session_id       TEXT PRIMARY KEY,
                last_byte_offset INTEGER DEFAULT 0,
                last_uuid        TEXT,
                last_scan_time   TEXT,
                transcript_path  TEXT
            );

            CREATE TABLE IF NOT EXISTS transcript_scan_state (
                transcript_path    TEXT PRIMARY KEY,
                last_byte_offset   INTEGER DEFAULT 0,
                last_entry_uuid    TEXT,
                last_scan_time     TEXT,
                entries_extracted   INTEGER DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS session_tags (
                tag        TEXT PRIMARY KEY,
                session_id TEXT,
                created_at TEXT NOT NULL,
                cwd        TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS focus_snapshots (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp      TEXT NOT NULL,
                project        TEXT,
                time_scale     TEXT NOT NULL,
                period_start   TEXT NOT NULL,
                period_end     TEXT NOT NULL,
                focus_summary  TEXT NOT NULL,
                top_topics     TEXT NOT NULL,
                event_count    INTEGER NOT NULL,
                metadata       TEXT NOT NULL DEFAULT '{}'
            );",
        )?;
        Ok(())
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

/// Base storage directory: `~/.reclaude/`.
pub fn base_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join(".reclaude")
}
