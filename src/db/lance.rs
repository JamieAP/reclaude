use std::path::Path;
use std::sync::Arc;

use arrow_array::{
    Array, Int64Array, RecordBatch, RecordBatchIterator, StringArray,
    builder::FixedSizeListBuilder,
    builder::Float32Builder,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};

use crate::models::Event;

const TABLE_NAME: &str = "events";
const VECTOR_DIM: i32 = 768;

/// Escape a string value for use in LanceDB SQL-like filter expressions.
/// Prevents injection by doubling single quotes.
fn escape_filter_val(s: &str) -> String {
    s.replace('\'', "''")
}

/// Build a WHERE clause from common filter parameters.
fn build_filter_clause(
    event_types: &[&str],
    session_id: Option<&str>,
    cwd: Option<&str>,
) -> Vec<String> {
    let mut filters = Vec::new();
    if event_types.len() == 1 {
        filters.push(format!("event_type = '{}'", escape_filter_val(event_types[0])));
    } else if event_types.len() > 1 {
        let in_list: Vec<String> = event_types.iter().map(|t| format!("'{}'", escape_filter_val(t))).collect();
        filters.push(format!("event_type IN ({})", in_list.join(", ")));
    }
    if let Some(sid) = session_id {
        filters.push(format!("session_id = '{}'", escape_filter_val(sid)));
    }
    if let Some(cwd_val) = cwd {
        filters.push(format!("cwd LIKE '{}%'", escape_filter_val(cwd_val)));
    }
    filters
}

/// LanceDB-backed event storage with vector search and FTS.
///
/// Events are stored as Arrow columnar data in `~/.reclaude/lance/events/`.
/// The table uses tantivy for FTS indexing on the `content` column and
/// supports 768-dim vector search for semantic queries.
pub struct EventStore {
    db: lancedb::Connection,
    db_path: String,
}

impl EventStore {
    pub async fn open(base: &Path) -> anyhow::Result<Self> {
        let lance_dir = base.join("lance");
        std::fs::create_dir_all(&lance_dir)?;
        let db_path = lance_dir.to_string_lossy().to_string();

        let db = lancedb::connect(&db_path).execute().await?;
        let store = Self { db, db_path };
        store.ensure_table().await?;
        Ok(store)
    }

    /// Create the events table if it doesn't exist.
    async fn ensure_table(&self) -> anyhow::Result<()> {
        let tables = self.db.table_names().execute().await?;
        if tables.contains(&TABLE_NAME.to_string()) {
            return Ok(());
        }

        // Create with an empty batch matching our schema
        let schema = Self::schema();
        let batch = RecordBatch::new_empty(schema.clone());
        let batches = RecordBatchIterator::new(
            vec![Ok(batch)],
            schema,
        );
        self.db
            .create_table(TABLE_NAME, Box::new(batches))
            .execute()
            .await?;
        Ok(())
    }

