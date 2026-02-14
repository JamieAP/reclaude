"""
Database layer for reclaude - semantic event capture.

Stores captured events in SQLite with:
- Connection pooling
- Schema management
- Typed query methods
- Vector similarity search via sqlite-vec
"""

from __future__ import annotations

import json
import os
import sqlite3

try:
    import sqlite_vec
    HAS_SQLITE_VEC = True
except ImportError:
    sqlite_vec = None
    HAS_SQLITE_VEC = False

from reclaude.git import normalize_remote_url
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Iterator
from contextlib import contextmanager


DEFAULT_DB_PATH = Path.home() / ".reclaude" / "capture.db"

def _normalize_session_id(session_id: str | None) -> str | None:
    """Normalize session ids so missing values never become empty-string sessions."""
    if session_id is None:
        return None
    if not isinstance(session_id, str):
        session_id = str(session_id)
    session_id = session_id.strip()
    return session_id or None


class SemanticEventType:
    USER_PROMPT = "user_prompt"
    ASSISTANT = "assistant"
    PLAN = "plan"
    THINKING = "thinking"
    COMPACTION = "compaction"
    FILE_DIFF = "file_diff"
    TOOL_USE = "tool_use"
    SYS_MSG = "sys_msg"
    PLAN_FILE = "plan_file"  # User's implementation plan document from ~/.claude/plans/
    # Session lifecycle
    SESSION_START = "session_start"
    SESSION_END = "session_end"
    # Agent and permission tracking
    SUBAGENT_STOP = "subagent_stop"
    PERMISSION_REQUEST = "permission_request"
    NOTIFICATION = "notification"
    # Task management (new todo system)
    TASK_CREATE = "task_create"
    TASK_UPDATE = "task_update"
    TASK_GET = "task_get"
    TASK_LIST = "task_list"
    # Legacy todo
    TODO_WRITE = "todo_write"
    # Subagent lifecycle
    SUBAGENT_SPAWN = "subagent_spawn"
    SUBAGENT_OUTPUT = "subagent_output"


@dataclass
class SemanticEvent:
    """A captured semantic event from Claude Code."""

    id: int
    timestamp: datetime
    event_type: str
    session_id: str | None
    content: str
    metadata: dict

    def __str__(self) -> str:
        ts = self.timestamp.strftime("%H:%M:%S")
        preview = self.content[:60].replace("\n", " ")
        if len(self.content) > 60:
            preview += "..."
        return f"[{ts}] {self.event_type}: {preview}"


@dataclass
class Learning:
    """A stored learning note."""

    id: int
    timestamp: datetime
    session_id: str | None
    cwd: str | None
    zellij_session: str | None
    content: str
    # Git context for worktree reconciliation
    repo_root: str | None = None
    remote_url: str | None = None
    repo_name: str | None = None
    branch: str | None = None
    is_worktree: bool | None = None


@dataclass
class FocusSnapshot:
    """User focus at a specific time scale."""

    id: int
    timestamp: datetime  # when generated
    project: str | None
    time_scale: str  # 'minute' | 'hour' | 'day' | 'week'
    period_start: datetime
    period_end: datetime
    focus_summary: str  # flash-generated focus description
    top_topics: list[str]  # main topics detected
    event_count: int
    metadata: dict


@dataclass
class SessionTranscript:
    """Archived session transcript (zstd-compressed JSONL)."""

    id: int
    session_id: str
    parent_session_id: str | None  # None for main sessions, parent UUID for subagents
    archived_at: datetime
    transcript_path: str | None
    size_bytes: int  # uncompressed size (for change detection)
    compressed_bytes: int | None
    metadata: dict


