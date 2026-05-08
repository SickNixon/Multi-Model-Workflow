// src-tauri/src/bridge_server.rs
// Local HTTP server on 127.0.0.1:7539.
//
// Two routes:
//   POST /bridge/event  — full JSON payload (works when CSP allows fetch to localhost)
//   GET  /ping          — chunked Image beacon fallback (bypasses connect-src CSP)
//
// WHY IMAGE BEACONS:
// AI sites have CSPs that block fetch/XHR to localhost (connect-src).
// But img-src is often unrestricted. By loading a fake image URL pointing to
// our local server, we can receive data from injected JS without CSP interference.
// Large payloads are split into chunks and reassembled here.

use axum::{
    extract::{Query, State as AxumState},
    http::{Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::{Arc, Mutex}};
use tauri::{AppHandle, Emitter, Manager};
use tower_http::cors::{Any, CorsLayer};

use crate::state::AppState;

pub const BRIDGE_PORT: u16 = 7539;

// ── Tauri event names ─────────────────────────────────────────────────────────
pub const EVENT_PANEL_OUTPUT:     &str = "panel:output";
pub const EVENT_PANEL_READY:      &str = "panel:ready";
pub const EVENT_PANEL_ERROR:      &str = "panel:error";
pub const EVENT_PANEL_GENERATING: &str = "panel:generating";

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    Output    { panel_id: String, output: String },
    Ready     { panel_id: String },
    Error     { panel_id: String, message: String },
    Generating{ panel_id: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct PanelEventPayload {
    pub panel_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output:  Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ── Chunk reassembly buffer ───────────────────────────────────────────────────
// Image beacon payloads arrive as URL-encoded chunks.
// Key: chunk_id, Value: (received chunks Vec<Option<String>>, total count)
type ChunkMap = Arc<Mutex<HashMap<String, Vec<Option<String>>>>>;

#[derive(Deserialize)]
struct PingParams {
    /// Unique ID for this multi-chunk message (timestamp-random)
    id: String,
    /// 0-based index of this chunk
    i: usize,
    /// Total number of chunks
    t: usize,
    /// URL-encoded chunk of the JSON payload
    d: String,
}

// ── Axum state ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct ServerState {
    app:    AppHandle,
    chunks: ChunkMap,
}

// ── Route: POST /bridge/event (direct fetch) ──────────────────────────────────

async fn handle_post_event(
    AxumState(ctx): AxumState<ServerState>,
    Json(event): Json<BridgeEvent>,
) -> StatusCode {
    dispatch_event(&ctx.app, event);
    StatusCode::OK
}

// ── Route: GET /ping (Image beacon with chunked payload) ─────────────────────

async fn handle_ping(
    AxumState(ctx): AxumState<ServerState>,
    Query(params): Query<PingParams>,
) -> impl IntoResponse {
    let full_json = {
        let mut map = ctx.chunks.lock().unwrap();
        let entry = map.entry(params.id.clone()).or_insert_with(|| vec![None; params.t]);

        // Grow if needed (shouldn't happen but be safe)
        if entry.len() < params.t {
            entry.resize(params.t, None);
        }

        if let Some(slot) = entry.get_mut(params.i) {
            *slot = Some(params.d.clone());
        }

        // All chunks received?
        if entry.iter().all(|c| c.is_some()) {
            let assembled: String = entry.iter().filter_map(|c| c.as_deref()).collect();
            map.remove(&params.id);
            Some(assembled)
        } else {
            None
        }
    };

    if let Some(json) = full_json {
        match serde_json::from_str::<BridgeEvent>(&json) {
            Ok(event) => { dispatch_event(&ctx.app, event); }
            Err(e) => {
                eprintln!("[bridge_server] failed to parse reassembled payload: {e}");
            }
        }
    }

    // Return a 1x1 transparent GIF so the Image element doesn't show broken icon
    (
        StatusCode::OK,
        [("Content-Type", "image/gif"), ("Access-Control-Allow-Origin", "*")],
        &b"\x47\x49\x46\x38\x39\x61\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00\x21\xf9\x04\x00\x00\x00\x00\x00\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00\x3b"[..],
    )
}

// ── Shared event dispatcher ───────────────────────────────────────────────────

fn dispatch_event(app: &AppHandle, event: BridgeEvent) {
    let app_state = app.state::<AppState>();
    match event {
        BridgeEvent::Output { panel_id, output } => {
            app_state.store_output(&panel_id, output.clone());
            let _ = app.emit(EVENT_PANEL_OUTPUT, PanelEventPayload {
                panel_id, output: Some(output), message: None,
            });
        }
        BridgeEvent::Ready { panel_id } => {
            use crate::state::PanelStatus;
            app_state.set_status(&panel_id, PanelStatus::Idle);
            let _ = app.emit(EVENT_PANEL_READY, PanelEventPayload {
                panel_id, output: None, message: None,
            });
        }
        BridgeEvent::Error { panel_id, message } => {
            use crate::state::PanelStatus;
            app_state.set_status(&panel_id, PanelStatus::Error { message: message.clone() });
            let _ = app.emit(EVENT_PANEL_ERROR, PanelEventPayload {
                panel_id, output: None, message: Some(message),
            });
        }
        BridgeEvent::Generating { panel_id } => {
            use crate::state::PanelStatus;
            app_state.set_status(&panel_id, PanelStatus::Generating);
            let _ = app.emit(EVENT_PANEL_GENERATING, PanelEventPayload {
                panel_id, output: None, message: None,
            });
        }
    }
}

// ── Diagnostic dump ───────────────────────────────────────────────────────────
// GET /diag?panel=X&data=... — receives DOM info from injected JS on page load.
// Writes to /tmp/vibe-diag-{panel}.json for Desktop Commander to read.
// axum's Query<> handles URL-decoding automatically — no extra crate needed.

#[derive(Deserialize)]
struct DiagParams { panel: Option<String>, data: String }

async fn handle_diag(Query(params): Query<DiagParams>) -> impl IntoResponse {
    let panel = params.panel.as_deref().unwrap_or("unknown");
    let path   = format!("/tmp/vibe-diag-{panel}.json");
    let _      = std::fs::write(&path, params.data.as_bytes());
    println!("[bridge_server] diagnostic saved → {path}");
    (StatusCode::OK, [("Content-Type", "text/plain"), ("Access-Control-Allow-Origin", "*")], "ok")
}

pub async fn start(app: AppHandle) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let state = ServerState {
        app,
        chunks: Arc::new(Mutex::new(HashMap::new())),
    };

    let router = Router::new()
        .route("/bridge/event", post(handle_post_event))
        .route("/ping",         get(handle_ping))
        .route("/diag",         get(handle_diag))
        .layer(cors)
        .with_state(state);

    let addr = format!("127.0.0.1:{}", BRIDGE_PORT);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => { eprintln!("[bridge_server] bind failed {addr}: {e}"); return; }
    };

    println!("[bridge_server] Listening on http://{}", addr);
    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("[bridge_server] error: {e}");
    }
}
