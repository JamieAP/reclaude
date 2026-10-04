use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;

use crate::db::Database;

/// Start the axum web server.
pub async fn serve(host: &str, port: u16, no_open: bool) -> anyhow::Result<()> {
    let db = Database::open().await?;
    let state = Arc::new(db);

    let mut app = api_router(state);

    // Serve static frontend from dist/ if it exists
    let dist_dir = frontend_dist_dir();
    if dist_dir.join("index.html").exists() {
        app = app.nest_service(
            "/assets",
            tower_http::services::ServeDir::new(dist_dir.join("assets")),
        );
        // SPA fallback: serve index.html for all non-API routes
        let dist = dist_dir.clone();
        app = app.fallback(get(move || {
            let index = dist.join("index.html");
            async move {
                match tokio::fs::read_to_string(&index).await {
                    Ok(html) => Html(html).into_response(),
                    Err(_) => StatusCode::NOT_FOUND.into_response(),
                }
            }
        }));
    }

    let addr = format!("{host}:{port}");
    eprintln!("reclaude UI: http://{addr}");

    if !no_open {
        let _ = std::process::Command::new("xdg-open")
            .arg(format!("http://{addr}"))
            .spawn();
    }

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn api_router(state: Arc<Database>) -> Router {
    let api_routes = Router::new()
        .route("/health", get(health))
        .route("/events", get(list_events))
        .route("/events/{id}", get(get_event))
        .route("/sessions", get(list_sessions))
        .route("/statistics", get(statistics))
.route("/repos", get(list_repos))
        .route("/focus", get(list_focus))
        .route("/personas/usage", get(personas_usage))
        .route("/personas/timeline", get(personas_timeline));

    let app = Router::new()
        .nest("/api", api_routes)
        .with_state(state);
    app


}

fn frontend_dist_dir() -> PathBuf {
    // Check common locations for the built frontend
    let candidates = [
        // Dev: binary in target/release/, frontend in repo root
        {
            let exe = std::env::current_exe().unwrap_or_default();
            exe.parent()
                .unwrap_or(std::path::Path::new("."))
                .join("../../frontend/dist")
        },
        // User-level install: ~/.reclaude/frontend/dist/
        crate::db::base_dir().join("frontend/dist"),
        // CWD-relative
        PathBuf::from("frontend/dist"),
    ];

    for path in &candidates {
        if path.join("index.html").exists() {
            return path.clone();
        }
    }

    // Fallback (won't serve frontend, API-only mode)
    candidates[0].clone()
}

// ── API Handlers ──────────────────────────────────────────────────────

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

#[derive(serde::Deserialize)]
struct EventsQuery {
    event_type: Option<String>,
    session_id: Option<String>,
    cwd: Option<String>,
    limit: Option<usize>,
}

async fn list_events(
    State(state): State<Arc<Database>>,
    Query(params): Query<EventsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let limit = params.limit.unwrap_or(100).min(1000);

    let event_types: Vec<&str> = match &params.event_type {
        Some(t) => t.split(',').collect(),
        None => Vec::new(),
    };

    let events = state
        .query(
            &event_types,
            params.session_id.as_deref(),
            params.cwd.as_deref(),
            limit,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let payload: Vec<serde_json::Value> = events
        .iter()
        .map(|e| event_json(e))
        .collect();

    Ok(Json(serde_json::json!(payload)))
}

async fn get_event(
    State(state): State<Arc<Database>>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let event = state
        .get_by_id(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match event {
        Some(e) => Ok(Json(event_json(&e))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn list_sessions(
    State(state): State<Arc<Database>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = state
        .list_sessions(None, 100)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let payload: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            let last_event_at = s.ended_at.as_deref().unwrap_or(&s.started_at);
            let duration_minutes = duration_minutes(&s.started_at, s.ended_at.as_deref());
            let repos: Vec<&str> = s.remote_url.as_deref().into_iter().collect();
            let branches: Vec<&str> = s.branch.as_deref().into_iter().collect();

            serde_json::json!({
                "session_id": s.session_id,
                "last_event_at": last_event_at,
                "event_count": s.event_count,
                "cwd": s.cwd,
                "repos": repos,
                "branches": branches,
                "lines_added": 0,
                "lines_removed": 0,
                "tool_use_count": 0,
                "file_diff_count": 0,
                "compaction_count": 0,
                "user_prompt_count": 0,
                "first_prompt": null,
                "started_at": s.started_at,
                "duration_minutes": duration_minutes,
                "files_modified": [],
            })
        })
        .collect();

    Ok(Json(serde_json::json!(payload)))
}

async fn statistics(
    State(state): State<Arc<Database>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let total = state
        .count()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let counts = state
        .counts_by_type()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let sessions = state
        .list_sessions(None, 10000)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let event_types: Vec<String> = counts.iter().map(|(k, _)| k.clone()).collect();

    let events_by_type: serde_json::Map<String, serde_json::Value> = counts
        .into_iter()
        .map(|(k, v)| (k, serde_json::json!(v)))
        .collect();

    // Build repo list from sessions with remote_url
    let mut repo_map: std::collections::HashMap<String, (Option<String>, i64, String)> =
        std::collections::HashMap::new();
    for s in &sessions {
        if let Some(ref url) = s.remote_url {
            let entry = repo_map.entry(url.clone()).or_insert_with(|| {
                (s.repo_name.clone(), 0, s.started_at.clone())
            });
            entry.1 += s.event_count;
            let ts = s.ended_at.as_deref().unwrap_or(&s.started_at);
            if ts > entry.2.as_str() {
                entry.2 = ts.to_string();
            }
        }
    }
    let mut repos: Vec<(String, Option<String>, i64, String)> = repo_map
        .into_iter()
        .map(|(url, (name, count, last))| (url, name, count, last))
        .collect();
    repos.sort_by(|a, b| b.3.cmp(&a.3));
    let repos: Vec<serde_json::Value> = repos
        .into_iter()
        .map(|(url, name, count, last)| {
            serde_json::json!({
                "remote_url": url,
                "repo_name": name,
                "event_count": count,
                "last_event_at": last,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "total_events": total,
        "events_by_type": events_by_type,
        "total_sessions": sessions.len(),
        "event_types": event_types,
        "repos": repos,
    })))
}

async fn list_repos(
    State(state): State<Arc<Database>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = state
        .list_sessions(None, 10000)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut repo_map: std::collections::HashMap<String, (Option<String>, i64, String)> =
        std::collections::HashMap::new();
    for s in &sessions {
        if let Some(ref url) = s.remote_url {
            let entry = repo_map.entry(url.clone()).or_insert_with(|| {
                (s.repo_name.clone(), 0, s.started_at.clone())
            });
            entry.1 += s.event_count;
            let ts = s.ended_at.as_deref().unwrap_or(&s.started_at);
            if ts > entry.2.as_str() {
                entry.2 = ts.to_string();
            }
        }
    }

    let mut repos: Vec<(String, Option<String>, i64, String)> = repo_map
        .into_iter()
        .map(|(url, (name, count, last))| (url, name, count, last))
        .collect();
    repos.sort_by(|a, b| b.3.cmp(&a.3));
    let repos: Vec<serde_json::Value> = repos
        .into_iter()
        .map(|(url, name, count, last)| {
            serde_json::json!({
                "remote_url": url,
                "repo_name": name,
                "event_count": count,
                "last_event_at": last,
            })
        })
        .collect();

    Ok(Json(serde_json::json!(repos)))
}

async fn list_focus() -> Json<serde_json::Value> {
    Json(serde_json::json!([]))
}

async fn personas_usage() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "days": 7,
        "total_task_calls": 0,
        "builtin_agents": [],
        "custom_personas": [],
    }))
}

