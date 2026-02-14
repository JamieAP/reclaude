use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use zerocopy::IntoBytes;

use crate::models::Event;

/// SQLite-backed event storage with FTS5 text search and sqlite-vec vector search.
///
/// All events stored in `~/.reclaude/metadata.db` alongside session metadata.
/// FTS5 in external content mode with INSERT/DELETE/UPDATE triggers keeps
/// the full-text index always in sync - no manual rebuilds needed.
///
/// `Mutex<Connection>` makes this `Send + Sync` for use in `Arc<AppState>`.
pub struct EventStore {
    conn: Mutex<Connection>,
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

impl EventStore {
    /// Open or create the event store in `base/metadata.db`.
    pub fn open(base: &Path) -> anyhow::Result<Self> {
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

        let store = Self {
            conn: Mutex::new(conn),
        };
        store.ensure_schema()?;

        // Verify sqlite-vec is loaded
        {
            let conn = store.conn.lock().unwrap();
            let _ver: String =
                conn.query_row("SELECT vec_version()", [], |r| r.get(0))?;
        }

        Ok(store)
    }

    fn ensure_schema(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        create_schema(&conn)
    }

    /// Insert a single event, letting AUTOINCREMENT assign the ID.
    /// Returns the assigned row ID.
    pub async fn insert(&self, event: &Event) -> anyhow::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO events (timestamp, event_type, category, session_id,
                content, cwd, remote_url, repo_name, branch,
                tool_name, file_path, metadata_json, vector)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                event.timestamp,
                event.event_type,
                event.category,
                event.session_id,
                event.content,
                event.cwd,
                event.remote_url,
                event.repo_name,
                event.branch,
                event.tool_name,
                event.file_path,
                event.metadata_json,
                event.vector.as_ref().map(|v| v.as_bytes().to_vec()),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Bulk insert events with pre-assigned IDs (used by backfill).
    /// Wrapped in a transaction - rolls back on any error.
    pub async fn bulk_insert(&self, events: &[Event]) -> anyhow::Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("BEGIN")?;
        let result = (|| -> anyhow::Result<()> {
            let mut stmt = conn.prepare_cached(
                "INSERT OR REPLACE INTO events (id, timestamp, event_type, category, session_id,
                    content, cwd, remote_url, repo_name, branch,
                    tool_name, file_path, metadata_json, vector)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            )?;
            for event in events {
                stmt.execute(rusqlite::params![
                    event.id,
                    event.timestamp,
                    event.event_type,
                    event.category,
                    event.session_id,
                    event.content,
                    event.cwd,
                    event.remote_url,
                    event.repo_name,
                    event.branch,
                    event.tool_name,
                    event.file_path,
                    event.metadata_json,
                    event.vector.as_ref().map(|v| v.as_bytes().to_vec()),
                ])?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = conn.execute_batch("ROLLBACK");
            return result;
        }
        conn.execute_batch("COMMIT")?;
        Ok(())
    }

