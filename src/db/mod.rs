pub mod events;
pub mod sqlite;

use std::path::PathBuf;

/// Unified database handle for reclaude storage.
///
/// Wraps EventStore (events + vectors) and MetadataDb (sessions + metadata),
/// both backed by the same SQLite file (`~/.reclaude/metadata.db`).
pub struct Database {
    pub events: events::EventStore,
    pub meta: sqlite::MetadataDb,
}

impl Database {
    /// Open or create the database at `~/.reclaude/`.
    pub async fn open() -> anyhow::Result<Self> {
        let base = base_dir();
        std::fs::create_dir_all(&base)?;

        let events = events::EventStore::open(&base)?;
        let meta = sqlite::MetadataDb::open(&base)?;
        Ok(Self { events, meta })
    }
}

/// Base storage directory: `~/.reclaude/`.
pub fn base_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join(".reclaude")
}
