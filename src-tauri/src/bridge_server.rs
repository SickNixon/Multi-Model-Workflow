// src-tauri/src/bridge_server.rs
// A tiny local HTTP server (axum) that runs on 127.0.0.1:7539.
//
// WHY THIS EXISTS:
// Tauri's window.eval() is fire-and-forget — you can send JS *to* a WebView
// but there's no clean return-value mechanism for external-URL windows.
// Instead, injected JS POSTs back to this server when it has something to say
// (output ready, error, status change). We then emit Tauri events to the
// orchestrator window so the React UI can react.
//
// SECURITY NOTE:
// This server is bound to 127.0.0.1 only, never 0.0.0.0. It's local-only.
// The CORS policy allows any origin because the requests come from third-party
// site origins (gemini.google.com etc.) that we have no control over.

use axum::{
    extract::State as AxumState,
    http::{Method, StatusCode},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tower_http::cors::{Any, CorsLayer};

use crate::state::AppState;

// ── Bridge port ───────────────────────────────────────────────────────────────

pub const BRIDGE_PORT: u16 = 7539;

// ── Request / response types ──────────────────────────────────────────────────

/// Payload posted by injected JS to /bridge/event
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    /// Generation finished — output is the full text response.
    Output { panel_id: String, output: String },
    /// Bridge initialised successfully after page load.
    Ready { panel_id: String },
    /// Something broke in the injected script.
    Error { panel_id: String, message: String },
    /// Generation started (the AI started streaming).
    Generating { panel_id: String },
}

/// Tauri event names emitted to the orchestrator window.
pub const EVENT_PANEL_OUTPUT:     &str = "panel:output";
pub const EVENT_PANEL_READY:      &str = "panel:ready";
pub const EVENT_PANEL_ERROR:      &str = "panel:error";
pub const EVENT_PANEL_GENERATING: &str = "panel:generating";

/// Serialisable payload for all panel Tauri events.
#[derive(Debug, Clone, Serialize)]
pub struct PanelEventPayload {
    pub panel_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ── Axum shared state ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct ServerState {
    app: AppHandle,
}

// ── Route handler ─────────────────────────────────────────────────────────────

async fn handle_bridge_event(
    AxumState(ctx): AxumState<ServerState>,
    Json(event): Json<BridgeEvent>,
) -> StatusCode {
    // Retrieve the single shared AppState from the Tauri app handle
    let app_state = ctx.app.state::<AppState>();

    match event {
        BridgeEvent::Output { panel_id, output } => {
            app_state.store_output(&panel_id, output.clone());

            let _ = ctx.app.emit(
                EVENT_PANEL_OUTPUT,
                PanelEventPayload {
                    panel_id,
                    output: Some(output),
                    message: None,
                },
            );
        }

        BridgeEvent::Ready { panel_id } => {
            use crate::state::PanelStatus;
            app_state.set_status(&panel_id, PanelStatus::Idle);

            let _ = ctx.app.emit(
                EVENT_PANEL_READY,
                PanelEventPayload {
                    panel_id,
                    output: None,
                    message: None,
                },
            );
        }

        BridgeEvent::Error { panel_id, message } => {
            use crate::state::PanelStatus;
            app_state.set_status(
                &panel_id,
                PanelStatus::Error { message: message.clone() },
            );

            let _ = ctx.app.emit(
                EVENT_PANEL_ERROR,
                PanelEventPayload {
                    panel_id,
                    output: None,
                    message: Some(message),
                },
            );
        }

        BridgeEvent::Generating { panel_id } => {
            use crate::state::PanelStatus;
            app_state.set_status(&panel_id, PanelStatus::Generating);

            let _ = ctx.app.emit(
                EVENT_PANEL_GENERATING,
                PanelEventPayload {
                    panel_id,
                    output: None,
                    message: None,
                },
            );
        }
    }

    StatusCode::OK
}

// ── Server startup ─────────────────────────────────────────────────────────────

/// Start the bridge HTTP server. Call this once at app startup via
/// tauri::async_runtime::spawn. It runs forever (until the app exits).
pub async fn start(app: AppHandle) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let server_state = ServerState { app };

    let router = Router::new()
        .route("/bridge/event", post(handle_bridge_event))
        .layer(cors)
        .with_state(server_state);

    let addr = format!("127.0.0.1:{}", BRIDGE_PORT);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[bridge_server] Failed to bind {}: {}", addr, e);
            return;
        }
    };

    println!("[bridge_server] Listening on http://{}", addr);

    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("[bridge_server] Server error: {}", e);
    }
}