    /// Query events with filters, returned sorted by timestamp descending.
    pub async fn query(
        &self,
        event_types: &[&str],
        session_id: Option<&str>,
        cwd: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<Event>> {
        self.query_range(event_types, session_id, cwd, None, None, limit)
            .await
    }

    /// Query events with filters including optional time range.
    pub async fn query_range(
        &self,
        event_types: &[&str],
        session_id: Option<&str>,
        cwd: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<Event>> {
        let conn = self.conn.lock().unwrap();

        let mut sql = String::from(
            "SELECT id, timestamp, event_type, category, session_id,
                    content, cwd, remote_url, repo_name, branch,
                    tool_name, file_path, metadata_json
             FROM events WHERE 1=1",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        append_filters(&mut sql, &mut params, event_types, session_id, cwd);

        if let Some(s) = since {
            sql.push_str(" AND timestamp >= ?");
            params.push(Box::new(s.to_string()));
        }
        if let Some(u) = until {
            sql.push_str(" AND timestamp < ?");
            params.push(Box::new(u.to_string()));
        }

        sql.push_str(" ORDER BY timestamp DESC");
        if limit > 0 {
            sql.push_str(" LIMIT ?");
            params.push(Box::new(limit as i64));
        }

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            row_to_event,
        )?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    /// Full-text search over event content using FTS5.
    pub async fn search_fts(
        &self,
        query: &str,
        event_types: &[&str],
        session_id: Option<&str>,
        cwd: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<Event>> {
        let conn = self.conn.lock().unwrap();

        let mut sql = String::from(
            "SELECT es.id, es.timestamp, es.event_type, es.category, es.session_id,
                    es.content, es.cwd, es.remote_url, es.repo_name, es.branch,
                    es.tool_name, es.file_path, es.metadata_json
             FROM events_fts fts
             JOIN events es ON es.id = fts.rowid
             WHERE fts.content MATCH ?",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        params.push(Box::new(query.to_string()));

        append_filters_prefixed(&mut sql, &mut params, event_types, session_id, cwd, "es");

        sql.push_str(" ORDER BY fts.rank");
        if limit > 0 {
            sql.push_str(" LIMIT ?");
            params.push(Box::new(limit as i64));
        }

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            row_to_event,
        )?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    /// Semantic (vector) search - brute-force cosine distance via sqlite-vec.
    pub async fn search_vector(
        &self,
        query_vector: &[f32],
        event_types: &[&str],
        session_id: Option<&str>,
        cwd: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<Event>> {
        let conn = self.conn.lock().unwrap();

        let mut sql = String::from(
            "SELECT id, timestamp, event_type, category, session_id,
                    content, cwd, remote_url, repo_name, branch,
                    tool_name, file_path, metadata_json,
                    vec_distance_cosine(vector, ?) AS distance
             FROM events
             WHERE vector IS NOT NULL",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        params.push(Box::new(query_vector.as_bytes().to_vec()));

        append_filters(&mut sql, &mut params, event_types, session_id, cwd);

        sql.push_str(" ORDER BY distance LIMIT ?");
        params.push(Box::new(limit as i64));

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            row_to_event,
        )?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    /// Rebuild FTS5 index from source table. Rarely needed since triggers
    /// keep the index in sync, but useful for migration/repair.
    pub async fn rebuild_fts_index(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("INSERT INTO events_fts(events_fts) VALUES('rebuild')")?;
        Ok(())
    }

    /// Optimize FTS5 index by merging b-tree segments. Call after bulk inserts.
    pub async fn optimize_fts(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("INSERT INTO events_fts(events_fts) VALUES('optimize')")?;
        Ok(())
    }

    /// No-op (kept for API compatibility).
    pub async fn compact(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA optimize")?;
        Ok(())
    }

    /// Total event count.
    pub async fn count(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
        Ok(count as usize)
    }

    /// Event counts grouped by type, sorted descending.
    pub async fn counts_by_type(&self) -> anyhow::Result<Vec<(String, usize)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT event_type, COUNT(*) FROM events GROUP BY event_type ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Get a single event by ID.
    pub async fn get_by_id(&self, id: i64) -> anyhow::Result<Option<Event>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT id, timestamp, event_type, category, session_id,
                    content, cwd, remote_url, repo_name, branch,
                    tool_name, file_path, metadata_json
             FROM events WHERE id = ?1",
            [id],
            row_to_event,
        );
        match result {
            Ok(event) => Ok(Some(event)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Query events that need embeddings (null vector, embeddable types).
    pub async fn query_unembedded(&self, limit: usize) -> anyhow::Result<Vec<Event>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, event_type, category, session_id,
                    content, cwd, remote_url, repo_name, branch,
                    tool_name, file_path, metadata_json
             FROM events
             WHERE vector IS NULL
               AND event_type IN ('user_prompt','assistant','plan','thinking')
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], row_to_event)?;
        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    /// Update the vector embedding for a specific event.
    pub async fn update_vector(&self, event_id: i64, vector: &[f32]) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE events SET vector = ?1 WHERE id = ?2",
            rusqlite::params![vector.as_bytes(), event_id],
        )?;
        Ok(())
    }

    /// Drop and recreate the events table. Used by backfill to start fresh.
    /// Holds the lock for the entire operation to prevent races.
    pub async fn recreate_table(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "DROP TABLE IF EXISTS events_fts;
             DROP TABLE IF EXISTS events;",
        )?;
        create_schema(&conn)
    }

}

// ── Schema ─────────────────────────────────────────────────────────

/// Create the events table, indexes, FTS5 virtual table, and sync triggers.
/// Takes `&Connection` directly so callers can hold their lock.
fn create_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            event_type TEXT NOT NULL,
            category TEXT NOT NULL,
            session_id TEXT,
            content TEXT NOT NULL,
            cwd TEXT,
            remote_url TEXT,
            repo_name TEXT,
            branch TEXT,
            tool_name TEXT,
            file_path TEXT,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            vector BLOB
        );

        CREATE INDEX IF NOT EXISTS idx_events_type ON events(event_type);
        CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id);
        CREATE INDEX IF NOT EXISTS idx_events_cwd ON events(cwd);
        CREATE INDEX IF NOT EXISTS idx_events_ts ON events(timestamp DESC);

        CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
            content,
            content='events',
            content_rowid='id',
            tokenize='porter unicode61',
            prefix='2 3'
        );

        CREATE TRIGGER IF NOT EXISTS events_fts_insert AFTER INSERT ON events BEGIN
            INSERT INTO events_fts(rowid, content) VALUES (new.id, new.content);
        END;

        CREATE TRIGGER IF NOT EXISTS events_fts_delete AFTER DELETE ON events BEGIN
            INSERT INTO events_fts(events_fts, rowid, content) VALUES ('delete', old.id, old.content);
        END;

        CREATE TRIGGER IF NOT EXISTS events_fts_update AFTER UPDATE ON events BEGIN
            INSERT INTO events_fts(events_fts, rowid, content) VALUES ('delete', old.id, old.content);
            INSERT INTO events_fts(rowid, content) VALUES (new.id, new.content);
        END;",
    )?;
    Ok(())
}

// ── Query Helpers ──────────────────────────────────────────────────

/// Append optional WHERE clauses for event_type, session_id, and cwd filters.
fn append_filters(
    sql: &mut String,
    params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
    event_types: &[&str],
    session_id: Option<&str>,
    cwd: Option<&str>,
) {
    append_filters_prefixed(sql, params, event_types, session_id, cwd, "")
}

/// Same as `append_filters` but with a table alias prefix (e.g., "es").
fn append_filters_prefixed(
    sql: &mut String,
    params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
    event_types: &[&str],
    session_id: Option<&str>,
    cwd: Option<&str>,
    prefix: &str,
) {
    let dot = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}.")
    };

    if event_types.len() == 1 {
        sql.push_str(&format!(" AND {dot}event_type = ?"));
        params.push(Box::new(event_types[0].to_string()));
    } else if event_types.len() > 1 {
        let placeholders: String = event_types.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        sql.push_str(&format!(" AND {dot}event_type IN ({placeholders})"));
        for et in event_types {
            params.push(Box::new(et.to_string()));
        }
    }

    if let Some(sid) = session_id {
        sql.push_str(&format!(" AND {dot}session_id = ?"));
        params.push(Box::new(sid.to_string()));
    }

    if let Some(cwd_val) = cwd {
        sql.push_str(&format!(" AND {dot}cwd LIKE ? || '%'"));
        params.push(Box::new(cwd_val.to_string()));
    }
}

/// Convert a rusqlite Row to an Event struct.
fn row_to_event(row: &rusqlite::Row) -> rusqlite::Result<Event> {
    Ok(Event {
        id: row.get("id")?,
        timestamp: row.get("timestamp")?,
        event_type: row.get("event_type")?,
        category: row.get("category")?,
        session_id: row.get("session_id")?,
        content: row.get("content")?,
        cwd: row.get("cwd")?,
        remote_url: row.get("remote_url")?,
        repo_name: row.get("repo_name")?,
        branch: row.get("branch")?,
        tool_name: row.get("tool_name")?,
        file_path: row.get("file_path")?,
        metadata_json: row.get("metadata_json")?,
        vector: None, // Don't deserialize vectors on read (perf)
    })
}