async fn personas_timeline() -> Json<serde_json::Value> {
    Json(serde_json::json!([]))
}

// ── Helpers ──────────────────────────────────────────────────────

/// Compute session duration in minutes from started_at/ended_at timestamps.
fn duration_minutes(started_at: &str, ended_at: Option<&str>) -> Option<i64> {
    let end = ended_at?;
    let start = chrono::DateTime::parse_from_rfc3339(started_at).ok()?;
    let end = chrono::DateTime::parse_from_rfc3339(end).ok()?;
    let dur = end.signed_duration_since(start);
    Some(dur.num_minutes())
}

// ── Event Serialization ──────────────────────────────────────────

fn event_json(e: &crate::models::Event) -> serde_json::Value {
    serde_json::json!({
        "id": e.id,
        "timestamp": e.timestamp,
        "event_type": e.event_type,
        "category": e.category,
        "session_id": e.session_id,
        "content": e.content,
        "cwd": e.cwd,
        "tool_name": e.tool_name,
        "file_path": e.file_path,
        "metadata": e.metadata(),
    })
}

#[cfg(test)]
mod privacy_tests {
    use super::*;
    #[tokio::test]
    async fn unrelated_browser_origins_cannot_read_local_session_api() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open_at(dir.path()).unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/health", listener.local_addr().unwrap());
        let (send, receive) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, api_router(db))
                .with_graceful_shutdown(async { let _ = receive.await; }).await.unwrap();
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = client.get(&url).header("Origin", "https://unrelated.example").send().await.unwrap();
        let allow = response.headers().get("access-control-allow-origin").cloned();
        assert!(response.status().is_success());
        let preflight = client.request(reqwest::Method::OPTIONS, &url)
            .header("Origin", "https://unrelated.example")
            .header("Access-Control-Request-Method", "GET").send().await.unwrap();
        let preflight_allow = preflight.headers().get("access-control-allow-origin").cloned();
        let _ = send.send(());
        server.await.unwrap();
        assert!(allow.is_none(), "unrelated origins can read private API responses");
        assert!(preflight_allow.is_none(), "unrelated preflight approved");
    }
}
