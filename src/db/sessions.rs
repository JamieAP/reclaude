use std::collections::HashMap;

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

    /// Get the Mattermost thread mapping for a session.
    pub async fn get_mm_thread(
        &self,
        session_id: &str,
    ) -> anyhow::Result<Option<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let row = conn.query_row(
            "SELECT channel_spec, root_post_id FROM session_mm_threads WHERE session_id = ?1",
            [session_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        );

        match row {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Upsert the Mattermost thread mapping for a session.
    pub async fn upsert_mm_thread(
        &self,
        session_id: &str,
        channel_spec: &str,
        root_post_id: &str,
        timestamp: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO session_mm_threads (session_id, channel_spec, root_post_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(session_id) DO UPDATE SET
               channel_spec = excluded.channel_spec,
               root_post_id = excluded.root_post_id,
               updated_at = excluded.updated_at",
            rusqlite::params![session_id, channel_spec, root_post_id, timestamp],
        )?;
        Ok(())
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

    // ── Recap ─────────────────────────────────────────────────────────

    /// Time-bucketed activity summary grouped by repo.
    /// Returns rows sorted by bucket (most recent first), then event count DESC.
    pub async fn query_recap(&self, opts: &RecapOptions) -> anyhow::Result<Vec<RecapRow>> {
        let conn = self.conn.lock().unwrap();

        // When zoomed, use cumulative time range; otherwise exclusive buckets
        let bucket_expr = if let Some(ref zoom) = opts.zoom {
            let interval = match zoom.as_str() {
                "2h" => "-2 hours",
                "24h" => "-24 hours",
                "7d" => "-7 days",
                _ => "-7 days",
            };
            format!(
                "CASE WHEN e.timestamp >= datetime('now', '{interval}') THEN '{zoom}' ELSE NULL END"
            )
        } else {
            "CASE \
                WHEN e.timestamp >= datetime('now', '-2 hours') THEN '2h' \
                WHEN e.timestamp >= datetime('now', '-24 hours') THEN '24h' \
                WHEN e.timestamp >= datetime('now', '-7 days') THEN '7d' \
            END".to_string()
        };

        let sql = format!(
            "WITH bucketed AS (
                SELECT
                    {bucket_expr} as bucket,
                    COALESCE(s.repo_name,
                        CASE WHEN e.cwd LIKE '/home/%/dev/%' THEN
                            substr(e.cwd, instr(e.cwd, '/dev/') + 5,
                                CASE WHEN instr(substr(e.cwd, instr(e.cwd, '/dev/') + 5), '/') > 0
                                    THEN instr(substr(e.cwd, instr(e.cwd, '/dev/') + 5), '/') - 1
                                    ELSE length(substr(e.cwd, instr(e.cwd, '/dev/') + 5))
                                END)
                        ELSE e.cwd END
                    ) as repo,
                    e.event_type,
                    e.session_id
                FROM events e
                LEFT JOIN sessions s ON e.session_id = s.session_id
                WHERE e.timestamp >= datetime('now', '-7 days')
            )
            SELECT
                bucket, repo,
                COUNT(*) as events,
                COUNT(DISTINCT session_id) as sessions,
                SUM(CASE WHEN event_type = 'file_diff' THEN 1 ELSE 0 END) as diffs,
                SUM(CASE WHEN event_type = 'user_prompt' THEN 1 ELSE 0 END) as prompts
            FROM bucketed
            WHERE bucket IS NOT NULL AND repo IS NOT NULL AND repo != ''
            GROUP BY bucket, repo
            HAVING events >= ?1
            ORDER BY
                CASE bucket WHEN '2h' THEN 0 WHEN '24h' THEN 1 WHEN '7d' THEN 2 END,
                events DESC"
        );

        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<RecapRow> = stmt.query_map([opts.min_events as i64], |row| {
            Ok(RecapRow {
                bucket: row.get(0)?,
                repo: row.get(1)?,
                events: row.get::<_, i64>(2)? as usize,
                sessions: row.get::<_, i64>(3)? as usize,
                diffs: row.get::<_, i64>(4)? as usize,
                prompts: row.get::<_, i64>(5)? as usize,
                top_files: Vec::new(),
                top_dirs: Vec::new(),
                top_prompts: Vec::new(),
                session_info: Vec::new(),
            })
        })?.filter_map(|r| r.ok()).collect();

        // Step 2: Enrich each row with file paths and (if zoomed) prompts
        let mut result = Vec::with_capacity(rows.len());
        for mut row in rows {
            let bucket_clause = if opts.zoom.is_some() {
                // Zoomed: cumulative window
                match row.bucket.as_str() {
                    "2h" => "e.timestamp >= datetime('now', '-2 hours')",
                    "24h" => "e.timestamp >= datetime('now', '-24 hours')",
                    "7d" => "e.timestamp >= datetime('now', '-7 days')",
                    _ => continue,
                }
            } else {
                // Overview: exclusive windows
                match row.bucket.as_str() {
                    "2h" => "e.timestamp >= datetime('now', '-2 hours')",
                    "24h" => "e.timestamp >= datetime('now', '-24 hours') AND e.timestamp < datetime('now', '-2 hours')",
                    "7d" => "e.timestamp >= datetime('now', '-7 days') AND e.timestamp < datetime('now', '-24 hours')",
                    _ => continue,
                }
            };

            // File data: individual paths (overview) or directory aggregation (zoomed)
            let want_files = (opts.zoom.is_some() && opts.dir_limit > 0)
                || (opts.zoom.is_none() && opts.file_limit > 0);
            if want_files {
                let sql_limit = if opts.zoom.is_some() { 200 } else { opts.file_limit };
                let file_sql = format!(
                    "SELECT
                        CASE
                            WHEN e.content LIKE '--- /%' THEN substr(e.content, 5, instr(substr(e.content, 5), char(10))-1)
                            WHEN e.content LIKE '+++ /%' THEN substr(e.content, 5, instr(substr(e.content, 5), char(10))-1)
                            ELSE NULL
                        END as filepath,
                        COUNT(*) as touches
                    FROM events e
                    LEFT JOIN sessions s ON e.session_id = s.session_id
                    WHERE e.event_type = 'file_diff'
                        AND {bucket_clause}
                        AND COALESCE(s.repo_name, '') = ?1
                        AND filepath IS NOT NULL
                        AND filepath != '/dev/null'
                    GROUP BY filepath
                    ORDER BY touches DESC
                    LIMIT ?2"
                );
                if let Ok(mut fstmt) = conn.prepare(&file_sql) {
                    if let Ok(files) = fstmt.query_map(
                        rusqlite::params![&row.repo, sql_limit as i64],
                        |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)),
                    ) {
                        let entries: Vec<(String, usize)> =
                            files.filter_map(|r| r.ok()).collect();

                        if opts.zoom.is_some() {
                            // Aggregate files into directories
                            let mut dir_map: HashMap<String, (usize, usize)> = HashMap::new();
                            for (filepath, touches) in &entries {
                                let dir = match filepath.rfind('/') {
                                    Some(idx) => filepath[..idx + 1].to_string(),
                                    None => String::new(),
                                };
                                let stats = dir_map.entry(dir).or_insert((0, 0));
                                stats.0 += 1; // unique files
                                stats.1 += touches; // total edits
                            }
                            let mut dirs: Vec<DirStats> = dir_map
                                .into_iter()
                                .map(|(dir, (files, edits))| DirStats {
                                    dir,
                                    unique_files: files,
                                    total_edits: edits,
                                })
                                .collect();
                            dirs.sort_by(|a, b| b.total_edits.cmp(&a.total_edits));
                            dirs.truncate(opts.dir_limit);
                            row.top_dirs = dirs;
                        } else {
                            row.top_files =
                                entries.into_iter().map(|(f, _)| f).collect();
                        }
                    }
                }
            }

            // Session tree (only when zoomed)
            if opts.session_limit > 0 {
                let session_sql = format!(
                    "SELECT
                        ms.session_id,
                        ms.event_count,
                        COALESCE(
                            (SELECT COUNT(*) FROM session_transcripts st
                             WHERE st.parent_session_id = ms.session_id), 0
                        ) as child_count,
                        COALESCE(
                            (SELECT substr(e2.content, 1, 100)
                             FROM events e2
                             WHERE e2.session_id = ms.session_id
                               AND e2.event_type = 'user_prompt'
                             ORDER BY e2.timestamp ASC
                             LIMIT 1),
                            ''
                        ) as first_prompt
                    FROM (
                        SELECT e.session_id, COUNT(*) as event_count
                        FROM events e
                        LEFT JOIN sessions s ON e.session_id = s.session_id
                        WHERE {bucket_clause}
                            AND COALESCE(s.repo_name, '') = ?1
                        GROUP BY e.session_id
                    ) ms
                    ORDER BY ms.event_count DESC
                    LIMIT ?2"
                );
                if let Ok(mut sstmt) = conn.prepare(&session_sql) {
                    if let Ok(sessions) = sstmt.query_map(
                        rusqlite::params![&row.repo, opts.session_limit as i64],
                        |r| {
                            Ok(SessionInfo {
                                session_id: r.get(0)?,
                                event_count: r.get::<_, i64>(1)? as usize,
                                child_count: r.get::<_, i64>(2)? as usize,
                                description: r
                                    .get::<_, String>(3)?
                                    .trim()
                                    .replace('\n', " "),
                                spawns: Vec::new(),
                            })
                        },
                    ) {
                        let mut infos: Vec<SessionInfo> =
                            sessions.filter_map(|r| r.ok()).collect();

                        // Enrich with subagent_spawn descriptions
                        for info in &mut infos {
                            if info.child_count == 0 {
                                continue;
                            }
                            let spawn_sql =
                                "SELECT substr(content, 1,
                                    CASE WHEN instr(content, char(10)) > 0
                                         THEN instr(content, char(10)) - 1
                                         ELSE 100 END)
                                 FROM events
                                 WHERE session_id = ?1
                                   AND event_type = 'subagent_spawn'
                                 ORDER BY timestamp DESC
                                 LIMIT 8";
                            if let Ok(mut sstmt2) = conn.prepare(spawn_sql) {
                                if let Ok(spawns) = sstmt2.query_map(
                                    [&info.session_id],
                                    |r| r.get::<_, String>(0),
                                ) {
                                    info.spawns = spawns
                                        .filter_map(|r| r.ok())
                                        .collect();
                                }
                            }
                        }

                        row.session_info = infos;
                    }
                }
            }

            // Top prompts (only when zoomed)
            if opts.prompt_limit > 0 {
                let prompt_sql = format!(
                    "SELECT substr(e.content, 1, 120)
                    FROM events e
                    LEFT JOIN sessions s ON e.session_id = s.session_id
                    WHERE e.event_type = 'user_prompt'
                        AND {bucket_clause}
                        AND COALESCE(s.repo_name, '') = ?1
                        AND length(e.content) > 5
                    ORDER BY e.timestamp DESC
                    LIMIT ?2"
                );
                if let Ok(mut pstmt) = conn.prepare(&prompt_sql) {
                    if let Ok(prompts) = pstmt.query_map(
                        rusqlite::params![&row.repo, opts.prompt_limit as i64],
                        |r| r.get::<_, String>(0),
                    ) {
                        row.top_prompts = prompts
                            .filter_map(|r| r.ok())
                            .map(|s| s.trim().replace('\n', " "))
                            .collect();
                    }
                }
            }

            result.push(row);
        }

        Ok(result)
    }
}

