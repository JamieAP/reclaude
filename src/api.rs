use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use tower_http::cors::CorsLayer;

use crate::db::events::EventStore;
use crate::db::sqlite::MetadataDb;

/// Shared application state - MetadataDb wrapped in tokio Mutex for async safety.
struct AppState {
    events: EventStore,
    meta: tokio::sync::Mutex<MetadataDb>,
}

/// Start the axum web server.
pub async fn serve(host: &str, port: u16, no_open: bool) -> anyhow::Result<()> {
    let db = crate::db::Database::open().await?;
    let state = Arc::new(AppState {
        events: db.events,
        meta: tokio::sync::Mutex::new(db.meta),
    });

    let api_routes = Router::new()
        .route("/health", get(health))
        .route("/events", get(list_events))
        .route("/events/{id}", get(get_event))
        .route("/sessions", get(list_sessions))
        .route("/statistics", get(statistics));

    let mut app = Router::new()
        .nest("/api", api_routes)
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

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
    State(state): State<Arc<AppState>>,
    Query(params): Query<EventsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let limit = params.limit.unwrap_or(100).min(1000);

    let event_types: Vec<&str> = match &params.event_type {
        Some(t) => t.split(',').collect(),
        None => Vec::new(),
    };

    let events = state
        .events
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
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let event = state
        .events
        .get_by_id(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match event {
        Some(e) => Ok(Json(event_json(&e))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let meta = state.meta.lock().await;
    let sessions = meta
        .list_sessions(None, 100)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let payload: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            serde_json::json!({
                "session_id": s.session_id,
                "started_at": s.started_at,
                "ended_at": s.ended_at,
                "cwd": s.cwd,
                "repo_name": s.repo_name,
                "event_count": s.event_count,
                "is_active": s.is_active,
            })
        })
        .collect();

    Ok(Json(serde_json::json!(payload)))
}

async fn statistics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let total = state
        .events
        .count()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let counts = state
        .events
        .counts_by_type()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let by_type: serde_json::Map<String, serde_json::Value> = counts
        .into_iter()
        .map(|(k, v)| (k, serde_json::json!(v)))
        .collect();

    Ok(Json(serde_json::json!({
        "total_events": total,
        "by_type": by_type,
    })))
}

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