class CaptureDB:
    """
    Database abstraction for semantic event capture.

    Usage:
        db = CaptureDB()  # uses default path
        db = CaptureDB(Path("/custom/path.db"))
        db = CaptureDB(":memory:")  # for testing
    """

    def __init__(self, path: Path | str | None = None):
        if path is None:
            env_path = os.environ.get("RECLAUDE_DB_PATH")
            if env_path:
                path = env_path

        if path == ":memory:":
            self.path: Path | str = ":memory:"
            self._is_memory = True
            self._persistent_conn = sqlite3.connect(":memory:")
            self._persistent_conn.row_factory = sqlite3.Row
            if HAS_SQLITE_VEC:
                self._persistent_conn.enable_load_extension(True)
                sqlite_vec.load(self._persistent_conn)
                self._persistent_conn.enable_load_extension(False)
        else:
            self.path = Path(path) if path else DEFAULT_DB_PATH
            self._is_memory = False
            self._persistent_conn = None
        self._ensure_schema()

    def _ensure_schema(self) -> None:
        if not self._is_memory and isinstance(self.path, Path):
            self.path.parent.mkdir(parents=True, exist_ok=True)

        with self.connection() as conn:
            if not self._is_memory:
                conn.execute("PRAGMA journal_mode=WAL")
                conn.execute("PRAGMA busy_timeout=5000")
            conn.execute("""
                CREATE TABLE IF NOT EXISTS semantic_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    session_id TEXT,
                    content TEXT NOT NULL,
                    metadata TEXT NOT NULL DEFAULT '{}'
                )
            """)
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_semantic_type ON semantic_events(event_type)"
            )
            conn.execute("DROP INDEX IF EXISTS idx_semantic_session")
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_semantic_session_ts ON semantic_events(session_id, timestamp DESC)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_semantic_timestamp ON semantic_events(timestamp)"
            )

            # Session scan state table (preserved for data retention)
            conn.execute("""
                CREATE TABLE IF NOT EXISTS session_scan_state (
                    session_id TEXT PRIMARY KEY,
                    last_byte_offset INTEGER DEFAULT 0,
                    last_uuid TEXT,
                    last_scan_time TEXT,
                    transcript_path TEXT
                )
            """)

            conn.execute("""
                CREATE TABLE IF NOT EXISTS learnings (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    session_id TEXT,
                    cwd TEXT,
                    zellij_session TEXT,
                    content TEXT NOT NULL,
                    repo_root TEXT,
                    remote_url TEXT,
                    repo_name TEXT,
                    branch TEXT,
                    is_worktree INTEGER
                )
            """)
            # Migration: add git context columns to existing learnings table
            for col in ["repo_root TEXT", "remote_url TEXT", "repo_name TEXT", "branch TEXT", "is_worktree INTEGER"]:
                try:
                    conn.execute(f"ALTER TABLE learnings ADD COLUMN {col}")
                except sqlite3.OperationalError:
                    pass  # Column already exists
            # Create indexes (after migration ensures columns exist)
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_learnings_session ON learnings(session_id)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_learnings_timestamp ON learnings(timestamp)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_learnings_repo_root ON learnings(repo_root)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_learnings_remote_url ON learnings(remote_url)"
            )

            # Vector tables for semantic search (requires sqlite-vec)
            if HAS_SQLITE_VEC:
                from reclaude.embeddings import EMBEDDING_DIM
                conn.execute(f"""
                    CREATE VIRTUAL TABLE IF NOT EXISTS vec_learnings USING vec0(
                        embedding float[{EMBEDDING_DIM}]
                    )
                """)
                conn.execute(f"""
                    CREATE VIRTUAL TABLE IF NOT EXISTS vec_events USING vec0(
                        embedding float[{EMBEDDING_DIM}]
                    )
                """)

            # Migration: normalize remote_urls (SSH/HTTPS -> canonical form)
            rows = conn.execute(
                "SELECT id, remote_url FROM learnings WHERE remote_url LIKE 'git@%' OR remote_url LIKE 'https://%' OR remote_url LIKE 'ssh://%'"
            ).fetchall()
            for row in rows:
                normalized = normalize_remote_url(row[1])
                if normalized != row[1]:
                    conn.execute("UPDATE learnings SET remote_url = ? WHERE id = ?", (normalized, row[0]))

            # Migration: normalize remote_urls in events metadata
            rows = conn.execute(
                "SELECT id, metadata FROM semantic_events WHERE json_extract(metadata, '$.remote_url') LIKE 'git@%' OR json_extract(metadata, '$.remote_url') LIKE 'https://%' OR json_extract(metadata, '$.remote_url') LIKE 'ssh://%'"
            ).fetchall()
            for row in rows:
                meta = json.loads(row[1])
                if "remote_url" in meta:
                    normalized = normalize_remote_url(meta["remote_url"])
                    if normalized != meta["remote_url"]:
                        meta["remote_url"] = normalized
                        conn.execute("UPDATE semantic_events SET metadata = ? WHERE id = ?", (json.dumps(meta), row[0]))

            # Note: intelligences table preserved for data retention (not dropped)
            conn.execute("""
                CREATE TABLE IF NOT EXISTS intelligences (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    role TEXT NOT NULL,
                    project TEXT,
                    days INTEGER NOT NULL,
                    event_range_start INTEGER,
                    event_range_end INTEGER,
                    learnings_count INTEGER NOT NULL DEFAULT 0,
                    model TEXT NOT NULL,
                    gemini_input TEXT NOT NULL,
                    gemini_output TEXT NOT NULL,
                    metadata TEXT NOT NULL DEFAULT '{}'
                )
            """)
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_intelligences_role ON intelligences(role)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_intelligences_project ON intelligences(project)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_intelligences_timestamp ON intelligences(timestamp)"
            )

            # Base units table (preserved for data retention)
            conn.execute("""
                CREATE TABLE IF NOT EXISTS base_units (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    project TEXT,
                    session_start TEXT NOT NULL,
                    session_end TEXT NOT NULL,
                    session_gap_minutes INTEGER,
                    event_ids TEXT NOT NULL,
                    learnings_ids TEXT NOT NULL,
                    event_count INTEGER NOT NULL,
                    model TEXT NOT NULL,
                    content TEXT NOT NULL,
                    metadata TEXT NOT NULL DEFAULT '{}',
                    input_chars INTEGER,
                    output_chars INTEGER
                )
            """)
            # Add columns to existing tables (migration)
            for col in ["input_chars INTEGER", "output_chars INTEGER", "session_id TEXT"]:
                try:
                    conn.execute(f"ALTER TABLE base_units ADD COLUMN {col}")
                except sqlite3.OperationalError:
                    pass  # Column already exists
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_base_units_project ON base_units(project)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_base_units_session_end ON base_units(session_end)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_base_units_session_id ON base_units(session_id)"
            )

            # Focus snapshots - user focus at different time scales
            conn.execute("""
                CREATE TABLE IF NOT EXISTS focus_snapshots (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    project TEXT,
                    time_scale TEXT NOT NULL,
                    period_start TEXT NOT NULL,
                    period_end TEXT NOT NULL,
                    focus_summary TEXT NOT NULL,
                    top_topics TEXT NOT NULL,
                    event_count INTEGER NOT NULL,
                    metadata TEXT NOT NULL DEFAULT '{}'
                )
            """)
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_focus_project ON focus_snapshots(project)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_focus_scale ON focus_snapshots(time_scale)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_focus_period ON focus_snapshots(period_end)"
            )

            # Session transcripts - raw JSONL archive with zstd compression
            conn.execute("""
                CREATE TABLE IF NOT EXISTS session_transcripts (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL UNIQUE,
                    parent_session_id TEXT,
                    archived_at TEXT NOT NULL,
                    transcript_path TEXT,
                    size_bytes INTEGER NOT NULL,
                    compressed_bytes INTEGER,
                    content BLOB NOT NULL,
                    metadata TEXT
                )
            """)
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_transcripts_parent ON session_transcripts(parent_session_id)"
            )

            # FTS5 index over event content
            conn.execute("""
                CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(
                    content,
                    content='semantic_events',
                    content_rowid='id'
                )
            """)
            # Auto-sync triggers
            for trigger_sql in [
                """CREATE TRIGGER IF NOT EXISTS events_fts_ai AFTER INSERT ON semantic_events BEGIN
                    INSERT INTO events_fts(rowid, content) VALUES (new.id, new.content);
                END""",
                """CREATE TRIGGER IF NOT EXISTS events_fts_ad AFTER DELETE ON semantic_events BEGIN
                    INSERT INTO events_fts(events_fts, rowid, content) VALUES('delete', old.id, old.content);
                END""",
                """CREATE TRIGGER IF NOT EXISTS events_fts_au AFTER UPDATE ON semantic_events BEGIN
                    INSERT INTO events_fts(events_fts, rowid, content) VALUES('delete', old.id, old.content);
                    INSERT INTO events_fts(rowid, content) VALUES (new.id, new.content);
                END""",
            ]:
                conn.execute(trigger_sql)

            # Transcript scan state for incremental extraction
            conn.execute("""
                CREATE TABLE IF NOT EXISTS transcript_scan_state (
                    transcript_path TEXT PRIMARY KEY,
                    last_byte_offset INTEGER DEFAULT 0,
                    last_entry_uuid TEXT,
                    last_scan_time TEXT,
                    entries_extracted INTEGER DEFAULT 0
                )
            """)

            # Session tags - allows agents to tag their session for later retrieval
            # session_id is resolved lazily at get-time by matching cwd + timestamp
            conn.execute("""
                CREATE TABLE IF NOT EXISTS session_tags (
                    tag TEXT PRIMARY KEY,
                    session_id TEXT,
                    created_at TEXT NOT NULL,
                    cwd TEXT NOT NULL
                )
            """)
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_session_tags_session ON session_tags(session_id)"
            )

            conn.commit()

    @contextmanager
    def connection(self) -> Iterator[sqlite3.Connection]:
        if self._is_memory and self._persistent_conn:
            yield self._persistent_conn
        else:
            conn = sqlite3.connect(self.path)
            conn.row_factory = sqlite3.Row
            if HAS_SQLITE_VEC:
                conn.enable_load_extension(True)
                sqlite_vec.load(conn)
                conn.enable_load_extension(False)
            try:
                yield conn
            finally:
                conn.close()

    def insert_event(
        self,
        event_type: str,
        content: str,
        timestamp: datetime | None = None,
        session_id: str | None = None,
        metadata: dict | None = None,
    ) -> int:
        ts = timestamp or datetime.now(timezone.utc)
        ts_str = ts.isoformat()
        meta_str = json.dumps(metadata or {}, default=str)
        session_id = _normalize_session_id(session_id)

        with self.connection() as conn:
            cursor = conn.execute(
                """
                INSERT INTO semantic_events
                (timestamp, event_type, session_id, content, metadata)
                VALUES (?, ?, ?, ?, ?)
            """,
                (ts_str, event_type, session_id, content, meta_str),
            )
            conn.commit()
            return cursor.lastrowid or 0

    def query_events(
        self,
        event_type: str | list[str] | None = None,
        session_id: str | None = None,
        since: datetime | None = None,
        until: datetime | None = None,
        cwd: str | None = None,
        cwd_prefix: bool = True,
        limit: int = 100,
        offset: int = 0,
        metadata_filter: dict | None = None,
    ) -> list[SemanticEvent]:
        """Query events with optional filters.

        Args:
            event_type: Single type or list of types to include (OR logic).
            cwd: Filter by cwd in metadata. If cwd_prefix=True, matches cwd or subdirs.
            cwd_prefix: If True, match cwd and subdirectories. If False, exact match only.
            metadata_filter: Dict of metadata key-value pairs to filter on (exact match).
                Example: {"phase": "post", "subtype": "summary"}
        """
        query = "SELECT * FROM semantic_events WHERE 1=1"
        params: list = []

        if event_type:
            if isinstance(event_type, list):
                placeholders = ",".join("?" * len(event_type))
                query += f" AND event_type IN ({placeholders})"
                params.extend(event_type)
            else:
                query += " AND event_type = ?"
                params.append(event_type)
        session_id = _normalize_session_id(session_id)
        if session_id:
            query += " AND session_id = ?"
            params.append(session_id)
        if since:
            query += " AND timestamp >= ?"
            params.append(since.isoformat())
        if until:
            query += " AND timestamp < ?"
            params.append(until.isoformat())
        if cwd:
            if cwd_prefix:
                # Match cwd or any subdirectory
                query += " AND (json_extract(metadata, '$.cwd') = ? OR json_extract(metadata, '$.cwd') LIKE ?)"
                params.append(cwd)
                params.append(cwd + "/%")
            else:
                query += " AND json_extract(metadata, '$.cwd') = ?"
                params.append(cwd)
        if metadata_filter:
            for key, value in metadata_filter.items():
                query += f" AND json_extract(metadata, '$.{key}') = ?"
                params.append(value)

        query += " ORDER BY timestamp DESC LIMIT ? OFFSET ?"
        params.append(limit)
        params.append(offset)

        with self.connection() as conn:
            rows = conn.execute(query, params).fetchall()

        return [self._row_to_event(row) for row in rows]

    def search_events(
        self,
        query: str,
        event_type: str | list[str] | None = None,
        limit: int = 20,
        cwd: str | None = None,
        cwd_prefix: bool = True,
        session_id: str | None = None,
    ) -> list[SemanticEvent]:
        """Full-text search over event content via FTS5."""
        # Escape query for FTS5: wrap in quotes to treat as literal phrase,
        # doubling any internal quotes to escape them
        escaped_query = '"' + query.replace('"', '""') + '"'
        sql = """
            SELECT e.* FROM events_fts f
            JOIN semantic_events e ON e.id = f.rowid
            WHERE events_fts MATCH ?
        """
        params: list = [escaped_query]
        if event_type:
            if isinstance(event_type, list):
                placeholders = ",".join("?" * len(event_type))
                sql += f" AND e.event_type IN ({placeholders})"
                params.extend(event_type)
            else:
                sql += " AND e.event_type = ?"
                params.append(event_type)
        if session_id:
            sql += " AND e.session_id = ?"
            params.append(session_id)
        if cwd:
            if cwd_prefix:
                sql += " AND (json_extract(e.metadata, '$.cwd') = ? OR json_extract(e.metadata, '$.cwd') LIKE ?)"
                params.append(cwd)
                params.append(cwd + "/%")
            else:
                sql += " AND json_extract(e.metadata, '$.cwd') = ?"
                params.append(cwd)
        sql += " ORDER BY e.timestamp DESC LIMIT ?"
        params.append(limit)

        with self.connection() as conn:
            rows = conn.execute(sql, params).fetchall()
        return [self._row_to_event(row) for row in rows]

    def rebuild_fts(self) -> int:
        """Rebuild FTS index from scratch. Returns rows indexed."""
        with self.connection() as conn:
            conn.execute("INSERT INTO events_fts(events_fts) VALUES('rebuild')")
            conn.commit()
            row = conn.execute("SELECT count(*) FROM events_fts").fetchone()
            return row[0]

    def get_event_by_id(self, event_id: int) -> SemanticEvent | None:
        with self.connection() as conn:
            row = conn.execute(
                "SELECT * FROM semantic_events WHERE id = ?",
                (event_id,),
            ).fetchone()
        return self._row_to_event(row) if row else None

    def query_events_since_id(
        self,
        since_id: int,
        event_type: str | None = None,
        cwd: str | None = None,
        cwd_prefix: bool = True,
        limit: int = 100,
    ) -> list[SemanticEvent]:
        """Query events with id > since_id (for delta/incremental queries).

        Args:
            since_id: Return events with id strictly greater than this.
            event_type: Filter by event type.
            cwd: Filter by cwd in metadata.
            cwd_prefix: If True, match cwd and subdirectories.
        """
        query = "SELECT * FROM semantic_events WHERE id > ?"
        params: list = [since_id]

        if event_type:
            query += " AND event_type = ?"
            params.append(event_type)
        if cwd:
            if cwd_prefix:
                query += " AND (json_extract(metadata, '$.cwd') = ? OR json_extract(metadata, '$.cwd') LIKE ?)"
                params.append(cwd)
                params.append(cwd + "/%")
            else:
                query += " AND json_extract(metadata, '$.cwd') = ?"
                params.append(cwd)

        query += " ORDER BY id ASC LIMIT ?"
        params.append(limit)

        with self.connection() as conn:
            rows = conn.execute(query, params).fetchall()

        return [self._row_to_event(row) for row in rows]

    def get_latest_event(
        self, event_type: str, session_id: str | None = None
    ) -> SemanticEvent | None:
        query = "SELECT * FROM semantic_events WHERE event_type = ?"
        params: list = [event_type]

        session_id = _normalize_session_id(session_id)
        if session_id:
            query += " AND session_id = ?"
            params.append(session_id)

        query += " ORDER BY timestamp DESC LIMIT 1"

        with self.connection() as conn:
            row = conn.execute(query, params).fetchone()

        return self._row_to_event(row) if row else None

    def count_events(self, event_type: str | None = None) -> int:
        if event_type:
            query = "SELECT COUNT(*) FROM semantic_events WHERE event_type = ?"
            params = (event_type,)
        else:
            query = "SELECT COUNT(*) FROM semantic_events"
            params = ()

        with self.connection() as conn:
            row = conn.execute(query, params).fetchone()
        return row[0] if row else 0

    def event_counts_by_type(self) -> dict[str, int]:
        with self.connection() as conn:
            rows = conn.execute("""
                SELECT event_type, COUNT(*) as cnt
                FROM semantic_events
                GROUP BY event_type
                ORDER BY cnt DESC
            """).fetchall()
        return {row["event_type"]: row["cnt"] for row in rows}

    def get_distinct_event_types(self) -> list[str]:
        with self.connection() as conn:
            rows = conn.execute("""
                SELECT DISTINCT event_type FROM semantic_events ORDER BY event_type
            """).fetchall()
        return [row["event_type"] for row in rows]

    def get_repos_with_stats(self) -> list[dict]:
        """Get repos with event counts and last activity.

        Returns list of dicts with: remote_url, repo_name, event_count, last_event_at
        Normalizes URLs so SSH/HTTPS variants are combined.
        """
        with self.connection() as conn:
            rows = conn.execute("""
                SELECT
                    json_extract(metadata, '$.remote_url') as remote_url,
                    json_extract(metadata, '$.repo_name') as repo_name,
                    COUNT(*) as event_count,
                    MAX(timestamp) as last_event_at
                FROM semantic_events
                WHERE json_extract(metadata, '$.remote_url') IS NOT NULL
                GROUP BY json_extract(metadata, '$.remote_url')
                ORDER BY last_event_at DESC
            """).fetchall()

        # Normalize URLs and combine duplicates
        from .git import normalize_remote_url

        combined: dict[str, dict] = {}
        for row in rows:
            raw_url = row["remote_url"]
            normalized = normalize_remote_url(raw_url) if raw_url else raw_url

            try:
                ts = datetime.fromisoformat(row["last_event_at"].replace("Z", "+00:00"))
            except (ValueError, AttributeError):
                ts = datetime.now(timezone.utc)

            if normalized in combined:
                # Combine with existing
                combined[normalized]["event_count"] += row["event_count"]
                if ts > combined[normalized]["last_event_at"]:
                    combined[normalized]["last_event_at"] = ts
            else:
                combined[normalized] = {
                    "remote_url": normalized,
                    "repo_name": row["repo_name"],
                    "event_count": row["event_count"],
                    "last_event_at": ts,
                }

        # Sort by last_event_at descending
        return sorted(combined.values(), key=lambda x: x["last_event_at"], reverse=True)

    def get_session_ids(self) -> list[str]:
        """Get session IDs ordered by most recent activity. Lightweight - no per-session queries."""
        with self.connection() as conn:
            rows = conn.execute("""
                SELECT session_id, MAX(timestamp) as last_ts
                FROM semantic_events
                WHERE session_id IS NOT NULL AND session_id != ''
                GROUP BY session_id
                ORDER BY last_ts DESC
            """).fetchall()
        return [row["session_id"] for row in rows]

    def get_orphaned_sessions(self, stale_minutes: int = 30) -> list[str]:
        """Find sessions with SESSION_START but no SESSION_END and stale activity."""
        cutoff = (datetime.now(timezone.utc) - timedelta(minutes=stale_minutes)).isoformat()
        with self.connection() as conn:
            rows = conn.execute("""
                SELECT DISTINCT e.session_id
                FROM semantic_events e
                WHERE e.event_type = 'session_start'
                  AND e.session_id IS NOT NULL
                  AND NOT EXISTS (
                      SELECT 1 FROM semantic_events e2
                      WHERE e2.session_id = e.session_id
                        AND e2.event_type = 'session_end'
                  )
                  AND (
                      SELECT MAX(e3.timestamp) FROM semantic_events e3
                      WHERE e3.session_id = e.session_id
                  ) < ?
            """, (cutoff,)).fetchall()
        return [row[0] for row in rows]

    def get_sessions_with_info(
        self,
        cwd: str | None = None,
        cwd_prefix: bool = True,
        limit: int | None = None,
    ) -> list[tuple[str, datetime, int, str | None, str | None, bool]]:
        """Get sessions with last event timestamp, event count, cwd, start_cwd, and active status.

        Args:
            cwd: Filter sessions by start_cwd (where session was initiated).
                 If None, returns all sessions.
            cwd_prefix: If True, match cwd and subdirectories. If False, exact match only.
            limit: Maximum number of sessions to return. If None, returns all.

        Returns:
            List of tuples: (session_id, last_ts, event_count, cwd, start_cwd, is_active)
        """
        # Build query with CTE to compute start_cwd and filter at DB level
        query = """
            WITH session_with_start_cwd AS (
                SELECT
                    s.session_id,
                    s.last_ts,
                    s.cnt,
                    (
                        SELECT json_extract(metadata, '$.cwd')
                        FROM semantic_events
                        WHERE session_id = s.session_id
                          AND metadata LIKE '%"cwd"%'
                        ORDER BY timestamp ASC
                        LIMIT 1
                    ) as start_cwd
                FROM (
                    SELECT
                        session_id,
                        MAX(timestamp) as last_ts,
                        COUNT(*) as cnt
                    FROM semantic_events
                    WHERE session_id IS NOT NULL AND session_id != ''
                    GROUP BY session_id
                ) s
            )
            SELECT * FROM session_with_start_cwd
        """
        params: list = []

        if cwd:
            if cwd_prefix:
                query += " WHERE (start_cwd = ? OR start_cwd LIKE ?)"
                params.append(cwd)
                params.append(cwd + "/%")
            else:
                query += " WHERE start_cwd = ?"
                params.append(cwd)

        query += " ORDER BY last_ts DESC"

        if limit:
            query += " LIMIT ?"
            params.append(limit)

        with self.connection() as conn:
            rows = conn.execute(query, params).fetchall()

        result = []
        for row in rows:
            try:
                ts = datetime.fromisoformat(row["last_ts"].replace("Z", "+00:00"))
            except (ValueError, AttributeError):
                ts = datetime.now(timezone.utc)
            session_id = row["session_id"]
            start_cwd = row["start_cwd"]
            current_cwd = self._get_session_cwd(session_id)
            is_active = self._is_session_active(session_id, ts)
            result.append((session_id, ts, row["cnt"], current_cwd, start_cwd, is_active))
        return result

    def _get_session_cwd(self, session_id: str) -> str | None:
        """Get the most recent cwd for a session from event metadata."""
        with self.connection() as conn:
            row = conn.execute("""
                SELECT metadata FROM semantic_events
                WHERE session_id = ? AND metadata LIKE '%"cwd"%'
                ORDER BY timestamp DESC LIMIT 1
            """, (session_id,)).fetchone()
        if row:
            try:
                meta = json.loads(row["metadata"])
                return meta.get("cwd")
            except (json.JSONDecodeError, TypeError):
                pass
        return None

    def _get_session_start_cwd(self, session_id: str) -> str | None:
        """Get the cwd from the session's first event (for resuming)."""
        with self.connection() as conn:
            row = conn.execute("""
                SELECT metadata FROM semantic_events
                WHERE session_id = ? AND metadata LIKE '%"cwd"%'
                ORDER BY timestamp ASC LIMIT 1
            """, (session_id,)).fetchone()
        if row:
            try:
                meta = json.loads(row["metadata"])
                return meta.get("cwd")
            except (json.JSONDecodeError, TypeError):
                pass
        return None

    def _is_session_active(self, session_id: str, last_ts: datetime) -> bool:
        """Check if session is active (last lifecycle event is not session_end, and recent activity)."""
        with self.connection() as conn:
            row = conn.execute("""
                SELECT event_type FROM semantic_events
                WHERE session_id = ? AND event_type IN ('session_start', 'session_end')
                ORDER BY timestamp DESC LIMIT 1
            """, (session_id,)).fetchone()
        if row and row["event_type"] == "session_end":
            return False
        # Consider active if last event within 30 minutes
        now = datetime.now(timezone.utc)
        return (now - last_ts).total_seconds() < 1800

    def get_session_extended_stats(self, session_id: str) -> dict:
        """Get extended stats for a session including repo, branches, line counts, etc."""
        with self.connection() as conn:
            # Get event type counts
            type_counts = conn.execute("""
                SELECT event_type, COUNT(*) as cnt
                FROM semantic_events
                WHERE session_id = ?
                GROUP BY event_type
            """, (session_id,)).fetchall()

            counts_by_type = {row["event_type"]: row["cnt"] for row in type_counts}

            # Get unique repos and branches from metadata
            meta_rows = conn.execute("""
                SELECT DISTINCT metadata FROM semantic_events
                WHERE session_id = ? AND metadata LIKE '%"remote_url"%'
            """, (session_id,)).fetchall()

            repos = set()
            branches = set()
            for row in meta_rows:
                try:
                    meta = json.loads(row["metadata"])
                    if meta.get("remote_url"):
                        # Normalize URL for SSH/HTTPS equivalence
                        repos.add(normalize_remote_url(meta["remote_url"]))
                    if meta.get("branch"):
                        branches.add(meta["branch"])
                except (json.JSONDecodeError, TypeError):
                    pass

            # Parse line counts and file paths from file_diff events
            diff_rows = conn.execute("""
                SELECT content, metadata FROM semantic_events
                WHERE session_id = ? AND event_type = 'file_diff'
            """, (session_id,)).fetchall()

            lines_added = 0
            lines_removed = 0
            files_modified = set()
            for row in diff_rows:
                content = row["content"] or ""
                for line in content.split("\n"):
                    if line.startswith("+") and not line.startswith("+++"):
                        lines_added += 1
                    elif line.startswith("-") and not line.startswith("---"):
                        lines_removed += 1
                # Extract file path from metadata
                try:
                    meta = json.loads(row["metadata"])
                    if meta.get("file_path"):
                        files_modified.add(meta["file_path"])
                except (json.JSONDecodeError, TypeError):
                    pass

            # Get first user prompt content
            first_prompt_row = conn.execute("""
                SELECT content FROM semantic_events
                WHERE session_id = ? AND event_type = 'user_prompt'
                ORDER BY timestamp ASC LIMIT 1
            """, (session_id,)).fetchone()

            first_prompt = None
            if first_prompt_row:
                content = first_prompt_row["content"] or ""
                # Truncate to first 120 chars, first line only
                first_line = content.split("\n")[0].strip()
                first_prompt = first_line[:120] + ("..." if len(first_line) > 120 else "")

            # Get session time bounds
            time_bounds = conn.execute("""
                SELECT MIN(timestamp) as started, MAX(timestamp) as ended
                FROM semantic_events
                WHERE session_id = ?
            """, (session_id,)).fetchone()

            started_at = None
            duration_minutes = None
            if time_bounds and time_bounds["started"]:
                try:
                    start_ts = datetime.fromisoformat(time_bounds["started"].replace("Z", "+00:00"))
                    end_ts = datetime.fromisoformat(time_bounds["ended"].replace("Z", "+00:00"))
                    started_at = start_ts
                    duration_minutes = int((end_ts - start_ts).total_seconds() / 60)
                except (ValueError, AttributeError):
                    pass

        # Get learnings count
        learnings_count = 0
        with self.connection() as conn:
            row = conn.execute("""
                SELECT COUNT(*) as cnt FROM learnings WHERE session_id = ?
            """, (session_id,)).fetchone()
            if row:
                learnings_count = row["cnt"]

        return {
            "repos": list(repos),
            "branches": list(branches),
            "lines_added": lines_added,
            "lines_removed": lines_removed,
            "learnings_count": learnings_count,
            "tool_use_count": counts_by_type.get("tool_use", 0),
            "file_diff_count": counts_by_type.get("file_diff", 0),
            "compaction_count": counts_by_type.get("compaction", 0),
            "user_prompt_count": counts_by_type.get("user_prompt", 0),
            "first_prompt": first_prompt,
            "started_at": started_at,
            "duration_minutes": duration_minutes,
            "files_modified": sorted(files_modified)[:10],  # Limit to 10 files
        }

    def insert_learning(
        self,
        content: str,
        session_id: str | None = None,
        cwd: str | None = None,
        zellij_session: str | None = None,
        timestamp: datetime | None = None,
        repo_root: str | None = None,
        remote_url: str | None = None,
        repo_name: str | None = None,
        branch: str | None = None,
        is_worktree: bool | None = None,
    ) -> int:
        ts = timestamp or datetime.now(timezone.utc)
        ts_str = ts.isoformat()
        session_id = _normalize_session_id(session_id)
        remote_url = normalize_remote_url(remote_url) if remote_url else None
        is_worktree_int = 1 if is_worktree else (0 if is_worktree is False else None)

        with self.connection() as conn:
            cursor = conn.execute(
                """INSERT INTO learnings
                (timestamp, session_id, cwd, zellij_session, content,
                 repo_root, remote_url, repo_name, branch, is_worktree)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (ts_str, session_id, cwd, zellij_session, content,
                 repo_root, remote_url, repo_name, branch, is_worktree_int),
            )
            conn.commit()
            return cursor.lastrowid or 0

    def query_learnings(
        self,
        cwd: str | None = None,
        cwd_prefix: bool = True,
        zellij_session: str | None = None,
        since: datetime | None = None,
        limit: int = 50,
        repo_root: str | None = None,
        remote_url: str | None = None,
        session_id: str | None = None,
    ) -> list[Learning]:
        """Query learnings with optional filters.

        Args:
            cwd: Filter by cwd. If cwd_prefix=True, matches cwd or subdirs.
            cwd_prefix: If True, match cwd and subdirectories. If False, exact match only.
            since: Only return learnings after this timestamp.
            repo_root: Filter by repo root (for worktree reconciliation).
            remote_url: Filter by remote URL (canonical repo identifier).
            session_id: Filter by session ID.
        """
        query = "SELECT * FROM learnings WHERE 1=1"
        params: list[str | int] = []

        if session_id:
            query += " AND session_id = ?"
            params.append(session_id)
        if cwd:
            if cwd_prefix:
                query += " AND (cwd = ? OR cwd LIKE ?)"
                params.append(cwd)
                params.append(cwd + "/%")
            else:
                query += " AND cwd = ?"
                params.append(cwd)
        if zellij_session:
            query += " AND zellij_session = ?"
            params.append(zellij_session)
        if since:
            query += " AND timestamp > ?"
            params.append(since.isoformat())
        if repo_root:
            query += " AND repo_root = ?"
            params.append(repo_root)
        if remote_url:
            query += " AND remote_url = ?"
            params.append(remote_url)

        query += " ORDER BY timestamp DESC LIMIT ?"
        params.append(limit)

        with self.connection() as conn:
            rows = conn.execute(query, params).fetchall()

        return [self._row_to_learning(row) for row in rows]

    def count_learnings(
        self,
        cwd: str | None = None,
        cwd_prefix: bool = True,
        zellij_session: str | None = None,
        since: datetime | None = None,
        repo_root: str | None = None,
        remote_url: str | None = None,
    ) -> int:
        """Count learnings with optional filters (efficient COUNT query).

        Args:
            cwd: Filter by cwd. If cwd_prefix=True, matches cwd or subdirs.
            cwd_prefix: If True, match cwd and subdirectories. If False, exact match only.
            since: Only count learnings after this timestamp.
            repo_root: Filter by repo root (for worktree reconciliation).
            remote_url: Filter by remote URL (canonical repo identifier).
        """
        query = "SELECT COUNT(*) FROM learnings WHERE 1=1"
        params: list[str] = []

        if cwd:
            if cwd_prefix:
                query += " AND (cwd = ? OR cwd LIKE ?)"
                params.append(cwd)
                params.append(cwd + "/%")
            else:
                query += " AND cwd = ?"
                params.append(cwd)
        if zellij_session:
            query += " AND zellij_session = ?"
            params.append(zellij_session)
        if since:
            query += " AND timestamp > ?"
            params.append(since.isoformat())
        if repo_root:
            query += " AND repo_root = ?"
            params.append(repo_root)
        if remote_url:
            query += " AND remote_url = ?"
            params.append(remote_url)

        with self.connection() as conn:
            row = conn.execute(query, params).fetchone()
        return row[0] if row else 0

    def _row_to_learning(self, row: sqlite3.Row) -> Learning:
        ts_str = row["timestamp"]
        try:
            ts = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
        except (ValueError, AttributeError):
            ts = datetime.now(timezone.utc)

        # Handle git context fields (may be missing in older rows)
        def get_field(name: str) -> str | None:
            try:
                return row[name]
            except (KeyError, IndexError):
                return None

        is_worktree_val = get_field("is_worktree")
        is_worktree = bool(is_worktree_val) if is_worktree_val is not None else None

        return Learning(
            id=row["id"],
            timestamp=ts,
            session_id=row["session_id"],
            cwd=row["cwd"],
            zellij_session=row["zellij_session"],
            content=row["content"],
            repo_root=get_field("repo_root"),
            remote_url=get_field("remote_url"),
            repo_name=get_field("repo_name"),
            branch=get_field("branch"),
            is_worktree=is_worktree,
        )

    def _row_to_event(self, row: sqlite3.Row) -> SemanticEvent:
        ts_str = row["timestamp"]
        try:
            ts = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
        except (ValueError, AttributeError):
            ts = datetime.now(timezone.utc)

        meta_str = row["metadata"]
        try:
            metadata = json.loads(meta_str)
        except (json.JSONDecodeError, TypeError):
            metadata = {}

        return SemanticEvent(
            id=row["id"],
            timestamp=ts,
            event_type=row["event_type"],
            session_id=row["session_id"],
            content=row["content"],
            metadata=metadata,
        )

    # -------------------------------------------------------------------------
    # Focus snapshot methods
    # -------------------------------------------------------------------------

    def insert_focus_snapshot(
        self,
        project: str | None,
        time_scale: str,
        period_start: datetime,
        period_end: datetime,
        focus_summary: str,
        top_topics: list[str],
        event_count: int,
        metadata: dict | None = None,
        timestamp: datetime | None = None,
    ) -> int:
        ts = timestamp or datetime.now(timezone.utc)
        ts_str = ts.isoformat()
        meta_str = json.dumps(metadata or {}, default=str)
        topics_str = json.dumps(top_topics)

        with self.connection() as conn:
            cursor = conn.execute(
                """
                INSERT INTO focus_snapshots
                (timestamp, project, time_scale, period_start, period_end,
                 focus_summary, top_topics, event_count, metadata)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (ts_str, project, time_scale, period_start.isoformat(),
                 period_end.isoformat(), focus_summary, topics_str, event_count, meta_str),
            )
            conn.commit()
            return cursor.lastrowid or 0

    def get_focus_snapshot(
        self,
        project: str | None,
        time_scale: str,
        period_start: datetime,
        period_end: datetime,
    ) -> FocusSnapshot | None:
        """Get existing focus snapshot for a specific period."""
        query = """
            SELECT * FROM focus_snapshots
            WHERE time_scale = ? AND period_start = ? AND period_end = ?
        """
        params: list = [time_scale, period_start.isoformat(), period_end.isoformat()]

        if project:
            query += " AND project = ?"
            params.append(project)
        else:
            query += " AND project IS NULL"

        with self.connection() as conn:
            row = conn.execute(query, params).fetchone()

        return self._row_to_focus_snapshot(row) if row else None

    def query_focus_snapshots(
        self,
        project: str | None = None,
        time_scale: str | None = None,
        limit: int = 50,
    ) -> list[FocusSnapshot]:
        query = "SELECT * FROM focus_snapshots WHERE 1=1"
        params: list = []

        if project:
            query += " AND project = ?"
            params.append(project)
        if time_scale:
            query += " AND time_scale = ?"
            params.append(time_scale)

        query += " ORDER BY period_end DESC LIMIT ?"
        params.append(limit)

        with self.connection() as conn:
            rows = conn.execute(query, params).fetchall()

        return [self._row_to_focus_snapshot(row) for row in rows]

    def _row_to_focus_snapshot(self, row: sqlite3.Row) -> FocusSnapshot:
        ts_str = row["timestamp"]
        try:
            ts = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
        except (ValueError, AttributeError):
            ts = datetime.now(timezone.utc)

        period_start = datetime.fromisoformat(row["period_start"].replace("Z", "+00:00"))
        period_end = datetime.fromisoformat(row["period_end"].replace("Z", "+00:00"))

        try:
            top_topics = json.loads(row["top_topics"])
        except (json.JSONDecodeError, TypeError):
            top_topics = []

        try:
            metadata = json.loads(row["metadata"])
        except (json.JSONDecodeError, TypeError):
            metadata = {}

        return FocusSnapshot(
            id=row["id"],
            timestamp=ts,
            project=row["project"],
            time_scale=row["time_scale"],
            period_start=period_start,
            period_end=period_end,
            focus_summary=row["focus_summary"],
            top_topics=top_topics,
            event_count=row["event_count"],
            metadata=metadata,
        )

    def get_latest_focus_by_scale(self, time_scale: str) -> FocusSnapshot | None:
        """Get the most recent focus snapshot for a given time scale."""
        with self.connection() as conn:
            row = conn.execute(
                """
                SELECT * FROM focus_snapshots
                WHERE time_scale = ?
                ORDER BY timestamp DESC
                LIMIT 1
                """,
                (time_scale,),
            ).fetchone()
        return self._row_to_focus_snapshot(row) if row else None

    def get_stale_focus_scales(self) -> list[str]:
        """
        Return list of time scales that need regeneration.

        A scale is stale if:
        - hour: last focus was in a previous hour
        - day: last focus was on a previous day
        - week: last focus was in a previous week
        """
        now = datetime.now(timezone.utc)
        stale: list[str] = []

        for scale in ("hour", "day", "week"):
            latest = self.get_latest_focus_by_scale(scale)
            if latest is None:
                stale.append(scale)
                continue

            ts = latest.timestamp
            if scale == "hour":
                # Stale if different hour (compare year, month, day, hour)
                if (ts.year, ts.month, ts.day, ts.hour) != (now.year, now.month, now.day, now.hour):
                    stale.append(scale)
            elif scale == "day":
                # Stale if different day
                if ts.date() != now.date():
                    stale.append(scale)
            elif scale == "week":
                # Stale if different ISO week
                if ts.isocalendar()[:2] != now.isocalendar()[:2]:
                    stale.append(scale)

        return stale

    # -------------------------------------------------------------------------
    # Transcript scan state (incremental extraction)
    # -------------------------------------------------------------------------

    def get_scan_state(self, transcript_path: str) -> dict | None:
        """Get scan state for a transcript."""
        with self.connection() as conn:
            row = conn.execute(
                "SELECT * FROM transcript_scan_state WHERE transcript_path = ?",
                (str(transcript_path),)
            ).fetchone()
            return dict(row) if row else None

    def update_scan_state(
        self,
        transcript_path: str,
        last_byte_offset: int,
        last_entry_uuid: str | None,
        entries_extracted: int,
    ) -> None:
        """Update scan state for a transcript."""
        with self.connection() as conn:
            conn.execute("""
                INSERT INTO transcript_scan_state
                    (transcript_path, last_byte_offset, last_entry_uuid, last_scan_time, entries_extracted)
                VALUES (?, ?, ?, datetime('now'), ?)
                ON CONFLICT(transcript_path) DO UPDATE SET
                    last_byte_offset = excluded.last_byte_offset,
                    last_entry_uuid = excluded.last_entry_uuid,
                    last_scan_time = excluded.last_scan_time,
                    entries_extracted = transcript_scan_state.entries_extracted + excluded.entries_extracted
            """, (str(transcript_path), last_byte_offset, last_entry_uuid, entries_extracted))
            conn.commit()

    # -------------------------------------------------------------------------
    # Compaction helpers
    # -------------------------------------------------------------------------

    def count_events_in_range(
        self,
        since: datetime,
        until: datetime | None = None,
        cwd: str | None = None,
        event_type: str | None = None,
    ) -> int:
        """Count events in a time range (for threshold inference)."""
        query = "SELECT COUNT(*) FROM semantic_events WHERE timestamp >= ?"
        params: list = [since.isoformat()]

        if until:
            query += " AND timestamp <= ?"
            params.append(until.isoformat())
        if event_type:
            query += " AND event_type = ?"
            params.append(event_type)
        if cwd:
            query += " AND (json_extract(metadata, '$.cwd') = ? OR json_extract(metadata, '$.cwd') LIKE ?)"
            params.append(cwd)
            params.append(cwd + "/%")

        with self.connection() as conn:
            row = conn.execute(query, params).fetchone()

        return row[0] if row else 0

    # -------------------------------------------------------------------------
    # Semantic search (embeddings) - requires sqlite-vec
    # -------------------------------------------------------------------------

    def insert_learning_embedding(self, learning_id: int, embedding: list[float]) -> None:
        """Store embedding for a learning in the vector table.

        Args:
            learning_id: ID of the learning (used as rowid in vec_learnings)
            embedding: The embedding vector (3072 floats for gemini-embedding-001)
        """
        if not HAS_SQLITE_VEC:
            return
        from reclaude.embeddings import embedding_to_blob

        blob = embedding_to_blob(embedding)
        with self.connection() as conn:
            # Use INSERT OR REPLACE to handle re-embedding
            conn.execute(
                "INSERT OR REPLACE INTO vec_learnings(rowid, embedding) VALUES (?, ?)",
                (learning_id, blob),
            )
            conn.commit()

    def has_learning_embedding(self, learning_id: int) -> bool:
        """Check if a learning has an embedding stored."""
        if not HAS_SQLITE_VEC:
            return False
        with self.connection() as conn:
            row = conn.execute(
                "SELECT 1 FROM vec_learnings WHERE rowid = ?",
                (learning_id,),
            ).fetchone()
        return row is not None

    def get_unembedded_learning_ids(self, limit: int = 100) -> list[int]:
        """Get IDs of learnings that don't have embeddings yet."""
        if not HAS_SQLITE_VEC:
            return []
        with self.connection() as conn:
            rows = conn.execute(
                """
                SELECT l.id FROM learnings l
                LEFT JOIN vec_learnings v ON l.id = v.rowid
                WHERE v.rowid IS NULL
                ORDER BY l.id
                LIMIT ?
                """,
                (limit,),
            ).fetchall()
        return [row[0] for row in rows]

    def query_learnings_semantic(
        self,
        query_embedding: list[float],
        limit: int = 10,
        distance_threshold: float | None = None,
        remote_url: str | None = None,
        repo_root: str | None = None,
        since: datetime | None = None,
    ) -> list[tuple[Learning, float]]:
        """
        Query learnings by semantic similarity.

        Args:
            query_embedding: The embedding vector to search for
            limit: Maximum number of results
            distance_threshold: Optional max distance (lower = more similar)
            remote_url: Optional filter by repo URL
            repo_root: Optional filter by repo root
            since: Optional filter for results after this timestamp

        Returns:
            List of (Learning, distance) tuples sorted by similarity
        """
        if not HAS_SQLITE_VEC:
            return []
        from reclaude.embeddings import embedding_to_blob

        query_blob = embedding_to_blob(query_embedding)

        # Build query with optional filters
        # Note: vec0 match queries return rowid and distance
        base_query = """
            SELECT
                v.rowid as learning_id,
                v.distance
            FROM vec_learnings v
            WHERE v.embedding MATCH ?
            AND k = ?
        """

        with self.connection() as conn:
            # First get vector matches
            vec_rows = conn.execute(base_query, (query_blob, limit * 2)).fetchall()

            if not vec_rows:
                return []

            # Get the actual learnings and apply filters
            learning_ids = [row["learning_id"] for row in vec_rows]
            distances = {row["learning_id"]: row["distance"] for row in vec_rows}

            # Build filter query
            placeholders = ",".join("?" * len(learning_ids))
            filter_query = f"SELECT * FROM learnings WHERE id IN ({placeholders})"
            params: list = list(learning_ids)

            if remote_url:
                filter_query += " AND remote_url = ?"
                params.append(remote_url)
            if repo_root:
                filter_query += " AND repo_root = ?"
                params.append(repo_root)
            if since:
                filter_query += " AND timestamp >= ?"
                params.append(since.isoformat())

            rows = conn.execute(filter_query, params).fetchall()

        # Combine results with distances
        results: list[tuple[Learning, float]] = []
        for row in rows:
            learning = self._row_to_learning(row)
            dist = distances.get(learning.id, float("inf"))

            # Apply distance threshold
            if distance_threshold is not None and dist > distance_threshold:
                continue

            results.append((learning, dist))

        # Sort by distance and limit
        results.sort(key=lambda x: x[1])
        return results[:limit]

    def count_learning_embeddings(self) -> int:
        """Count how many learnings have embeddings."""
        if not HAS_SQLITE_VEC:
            return 0
        with self.connection() as conn:
            row = conn.execute("SELECT COUNT(*) FROM vec_learnings").fetchone()
        return row[0] if row else 0

    # -------------------------------------------------------------------------
    # Event embeddings (user_prompt, plan, assistant, thinking, compaction)
    # -------------------------------------------------------------------------

    EMBEDDABLE_EVENT_TYPES = (
        "user_prompt", "plan", "plan_file", "assistant", "thinking", "compaction"
    )

    def insert_event_embedding(self, event_id: int, embedding: list[float]) -> None:
        """Store embedding for an event in the vector table."""
        if not HAS_SQLITE_VEC:
            return
        from reclaude.embeddings import embedding_to_blob

        blob = embedding_to_blob(embedding)
        with self.connection() as conn:
            conn.execute(
                "INSERT OR REPLACE INTO vec_events(rowid, embedding) VALUES (?, ?)",
                (event_id, blob),
            )
            conn.commit()

    def get_unembedded_event_ids(
        self,
        event_types: tuple[str, ...] | None = None,
        limit: int = 100,
    ) -> list[int]:
        """Get IDs of events that don't have embeddings yet."""
        if not HAS_SQLITE_VEC:
            return []

        types = event_types or self.EMBEDDABLE_EVENT_TYPES
        placeholders = ",".join("?" * len(types))

        with self.connection() as conn:
            rows = conn.execute(
                f"""
                SELECT e.id FROM semantic_events e
                LEFT JOIN vec_events v ON e.id = v.rowid
                WHERE v.rowid IS NULL
                AND e.event_type IN ({placeholders})
                ORDER BY e.id
                LIMIT ?
                """,
                (*types, limit),
            ).fetchall()
        return [row[0] for row in rows]

    def query_events_semantic(
        self,
        query_embedding: list[float],
        event_types: tuple[str, ...] | None = None,
        limit: int = 10,
        distance_threshold: float | None = None,
        remote_url: str | None = None,
        since: datetime | None = None,
    ) -> list[tuple[SemanticEvent, float]]:
        """Query events by semantic similarity."""
        if not HAS_SQLITE_VEC:
            return []
        from reclaude.embeddings import embedding_to_blob

        query_blob = embedding_to_blob(query_embedding)
        types = event_types or self.EMBEDDABLE_EVENT_TYPES

        base_query = """
            SELECT v.rowid as event_id, v.distance
            FROM vec_events v
            WHERE v.embedding MATCH ?
            AND k = ?
        """

        with self.connection() as conn:
            vec_rows = conn.execute(base_query, (query_blob, limit * 3)).fetchall()

            if not vec_rows:
                return []

            event_ids = [row["event_id"] for row in vec_rows]
            distances = {row["event_id"]: row["distance"] for row in vec_rows}

            # Filter by event type and optional remote_url
            placeholders = ",".join("?" * len(event_ids))
            type_placeholders = ",".join("?" * len(types))
            filter_query = f"""
                SELECT * FROM semantic_events
                WHERE id IN ({placeholders})
                AND event_type IN ({type_placeholders})
            """
            params: list = list(event_ids) + list(types)

            if remote_url:
                filter_query += " AND json_extract(metadata, '$.remote_url') = ?"
                params.append(remote_url)
            if since:
                filter_query += " AND timestamp >= ?"
                params.append(since.isoformat())

            rows = conn.execute(filter_query, params).fetchall()

        results: list[tuple[SemanticEvent, float]] = []
        for row in rows:
            event = self._row_to_event(row)
            dist = distances.get(event.id, float("inf"))

            if distance_threshold is not None and dist > distance_threshold:
                continue

            results.append((event, dist))

        results.sort(key=lambda x: x[1])
        return results[:limit]

    def count_event_embeddings(self, event_types: tuple[str, ...] | None = None) -> int:
        """Count how many events have embeddings."""
        if not HAS_SQLITE_VEC:
            return 0

        types = event_types or self.EMBEDDABLE_EVENT_TYPES
        placeholders = ",".join("?" * len(types))

        with self.connection() as conn:
            row = conn.execute(
                f"""
                SELECT COUNT(*) FROM vec_events v
                JOIN semantic_events e ON v.rowid = e.id
                WHERE e.event_type IN ({placeholders})
                """,
                types,
            ).fetchone()
        return row[0] if row else 0

    # -------------------------------------------------------------------------
    # Session transcript archival
    # -------------------------------------------------------------------------

    def upsert_transcript(
        self,
        session_id: str,
        content: bytes,
        size_bytes: int,
        compressed_bytes: int,
        transcript_path: str | None = None,
        parent_session_id: str | None = None,
        metadata: dict | None = None,
        timestamp: datetime | None = None,
    ) -> tuple[int, bool]:
        """
        Insert or update a transcript archive.

        Returns:
            (row_id, is_new) - row_id and whether this was a new insert
        """
        ts = timestamp or datetime.now(timezone.utc)
        ts_str = ts.isoformat()
        meta_str = json.dumps(metadata or {}, default=str)

        with self.connection() as conn:
            # Check if exists and if size changed
            existing = conn.execute(
                "SELECT id, size_bytes FROM session_transcripts WHERE session_id = ?",
                (session_id,),
            ).fetchone()

            if existing is None:
                # New transcript
                cursor = conn.execute(
                    """
                    INSERT INTO session_transcripts
                    (session_id, parent_session_id, archived_at, transcript_path,
                     size_bytes, compressed_bytes, content, metadata)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                    (session_id, parent_session_id, ts_str, transcript_path,
                     size_bytes, compressed_bytes, content, meta_str),
                )
                conn.commit()
                return cursor.lastrowid or 0, True

            if existing["size_bytes"] != size_bytes:
                # Size changed - update
                conn.execute(
                    """
                    UPDATE session_transcripts
                    SET archived_at = ?, transcript_path = ?, size_bytes = ?,
                        compressed_bytes = ?, content = ?, metadata = ?
                    WHERE session_id = ?
                    """,
                    (ts_str, transcript_path, size_bytes, compressed_bytes,
                     content, meta_str, session_id),
                )
                conn.commit()
                return existing["id"], False

            # No change
            return existing["id"], False

    def get_transcript(self, session_id: str) -> SessionTranscript | None:
        """Get transcript metadata by session_id (without content blob)."""
        with self.connection() as conn:
            row = conn.execute(
                """
                SELECT id, session_id, parent_session_id, archived_at,
                       transcript_path, size_bytes, compressed_bytes, metadata
                FROM session_transcripts WHERE session_id = ?
                """,
                (session_id,),
            ).fetchone()
        return self._row_to_transcript(row) if row else None

    def get_transcript_content(self, session_id: str) -> bytes | None:
        """Get raw compressed content blob for a transcript."""
        with self.connection() as conn:
            row = conn.execute(
                "SELECT content FROM session_transcripts WHERE session_id = ?",
                (session_id,),
            ).fetchone()
        return row["content"] if row else None

    def query_transcripts(
        self,
        parent_session_id: str | None = None,
        include_subagents: bool = True,
        limit: int = 100,
    ) -> list[SessionTranscript]:
        """Query transcripts with optional filters."""
        query = """
            SELECT id, session_id, parent_session_id, archived_at,
                   transcript_path, size_bytes, compressed_bytes, metadata
            FROM session_transcripts WHERE 1=1
        """
        params: list = []

        if parent_session_id:
            query += " AND parent_session_id = ?"
            params.append(parent_session_id)
        elif not include_subagents:
            query += " AND parent_session_id IS NULL"

        query += " ORDER BY archived_at DESC LIMIT ?"
        params.append(limit)

        with self.connection() as conn:
            rows = conn.execute(query, params).fetchall()

        return [self._row_to_transcript(row) for row in rows]

    def get_transcript_stats(self) -> dict:
        """Get aggregate statistics for archived transcripts."""
        with self.connection() as conn:
            row = conn.execute("""
                SELECT
                    COUNT(*) as total,
                    SUM(CASE WHEN parent_session_id IS NULL THEN 1 ELSE 0 END) as main_count,
                    SUM(CASE WHEN parent_session_id IS NOT NULL THEN 1 ELSE 0 END) as subagent_count,
                    SUM(size_bytes) as total_size,
                    SUM(compressed_bytes) as total_compressed,
                    MIN(archived_at) as oldest,
                    MAX(archived_at) as newest
                FROM session_transcripts
            """).fetchone()

        return {
            "total": row["total"] or 0,
            "main_count": row["main_count"] or 0,
            "subagent_count": row["subagent_count"] or 0,
            "total_size_bytes": row["total_size"] or 0,
            "total_compressed_bytes": row["total_compressed"] or 0,
            "oldest": row["oldest"],
            "newest": row["newest"],
        }

    def get_archived_session_ids(self) -> set[str]:
        """Get set of all archived session IDs (for efficient sync checks)."""
        with self.connection() as conn:
            rows = conn.execute(
                "SELECT session_id, size_bytes FROM session_transcripts"
            ).fetchall()
        return {(row["session_id"], row["size_bytes"]) for row in rows}

    def _row_to_transcript(self, row: sqlite3.Row) -> SessionTranscript:
        ts_str = row["archived_at"]
        try:
            ts = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
        except (ValueError, AttributeError):
            ts = datetime.now(timezone.utc)

        try:
            metadata = json.loads(row["metadata"]) if row["metadata"] else {}
        except (json.JSONDecodeError, TypeError):
            metadata = {}

        return SessionTranscript(
            id=row["id"],
            session_id=row["session_id"],
            parent_session_id=row["parent_session_id"],
            archived_at=ts,
            transcript_path=row["transcript_path"],
            size_bytes=row["size_bytes"],
            compressed_bytes=row["compressed_bytes"],
            metadata=metadata,
        )

    # -------------------------------------------------------------------------
    # Session tags - agent-assigned labels for session lookup
    # -------------------------------------------------------------------------

    def tag_session(self, cwd: str) -> str:
        """Create a tag for the current session context.

        Generates a random tag and stores it with CWD + timestamp.
        The session ID is resolved lazily when get_session_by_tag is called.

        Args:
            cwd: Working directory where the session is running

        Returns:
            The generated tag (8 char random string)
        """
        import secrets
        tag = secrets.token_hex(4)  # 8 hex chars
        ts = datetime.now(timezone.utc).isoformat()

        with self.connection() as conn:
            conn.execute(
                """
                INSERT INTO session_tags (tag, session_id, created_at, cwd)
                VALUES (?, NULL, ?, ?)
                """,
                (tag, ts, cwd),
            )
            conn.commit()

        return tag

    def get_session_by_tag(self, tag: str) -> str | None:
        """Retrieve a session ID by its tag.

        Resolves the session ID lazily by finding the session that was active
        in the tagged CWD at the tagged timestamp.

        Args:
            tag: The tag to look up

        Returns:
            Session ID if found, None otherwise
        """
        with self.connection() as conn:
            row = conn.execute(
                "SELECT session_id, created_at, cwd FROM session_tags WHERE tag = ?",
                (tag,),
            ).fetchone()

        if not row:
            return None

        # If already resolved, return it
        if row["session_id"]:
            return row["session_id"]

        # Resolve: find session active in this CWD around this timestamp
        created_at = row["created_at"]
        cwd = row["cwd"]

        # Find sessions with events in this CWD near the tag timestamp
        # Look for events within a small window around the tag creation time
        with self.connection() as conn:
            # Find the session with the most recent event in this CWD
            # that occurred before or at the tag timestamp
            result = conn.execute(
                """
                SELECT session_id FROM semantic_events
                WHERE (json_extract(metadata, '$.cwd') = ? OR json_extract(metadata, '$.cwd') LIKE ?)
                  AND timestamp <= ?
                  AND session_id IS NOT NULL
                ORDER BY timestamp DESC
                LIMIT 1
                """,
                (cwd, cwd + "/%", created_at),
            ).fetchone()

        if not result:
            return None

        session_id = result["session_id"]

        # Cache the resolved session_id for future lookups
        with self.connection() as conn:
            conn.execute(
                "UPDATE session_tags SET session_id = ? WHERE tag = ?",
                (session_id, tag),
            )
            conn.commit()

        return session_id

    def list_session_tags(self, session_id: str | None = None) -> list[dict]:
        """List all session tags, optionally filtered by session.

        Returns:
            List of dicts with tag, session_id, created_at, cwd
        """
        query = "SELECT * FROM session_tags"
        params: list = []
        if session_id:
            query += " WHERE session_id = ?"
            params.append(session_id)
        query += " ORDER BY created_at DESC"

        with self.connection() as conn:
            rows = conn.execute(query, params).fetchall()

        return [dict(row) for row in rows]

    def delete_session_tag(self, tag: str) -> bool:
        """Delete a session tag.

        Returns:
            True if tag was deleted, False if it didn't exist
        """
        with self.connection() as conn:
            cursor = conn.execute("DELETE FROM session_tags WHERE tag = ?", (tag,))
            conn.commit()
            return cursor.rowcount > 0
