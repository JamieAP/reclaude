use crate::models::{ScanState, Session, SessionTranscript};

use super::Database;

// ── Session Methods ────────────────────────────────────────────────

impl Database {
    /// Upsert a session record (called on every captured event).
    pub async fn upsert_session(
        &self,
        session_id: &str,
        timestamp: &str,
        cwd: Option<&str>,
        repo_name: Option<&str>,
        remote_url: Option<&str>,
        branch: Option<&str>,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (session_id, started_at, ended_at, cwd, repo_name, remote_url, branch, event_count, is_active)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, 1, 1)
             ON CONFLICT(session_id) DO UPDATE SET
               ended_at = ?2,
               event_count = event_count + 1,
               is_active = 1,
               cwd = COALESCE(sessions.cwd, ?3),
               repo_name = COALESCE(sessions.repo_name, ?4),
               remote_url = COALESCE(sessions.remote_url, ?5),
               branch = COALESCE(sessions.branch, ?6)",
            rusqlite::params![session_id, timestamp, cwd, repo_name, remote_url, branch],
        )?;
        Ok(())
    }

    /// Mark a session as inactive (on session_end or stop).
    pub async fn end_session(&self, session_id: &str, timestamp: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET ended_at = ?1, is_active = 0 WHERE session_id = ?2",
            rusqlite::params![timestamp, session_id],
        )?;
        Ok(())
    }

    /// List sessions, optionally filtered by cwd prefix.
    pub async fn list_sessions(
        &self,
        cwd: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<Session>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT session_id, started_at, ended_at, cwd, repo_name, remote_url, branch, event_count, is_active
             FROM sessions",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(cwd_val) = cwd {
            sql.push_str(" WHERE cwd LIKE ?1");
            params.push(Box::new(format!("{cwd_val}%")));
        }

        sql.push_str(" ORDER BY started_at DESC LIMIT ?");
        params.push(Box::new(limit as i64));

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
            Ok(Session {
                session_id: row.get(0)?,
                started_at: row.get(1)?,
                ended_at: row.get(2)?,
                cwd: row.get(3)?,
                repo_name: row.get(4)?,
                remote_url: row.get(5)?,
                branch: row.get(6)?,
                event_count: row.get(7)?,
                is_active: row.get::<_, i64>(8)? != 0,
            })
        })?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }
        Ok(sessions)
    }

    // ── Tag Operations ──────────────────────────────────────────────

    /// Create a random tag for the current session context.
    pub async fn tag_session(&self, cwd: &str) -> anyhow::Result<String> {
        let conn = self.conn.lock().unwrap();
        let tag = generate_tag();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR REPLACE INTO session_tags (tag, created_at, cwd) VALUES (?1, ?2, ?3)",
            rusqlite::params![tag, now, cwd],
        )?;
        Ok(tag)
    }

    /// Look up a session ID by its tag.
    /// Resolves lazily: if session_id is NULL, finds the most recent event
    /// matching the tag's cwd before the tag timestamp, then caches the result.
    pub async fn get_session_by_tag(&self, tag: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let row = conn.query_row(
            "SELECT session_id, created_at, cwd FROM session_tags WHERE tag = ?1",
            [tag],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        );
        let (cached_sid, created_at, cwd) = match row {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        // Already resolved
        if cached_sid.is_some() {
            return Ok(cached_sid);
        }

        // Lazy resolve: find session with most recent event in this cwd before tag creation
        let resolved: Option<String> = conn
            .query_row(
                "SELECT session_id FROM events
                 WHERE cwd = ?1 AND timestamp <= ?2 AND session_id IS NOT NULL
                 ORDER BY timestamp DESC LIMIT 1",
                rusqlite::params![cwd, created_at],
                |row| row.get(0),
            )
            .ok();

        // Cache for future lookups
        if let Some(ref sid) = resolved {
            let _ = conn.execute(
                "UPDATE session_tags SET session_id = ?1 WHERE tag = ?2",
                rusqlite::params![sid, tag],
            );
        }

        Ok(resolved)
    }

    // ── Transcript Operations ───────────────────────────────────────

    /// Upsert a compressed transcript archive. Returns true if new insert.
    pub async fn upsert_transcript(
        &self,
        session_id: &str,
        content: &[u8],
        size_bytes: i64,
        compressed_bytes: i64,
        transcript_path: Option<&str>,
        parent_session_id: Option<&str>,
        metadata_json: Option<&str>,
    ) -> anyhow::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let result = conn.execute(
            "INSERT INTO session_transcripts (session_id, parent_session_id, archived_at, transcript_path, size_bytes, compressed_bytes, content, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(session_id) DO UPDATE SET
               archived_at = ?3,
               size_bytes = ?5,
               compressed_bytes = ?6,
               content = ?7,
               metadata = ?8",
            rusqlite::params![session_id, parent_session_id, now, transcript_path, size_bytes, compressed_bytes, content, metadata_json],
        )?;
        Ok(result > 0)
    }

    /// List archived transcripts.
    pub async fn list_transcripts(
        &self,
        include_subagents: bool,
        limit: usize,
    ) -> anyhow::Result<Vec<SessionTranscript>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, session_id, parent_session_id, archived_at, transcript_path, size_bytes, compressed_bytes, metadata
             FROM session_transcripts",
        );
        if !include_subagents {
            sql.push_str(" WHERE parent_session_id IS NULL");
        }
        sql.push_str(" ORDER BY archived_at DESC LIMIT ?");

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok(SessionTranscript {
                id: row.get(0)?,
                session_id: row.get(1)?,
                parent_session_id: row.get(2)?,
                archived_at: row.get(3)?,
                transcript_path: row.get(4)?,
                size_bytes: row.get(5)?,
                compressed_bytes: row.get(6)?,
                metadata_json: row.get(7)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get compressed transcript content by session ID.
    pub async fn get_transcript_content(&self, session_id: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT content FROM session_transcripts WHERE session_id = ?1",
            [session_id],
            |row| row.get::<_, Vec<u8>>(0),
        );
        match result {
            Ok(content) => Ok(Some(content)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get archived session IDs with their sizes (for change detection).
    pub async fn get_archived_session_ids(&self) -> anyhow::Result<Vec<(String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT session_id, size_bytes FROM session_transcripts",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get transcript archive statistics.
    pub async fn get_transcript_stats(&self) -> anyhow::Result<TranscriptStats> {
        let conn = self.conn.lock().unwrap();
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_transcripts", [], |r| r.get(0)
        )?;
        let main_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_transcripts WHERE parent_session_id IS NULL", [], |r| r.get(0)
        )?;
        let total_size: i64 = conn.query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM session_transcripts", [], |r| r.get(0)
        )?;
        let total_compressed: i64 = conn.query_row(
            "SELECT COALESCE(SUM(compressed_bytes), 0) FROM session_transcripts", [], |r| r.get(0)
        )?;
        let oldest: String = conn.query_row(
            "SELECT COALESCE(MIN(archived_at), '') FROM session_transcripts", [], |r| r.get(0)
        )?;
        let newest: String = conn.query_row(
            "SELECT COALESCE(MAX(archived_at), '') FROM session_transcripts", [], |r| r.get(0)
        )?;

        Ok(TranscriptStats {
            total: total as usize,
            main_count: main_count as usize,
            subagent_count: (total - main_count) as usize,
            total_size_bytes: total_size,
            total_compressed_bytes: total_compressed,
            oldest,
            newest,
        })
    }

    // ── Scan State Operations ────────────────────────────────────────

    /// Get the scan state for a transcript path.
    pub async fn get_scan_state(&self, transcript_path: &str) -> anyhow::Result<Option<ScanState>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT last_byte_offset FROM transcript_scan_state WHERE transcript_path = ?1",
            [transcript_path],
            |row| Ok(ScanState {
                last_byte_offset: row.get(0)?,
            }),
        );
        match result {
            Ok(state) => Ok(Some(state)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Update scan state for incremental transcript processing.
    pub async fn update_scan_state(
        &self,
        transcript_path: &str,
        last_byte_offset: i64,
        last_entry_uuid: Option<&str>,
        entries_extracted: i64,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO transcript_scan_state (transcript_path, last_byte_offset, last_entry_uuid, last_scan_time, entries_extracted)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(transcript_path) DO UPDATE SET
               last_byte_offset = ?2,
               last_entry_uuid = COALESCE(?3, last_entry_uuid),
               last_scan_time = ?4,
               entries_extracted = entries_extracted + ?5",
            rusqlite::params![transcript_path, last_byte_offset, last_entry_uuid, now, entries_extracted],
        )?;
        Ok(())
    }
}

/// Aggregate statistics for the transcript archive.
pub struct TranscriptStats {
    pub total: usize,
    pub main_count: usize,
    pub subagent_count: usize,
    pub total_size_bytes: i64,
    pub total_compressed_bytes: i64,
    pub oldest: String,
    pub newest: String,
}

fn generate_tag() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    // 12-char hex tag from full microsecond timestamp (unique across calls)
    format!("{:012x}", micros & 0xFFFF_FFFF_FFFF)
}