    /// The Arrow schema for the events table.
    fn schema() -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("timestamp", DataType::Utf8, false),
            Field::new("event_type", DataType::Utf8, false),
            Field::new("category", DataType::Utf8, false),
            Field::new("session_id", DataType::Utf8, true),
            Field::new("content", DataType::Utf8, false),
            Field::new("cwd", DataType::Utf8, true),
            Field::new("remote_url", DataType::Utf8, true),
            Field::new("repo_name", DataType::Utf8, true),
            Field::new("branch", DataType::Utf8, true),
            Field::new("tool_name", DataType::Utf8, true),
            Field::new("file_path", DataType::Utf8, true),
            Field::new("metadata_json", DataType::Utf8, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    VECTOR_DIM,
                ),
                true,
            ),
        ]))
    }

    fn table(&self) -> impl std::future::Future<Output = anyhow::Result<lancedb::Table>> + '_ {
        async {
            Ok(self.db.open_table(TABLE_NAME).execute().await?)
        }
    }

    /// Get the next monotonic event ID by counting existing rows.
    pub async fn next_id(&self) -> anyhow::Result<i64> {
        let table = self.table().await?;
        let count = table.count_rows(None).await?;
        Ok(count as i64 + 1)
    }

    /// Insert a single event, assigning a monotonic ID. Returns the assigned ID.
    pub async fn insert(&self, event: &Event) -> anyhow::Result<i64> {
        let table = self.table().await?;
        let id = self.next_id().await?;
        let mut event = event.clone();
        event.id = id;
        let batch = Self::event_to_batch(&event)?;
        let batches = RecordBatchIterator::new(
            vec![Ok(batch)],
            Self::schema(),
        );
        table.add(Box::new(batches)).execute().await?;
        Ok(id)
    }

    /// Query events with filters, returned sorted by timestamp descending.
    ///
    /// LanceDB scans don't support ORDER BY, so we filter with `.only_if()`,
    /// collect to Vec, and sort in Rust.
    pub async fn query(
        &self,
        event_types: &[&str],
        session_id: Option<&str>,
        cwd: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<Event>> {
        self.query_range(event_types, session_id, cwd, None, None, limit).await
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
        let table = self.table().await?;
        let mut q = table.query();

        // Build SQL WHERE clause
        let mut filters = build_filter_clause(event_types, session_id, cwd);
        if let Some(since_val) = since {
            filters.push(format!("timestamp >= '{}'", escape_filter_val(since_val)));
        }
        if let Some(until_val) = until {
            filters.push(format!("timestamp < '{}'", escape_filter_val(until_val)));
        }

        if !filters.is_empty() {
            q = q.only_if(filters.join(" AND "));
        }

        // Exclude the vector column from results for perf.
        // LanceDB 0.15 applies a default limit of 10 even on plain scans,
        // so we must set an explicit limit. We sort in Rust after fetching,
        // so we need all matching rows. Use count_rows() as the ceiling.
        let row_count = table.count_rows(None).await?;
        q = q.select(lancedb::query::Select::columns(&[
            "id", "timestamp", "event_type", "category", "session_id",
            "content", "cwd", "remote_url", "repo_name", "branch",
            "tool_name", "file_path", "metadata_json",
        ]))
        .limit(row_count);

        let batches: Vec<RecordBatch> = q.execute().await?.try_collect().await?;
        let mut events = Self::batches_to_events(&batches)?;

        // Sort by timestamp descending (LanceDB doesn't support ORDER BY)
        events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        if limit > 0 && events.len() > limit {
            events.truncate(limit);
        }

        Ok(events)
    }

    /// Full-text search over event content using tantivy FTS index.
    ///
    /// If the FTS index is stale or missing, auto-rebuilds once and retries.
    /// LanceDB can panic (in Arrow cast code) when the index is severely
    /// out of date, so we catch that and treat it as a rebuild-worthy error.
    pub async fn search_fts(
        &self,
        query: &str,
        event_types: &[&str],
        session_id: Option<&str>,
        cwd: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<Event>> {
        match self.try_fts_query(query, event_types, session_id, cwd, limit).await {
            Ok(events) => Ok(events),
            Err(e) => {
                tracing::debug!("FTS query failed ({e}), rebuilding index and retrying");
                self.rebuild_fts_index().await?;
                self.try_fts_query(query, event_types, session_id, cwd, limit).await
            }
        }
    }

    /// Execute an FTS query against the existing index. Returns an error
    /// if the index is missing, stale, or corrupted.
    async fn try_fts_query(
        &self,
        query: &str,
        event_types: &[&str],
        session_id: Option<&str>,
        cwd: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<Event>> {
        let table = self.table().await?;
        let fts_query = lance_index::scalar::FullTextSearchQuery::new(query.to_string());

        let mut q = table.query()
            .full_text_search(fts_query)
            .select(lancedb::query::Select::columns(&[
                "id", "timestamp", "event_type", "category", "session_id",
                "content", "cwd", "remote_url", "repo_name", "branch",
                "tool_name", "file_path", "metadata_json",
            ]))
            .limit(limit);

        let filters = build_filter_clause(event_types, session_id, cwd);
        if !filters.is_empty() {
            q = q.only_if(filters.join(" AND "));
        }

        let batches: Vec<RecordBatch> = q.execute().await?.try_collect().await?;
        Self::batches_to_events(&batches)
    }

    /// Semantic (vector) search - find events nearest to query embedding.
    pub async fn search_vector(
        &self,
        query_vector: &[f32],
        event_types: &[&str],
        session_id: Option<&str>,
        cwd: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<Event>> {
        let table = self.table().await?;

        let mut q = table.vector_search(query_vector)?
            .limit(limit)
            .select(lancedb::query::Select::columns(&[
                "id", "timestamp", "event_type", "category", "session_id",
                "content", "cwd", "remote_url", "repo_name", "branch",
                "tool_name", "file_path", "metadata_json",
            ]));

        // Build WHERE filter (only rows with non-null vectors)
        let mut filters = vec!["vector IS NOT NULL".to_string()];
        filters.extend(build_filter_clause(event_types, session_id, cwd));
        q = q.only_if(filters.join(" AND "));

        let batches: Vec<RecordBatch> = q.execute()
            .await?
            .try_collect()
            .await?;

        Self::batches_to_events(&batches)
    }

    /// Rebuild the tantivy FTS index on the content column.
    ///
    /// Must be called after inserts for FTS to pick up new data.
    /// Uses `replace=true` to rebuild from scratch.
    pub async fn rebuild_fts_index(&self) -> anyhow::Result<()> {
        let table = self.table().await?;
        table
            .create_index(
                &["content"],
                lancedb::index::Index::FTS(
                    lancedb::index::scalar::FtsIndexBuilder::default(),
                ),
            )
            .replace(true)
            .execute()
            .await?;
        Ok(())
    }

    /// Compact small data fragments into larger files.
    ///
    /// LanceDB creates a new fragment for each `add()` call. After bulk
    /// insertions (e.g., transcript extraction), compact to merge fragments
    /// so queries can scan all data reliably.
    pub async fn compact(&self) -> anyhow::Result<()> {
        let table = self.table().await?;
        table.optimize(lancedb::table::OptimizeAction::All).await?;
        Ok(())
    }

    /// Get the total number of events.
    pub async fn count(&self) -> anyhow::Result<usize> {
        let table = self.table().await?;
        Ok(table.count_rows(None).await?)
    }

    /// Get event counts grouped by event_type.
    ///
    /// Since LanceDB doesn't support GROUP BY, we scan all events
    /// and aggregate in Rust.
    pub async fn counts_by_type(&self) -> anyhow::Result<Vec<(String, usize)>> {
        let table = self.table().await?;
        // LanceDB 0.15 applies a default limit of 10 even on plain scans.
        let row_count = table.count_rows(None).await?;
        let batches: Vec<RecordBatch> = table.query()
            .select(lancedb::query::Select::columns(&["event_type"]))
            .limit(row_count)
            .execute()
            .await?
            .try_collect()
            .await?;

        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for batch in &batches {
            let col = batch
                .column_by_name("event_type")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                .ok_or_else(|| anyhow::anyhow!("missing event_type column"))?;
            for i in 0..col.len() {
                if !col.is_null(i) {
                    *counts.entry(col.value(i).to_string()).or_default() += 1;
                }
            }
        }

        let mut result: Vec<_> = counts.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(result)
    }

    /// Get a single event by ID.
    pub async fn get_by_id(&self, id: i64) -> anyhow::Result<Option<Event>> {
        let table = self.table().await?;
        let batches: Vec<RecordBatch> = table.query()
            .only_if(format!("id = {id}"))
            .select(lancedb::query::Select::columns(&[
                "id", "timestamp", "event_type", "category", "session_id",
                "content", "cwd", "remote_url", "repo_name", "branch",
                "tool_name", "file_path", "metadata_json",
            ]))
            .limit(1)
            .execute()
            .await?
            .try_collect()
            .await?;

        let events = Self::batches_to_events(&batches)?;
        Ok(events.into_iter().next())
    }

    /// Query events with null vectors that should have embeddings.
    pub async fn query_unembedded(&self, limit: usize) -> anyhow::Result<Vec<Event>> {
        let table = self.table().await?;
        let where_clause =
            "vector IS NULL AND event_type IN ('user_prompt','assistant','plan','thinking')";
        let batches: Vec<RecordBatch> = table.query()
            .only_if(where_clause)
            .select(lancedb::query::Select::columns(&[
                "id", "timestamp", "event_type", "category", "session_id",
                "content", "cwd", "remote_url", "repo_name", "branch",
                "tool_name", "file_path", "metadata_json",
            ]))
            .limit(limit)
            .execute()
            .await?
            .try_collect()
            .await?;
        Self::batches_to_events(&batches)
    }

    /// Update the vector column for a specific event by ID.
    pub async fn update_vector(&self, event_id: i64, vector: &[f32]) -> anyhow::Result<()> {
        let table = self.table().await?;

        // Build a single-row batch with id + vector, then merge on id
        let id_arr = Int64Array::from(vec![event_id]);

        let mut list_builder = FixedSizeListBuilder::new(
            arrow_array::builder::Float32Builder::new(),
            VECTOR_DIM,
        );
        let values = list_builder.values();
        for &v in vector {
            values.append_value(v);
        }
        list_builder.append(true);

        let schema = Arc::new(arrow_schema::Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    VECTOR_DIM,
                ),
                true,
            ),
        ]));

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(id_arr), Arc::new(list_builder.finish())],
        )?;

        let mut op = table.merge_insert(&["id"]);
        op.when_matched_update_all(None);
        op.execute(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
            .await?;

        Ok(())
    }

    // ── Arrow Conversion ────────────────────────────────────────────

    /// Convert a single Event to an Arrow RecordBatch.
    fn event_to_batch(event: &Event) -> anyhow::Result<RecordBatch> {
        Self::events_to_batch(std::slice::from_ref(event))
    }

    /// Convert a slice of Events to an Arrow RecordBatch.
    fn events_to_batch(events: &[Event]) -> anyhow::Result<RecordBatch> {
        let len = events.len();

        let ids: Vec<i64> = events.iter().map(|e| e.id).collect();
        let timestamps: Vec<&str> = events.iter().map(|e| e.timestamp.as_str()).collect();
        let event_types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
        let categories: Vec<&str> = events.iter().map(|e| e.category.as_str()).collect();
        let session_ids: Vec<Option<&str>> = events.iter().map(|e| e.session_id.as_deref()).collect();
        let contents: Vec<&str> = events.iter().map(|e| e.content.as_str()).collect();
        let cwds: Vec<Option<&str>> = events.iter().map(|e| e.cwd.as_deref()).collect();
        let remote_urls: Vec<Option<&str>> = events.iter().map(|e| e.remote_url.as_deref()).collect();
        let repo_names: Vec<Option<&str>> = events.iter().map(|e| e.repo_name.as_deref()).collect();
        let branches: Vec<Option<&str>> = events.iter().map(|e| e.branch.as_deref()).collect();
        let tool_names: Vec<Option<&str>> = events.iter().map(|e| e.tool_name.as_deref()).collect();
        let file_paths: Vec<Option<&str>> = events.iter().map(|e| e.file_path.as_deref()).collect();
        let metadata_jsons: Vec<&str> = events.iter().map(|e| e.metadata_json.as_str()).collect();

        // Build the vector column (FixedSizeList<Float32, 768>)
        let mut list_builder = FixedSizeListBuilder::new(Float32Builder::new(), VECTOR_DIM);
        for event in events {
            match &event.vector {
                Some(vec) => {
                    let values = list_builder.values();
                    for &v in vec {
                        values.append_value(v);
                    }
                    list_builder.append(true);
                }
                None => {
                    let values = list_builder.values();
                    for _ in 0..VECTOR_DIM {
                        values.append_null();
                    }
                    list_builder.append(false);
                }
            }
        }

        let batch = RecordBatch::try_new(
            Self::schema(),
            vec![
                Arc::new(Int64Array::from(ids)),
                Arc::new(StringArray::from(timestamps)),
                Arc::new(StringArray::from(event_types)),
                Arc::new(StringArray::from(categories)),
                Arc::new(StringArray::from(session_ids)),
                Arc::new(StringArray::from(contents)),
                Arc::new(StringArray::from(cwds)),
                Arc::new(StringArray::from(remote_urls)),
                Arc::new(StringArray::from(repo_names)),
                Arc::new(StringArray::from(branches)),
                Arc::new(StringArray::from(tool_names)),
                Arc::new(StringArray::from(file_paths)),
                Arc::new(StringArray::from(metadata_jsons)),
                Arc::new(list_builder.finish()),
            ],
        )?;

        debug_assert_eq!(batch.num_rows(), len);
        Ok(batch)
    }

    /// Convert Arrow RecordBatches back to Event structs.
    fn batches_to_events(batches: &[RecordBatch]) -> anyhow::Result<Vec<Event>> {
        let mut events = Vec::new();
        for batch in batches {
            let n = batch.num_rows();
            if n == 0 {
                continue;
            }

            let id_col = col_i64(batch, "id")?;
            let ts_col = col_str(batch, "timestamp")?;
            let et_col = col_str(batch, "event_type")?;
            let cat_col = col_str(batch, "category")?;
            let sid_col = col_str_opt(batch, "session_id");
            let content_col = col_str(batch, "content")?;
            let cwd_col = col_str_opt(batch, "cwd");
            let remote_col = col_str_opt(batch, "remote_url");
            let repo_col = col_str_opt(batch, "repo_name");
            let branch_col = col_str_opt(batch, "branch");
            let tool_col = col_str_opt(batch, "tool_name");
            let file_col = col_str_opt(batch, "file_path");
            let meta_col = col_str(batch, "metadata_json")?;

            for i in 0..n {
                events.push(Event {
                    id: id_col.value(i),
                    timestamp: ts_col.value(i).to_string(),
                    event_type: et_col.value(i).to_string(),
                    category: cat_col.value(i).to_string(),
                    session_id: sid_col.as_ref().and_then(|c| {
                        if c.is_null(i) { None } else { Some(c.value(i).to_string()) }
                    }),
                    content: content_col.value(i).to_string(),
                    cwd: cwd_col.as_ref().and_then(|c| {
                        if c.is_null(i) { None } else { Some(c.value(i).to_string()) }
                    }),
                    remote_url: remote_col.as_ref().and_then(|c| {
                        if c.is_null(i) { None } else { Some(c.value(i).to_string()) }
                    }),
                    repo_name: repo_col.as_ref().and_then(|c| {
                        if c.is_null(i) { None } else { Some(c.value(i).to_string()) }
                    }),
                    branch: branch_col.as_ref().and_then(|c| {
                        if c.is_null(i) { None } else { Some(c.value(i).to_string()) }
                    }),
                    tool_name: tool_col.as_ref().and_then(|c| {
                        if c.is_null(i) { None } else { Some(c.value(i).to_string()) }
                    }),
                    file_path: file_col.as_ref().and_then(|c| {
                        if c.is_null(i) { None } else { Some(c.value(i).to_string()) }
                    }),
                    metadata_json: meta_col.value(i).to_string(),
                    vector: None, // Don't deserialize vectors on read (perf)
                });
            }
        }
        Ok(events)
    }
}

// ── Arrow Column Helpers ────────────────────────────────────────────

fn col_i64<'a>(batch: &'a RecordBatch, name: &str) -> anyhow::Result<&'a Int64Array> {
    batch
        .column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<Int64Array>())
        .ok_or_else(|| anyhow::anyhow!("missing or wrong type for column: {name}"))
}

fn col_str<'a>(batch: &'a RecordBatch, name: &str) -> anyhow::Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        .ok_or_else(|| anyhow::anyhow!("missing or wrong type for column: {name}"))
}

fn col_str_opt<'a>(batch: &'a RecordBatch, name: &str) -> Option<&'a StringArray> {
    batch
        .column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
}

impl std::fmt::Debug for EventStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventStore")
            .field("path", &self.db_path)
            .finish()
    }
}