/// A single row in the recap: one repo in one time bucket.
pub struct RecapRow {
    pub bucket: String,
    pub repo: String,
    pub events: usize,
    pub sessions: usize,
    pub diffs: usize,
    pub prompts: usize,
    /// Top file paths touched (from diffs), for overview mode.
    pub top_files: Vec<String>,
    /// Directory-level aggregation (from diffs), for zoomed mode.
    pub top_dirs: Vec<DirStats>,
    /// Recent user prompts (newest first), for zoomed views.
    pub top_prompts: Vec<String>,
    /// Session tree info (parent → child), for zoomed views.
    pub session_info: Vec<SessionInfo>,
}

/// Directory-level edit statistics.
pub struct DirStats {
    pub dir: String,
    pub unique_files: usize,
    pub total_edits: usize,
}

/// Session identity with spawned subagent info for tree display.
pub struct SessionInfo {
    pub session_id: String,
    pub event_count: usize,
    pub child_count: usize,
    pub description: String,
    /// First-line descriptions from subagent_spawn events.
    pub spawns: Vec<String>,
}

/// Which time window to show, and how much detail.
pub struct RecapOptions {
    pub min_events: usize,
    /// If set, show only this bucket with extra detail.
    pub zoom: Option<String>,
    /// Max file paths per row (overview mode).
    pub file_limit: usize,
    /// Max directories per row (zoomed mode).
    pub dir_limit: usize,
    /// Max sessions per row (0 = skip).
    pub session_limit: usize,
    /// Max prompts per row (0 = skip).
    pub prompt_limit: usize,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_db() -> (Database, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = Database::open_at(dir.path()).unwrap();
        (db, dir)
    }

    #[tokio::test]
    async fn mm_thread_roundtrip() {
        let (db, _dir) = open_db();
        assert!(db.get_mm_thread("sess-001").await.unwrap().is_none());

        db.upsert_mm_thread(
            "sess-001",
            "demo:sessions",
            "root-post-1",
            "2026-02-27T12:00:00Z",
        )
        .await
        .unwrap();

        let thread = db.get_mm_thread("sess-001").await.unwrap();
        assert_eq!(
            thread,
            Some((
                "demo:sessions".to_string(),
                "root-post-1".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn mm_thread_upsert_overwrites_existing_mapping() {
        let (db, _dir) = open_db();
        db.upsert_mm_thread(
            "sess-001",
            "demo:sessions",
            "root-post-1",
            "2026-02-27T12:00:00Z",
        )
        .await
        .unwrap();

        db.upsert_mm_thread(
            "sess-001",
            "demo:sessions",
            "root-post-2",
            "2026-02-27T12:05:00Z",
        )
        .await
        .unwrap();

        let thread = db.get_mm_thread("sess-001").await.unwrap().unwrap();
        assert_eq!(thread.1, "root-post-2");
    }
}