impl std::fmt::Debug for EventStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventStore").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_event(event_type: &str, content: &str) -> Event {
        Event {
            id: 0,
            timestamp: "2026-02-14T12:00:00Z".to_string(),
            event_type: event_type.to_string(),
            category: "conversation".to_string(),
            session_id: Some("sess-001".to_string()),
            content: content.to_string(),
            cwd: Some("/home/user/project".to_string()),
            remote_url: None,
            repo_name: Some("myrepo".to_string()),
            branch: Some("main".to_string()),
            tool_name: None,
            file_path: None,
            metadata_json: "{}".to_string(),
            vector: None,
        }
    }

    fn open_store() -> (EventStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = EventStore::open(dir.path()).unwrap();
        (store, dir)
    }

    #[tokio::test]
    async fn open_creates_schema_and_verifies_sqlite_vec() {
        let (store, _dir) = open_store();
        // If we got here, schema + sqlite-vec initialized successfully
        assert_eq!(store.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn insert_assigns_autoincrement_id() {
        let (store, _dir) = open_store();
        let event = test_event("user_prompt", "hello world");
        let id1 = store.insert(&event).await.unwrap();
        let id2 = store.insert(&event).await.unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[tokio::test]
    async fn insert_then_get_by_id_roundtrips() {
        let (store, _dir) = open_store();
        let event = test_event("assistant", "I can help with that");
        let id = store.insert(&event).await.unwrap();

        let retrieved = store.get_by_id(id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, id);
        assert_eq!(retrieved.event_type, "assistant");
        assert_eq!(retrieved.content, "I can help with that");
        assert_eq!(retrieved.session_id, Some("sess-001".to_string()));
    }

    #[tokio::test]
    async fn get_by_id_returns_none_for_missing() {
        let (store, _dir) = open_store();
        assert!(store.get_by_id(9999).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn bulk_insert_with_explicit_ids() {
        let (store, _dir) = open_store();
        let events: Vec<Event> = (1..=100)
            .map(|i| {
                let mut e = test_event("tool_use", &format!("event {i}"));
                e.id = i;
                e
            })
            .collect();

        store.bulk_insert(&events).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 100);

        let e50 = store.get_by_id(50).await.unwrap().unwrap();
        assert_eq!(e50.content, "event 50");
    }

    #[tokio::test]
    async fn bulk_insert_empty_is_noop() {
        let (store, _dir) = open_store();
        store.bulk_insert(&[]).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn count_and_counts_by_type() {
        let (store, _dir) = open_store();
        for content in ["a", "b", "c"] {
            store
                .insert(&test_event("user_prompt", content))
                .await
                .unwrap();
        }
        store
            .insert(&test_event("assistant", "reply"))
            .await
            .unwrap();

        assert_eq!(store.count().await.unwrap(), 4);

        let counts = store.counts_by_type().await.unwrap();
        assert_eq!(counts[0], ("user_prompt".to_string(), 3));
        assert_eq!(counts[1], ("assistant".to_string(), 1));
    }

    #[tokio::test]
    async fn query_filters_by_event_type() {
        let (store, _dir) = open_store();
        store
            .insert(&test_event("user_prompt", "q1"))
            .await
            .unwrap();
        store
            .insert(&test_event("assistant", "a1"))
            .await
            .unwrap();
        store
            .insert(&test_event("user_prompt", "q2"))
            .await
            .unwrap();

        let prompts = store.query(&["user_prompt"], None, None, 10).await.unwrap();
        assert_eq!(prompts.len(), 2);
        assert!(prompts.iter().all(|e| e.event_type == "user_prompt"));
    }

    #[tokio::test]
    async fn query_filters_by_session_id() {
        let (store, _dir) = open_store();
        let mut e1 = test_event("assistant", "from session 1");
        e1.session_id = Some("sess-001".to_string());
        let mut e2 = test_event("assistant", "from session 2");
        e2.session_id = Some("sess-002".to_string());

        store.insert(&e1).await.unwrap();
        store.insert(&e2).await.unwrap();

        let results = store
            .query(&[], Some("sess-002"), None, 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "from session 2");
    }

    #[tokio::test]
    async fn query_filters_by_cwd_prefix() {
        let (store, _dir) = open_store();
        let mut e1 = test_event("assistant", "in project");
        e1.cwd = Some("/home/user/project".to_string());
        let mut e2 = test_event("assistant", "in other");
        e2.cwd = Some("/home/user/other".to_string());

        store.insert(&e1).await.unwrap();
        store.insert(&e2).await.unwrap();

        let results = store
            .query(&[], None, Some("/home/user/project"), 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "in project");
    }

    #[tokio::test]
    async fn query_returns_timestamp_desc_order() {
        let (store, _dir) = open_store();
        for (i, ts) in ["2026-01-01T00:00:00Z", "2026-01-03T00:00:00Z", "2026-01-02T00:00:00Z"]
            .iter()
            .enumerate()
        {
            let mut e = test_event("assistant", &format!("event {i}"));
            e.timestamp = ts.to_string();
            store.insert(&e).await.unwrap();
        }

        let results = store.query(&[], None, None, 10).await.unwrap();
        assert_eq!(results[0].timestamp, "2026-01-03T00:00:00Z");
        assert_eq!(results[1].timestamp, "2026-01-02T00:00:00Z");
        assert_eq!(results[2].timestamp, "2026-01-01T00:00:00Z");
    }

    #[tokio::test]
    async fn query_range_filters_by_time() {
        let (store, _dir) = open_store();
        for ts in [
            "2026-01-01T00:00:00Z",
            "2026-01-15T00:00:00Z",
            "2026-02-01T00:00:00Z",
        ] {
            let mut e = test_event("assistant", ts);
            e.timestamp = ts.to_string();
            store.insert(&e).await.unwrap();
        }

        let results = store
            .query_range(
                &[],
                None,
                None,
                Some("2026-01-10T00:00:00Z"),
                Some("2026-01-20T00:00:00Z"),
                10,
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].timestamp, "2026-01-15T00:00:00Z");
    }

    // ── FTS Tests (the critical migration behavior) ─────────────────

    #[tokio::test]
    async fn fts_immediately_searchable_after_insert() {
        let (store, _dir) = open_store();
        // This is THE test - FTS triggers ensure immediate searchability.
        store
            .insert(&test_event("user_prompt", "implement authentication middleware"))
            .await
            .unwrap();

        // Immediately search - no rebuild needed
        let results = store
            .search_fts("authentication", &[], None, None, 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("authentication"));
    }

    #[tokio::test]
    async fn fts_respects_filters() {
        let (store, _dir) = open_store();
        let mut e1 = test_event("user_prompt", "fix the login bug");
        e1.session_id = Some("sess-A".to_string());
        let mut e2 = test_event("assistant", "the login bug is in auth.rs");
        e2.session_id = Some("sess-B".to_string());

        store.insert(&e1).await.unwrap();
        store.insert(&e2).await.unwrap();

        // Both match "login"
        let all = store.search_fts("login", &[], None, None, 10).await.unwrap();
        assert_eq!(all.len(), 2);

        // Filter by type
        let prompts_only = store
            .search_fts("login", &["user_prompt"], None, None, 10)
            .await
            .unwrap();
        assert_eq!(prompts_only.len(), 1);
        assert_eq!(prompts_only[0].event_type, "user_prompt");

        // Filter by session
        let sess_b = store
            .search_fts("login", &[], Some("sess-B"), None, 10)
            .await
            .unwrap();
        assert_eq!(sess_b.len(), 1);
        assert_eq!(sess_b[0].session_id, Some("sess-B".to_string()));
    }

    #[tokio::test]
    async fn fts_porter_stemming_matches_word_variants() {
        let (store, _dir) = open_store();
        store
            .insert(&test_event("assistant", "implementing the feature"))
            .await
            .unwrap();

        // Porter stemmer: "implement" should match "implementing"
        let results = store
            .search_fts("implement", &[], None, None, 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn fts_no_results_for_nonexistent_term() {
        let (store, _dir) = open_store();
        store
            .insert(&test_event("assistant", "hello world"))
            .await
            .unwrap();

        let results = store
            .search_fts("xyzzy_nonexistent", &[], None, None, 10)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn rebuild_fts_index_keeps_data_searchable() {
        let (store, _dir) = open_store();
        store
            .insert(&test_event("assistant", "important data"))
            .await
            .unwrap();

        store.rebuild_fts_index().await.unwrap();

        let results = store
            .search_fts("important", &[], None, None, 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    // ── Vector Search Tests ─────────────────────────────────────────

    #[tokio::test]
    async fn vector_search_finds_nearest() {
        let (store, _dir) = open_store();

        // Insert events with known vectors
        let mut e1 = test_event("assistant", "about rust");
        e1.vector = Some(vec![1.0, 0.0, 0.0]);
        let mut e2 = test_event("assistant", "about python");
        e2.vector = Some(vec![0.0, 1.0, 0.0]);
        let mut e3 = test_event("assistant", "also about rust");
        e3.vector = Some(vec![0.9, 0.1, 0.0]);

        store.insert(&e1).await.unwrap();
        store.insert(&e2).await.unwrap();
        store.insert(&e3).await.unwrap();

        // Query vector closest to [1, 0, 0] - should return "about rust" first
        let results = store
            .search_vector(&[1.0, 0.0, 0.0], &[], None, None, 2)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].content, "about rust");
        assert_eq!(results[1].content, "also about rust");
    }

    #[tokio::test]
    async fn vector_search_skips_null_vectors() {
        let (store, _dir) = open_store();

        let mut with_vec = test_event("assistant", "has embedding");
        with_vec.vector = Some(vec![1.0, 0.0, 0.0]);
        let without_vec = test_event("assistant", "no embedding");

        store.insert(&with_vec).await.unwrap();
        store.insert(&without_vec).await.unwrap();

        let results = store
            .search_vector(&[1.0, 0.0, 0.0], &[], None, None, 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "has embedding");
    }

    // ── Embedding Lifecycle Tests ───────────────────────────────────

    #[tokio::test]
    async fn query_unembedded_returns_embeddable_types_only() {
        let (store, _dir) = open_store();
        store
            .insert(&test_event("user_prompt", "should embed"))
            .await
            .unwrap();
        store
            .insert(&test_event("tool_use", "should NOT embed"))
            .await
            .unwrap();
        store
            .insert(&test_event("assistant", "should embed too"))
            .await
            .unwrap();

        let unembedded = store.query_unembedded(100).await.unwrap();
        assert_eq!(unembedded.len(), 2);
        assert!(unembedded.iter().all(|e| e.event_type != "tool_use"));
    }

    #[tokio::test]
    async fn update_vector_then_no_longer_unembedded() {
        let (store, _dir) = open_store();
        let id = store
            .insert(&test_event("user_prompt", "embed me"))
            .await
            .unwrap();

        assert_eq!(store.query_unembedded(100).await.unwrap().len(), 1);

        store.update_vector(id, &[0.1, 0.2, 0.3]).await.unwrap();

        assert_eq!(store.query_unembedded(100).await.unwrap().len(), 0);
    }

    // ── Table Lifecycle Tests ───────────────────────────────────────

    #[tokio::test]
    async fn recreate_table_drops_all_data() {
        let (store, _dir) = open_store();
        for i in 0..10 {
            store
                .insert(&test_event("assistant", &format!("event {i}")))
                .await
                .unwrap();
        }
        assert_eq!(store.count().await.unwrap(), 10);

        store.recreate_table().await.unwrap();
        assert_eq!(store.count().await.unwrap(), 0);

        // FTS still works after recreate
        store
            .insert(&test_event("assistant", "fresh start"))
            .await
            .unwrap();
        let results = store
            .search_fts("fresh", &[], None, None, 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn compact_is_harmless_noop() {
        let (store, _dir) = open_store();
        store
            .insert(&test_event("assistant", "data"))
            .await
            .unwrap();
        // Should not error or affect data
        store.compact().await.unwrap();
        assert_eq!(store.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn query_with_multiple_event_types() {
        let (store, _dir) = open_store();
        store
            .insert(&test_event("user_prompt", "q"))
            .await
            .unwrap();
        store
            .insert(&test_event("assistant", "a"))
            .await
            .unwrap();
        store
            .insert(&test_event("tool_use", "t"))
            .await
            .unwrap();

        let results = store
            .query(&["user_prompt", "assistant"], None, None, 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.event_type != "tool_use"));
    }
}

