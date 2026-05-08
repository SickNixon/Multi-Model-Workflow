// src-tauri/src/commands.rs
// All Tauri commands exposed to the frontend via invoke().

use crate::bridge::get_bridge_script;
use crate::bridge_server::BRIDGE_PORT;
use crate::state::{AppState, PanelInfo, PanelStatus};

use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

// ── Shared result type ────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub struct CmdResult<T: serde::Serialize> {
    pub ok:    bool,
    pub data:  Option<T>,
    pub error: Option<String>,
}

impl<T: serde::Serialize> CmdResult<T> {
    fn success(data: T) -> Self {
        Self { ok: true, data: Some(data), error: None }
    }
    fn fail(msg: impl Into<String>) -> Self {
        Self { ok: false, data: None, error: Some(msg.into()) }
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Return current state of all known panels.
/// Also reconciles: if a window no longer exists but we think it's open, mark Closed.
#[tauri::command]
pub fn get_panel_states(
    app:   AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<Vec<PanelInfo>> {
    // Reconcile stale state — if window was closed by the user via the X button,
    // the AppState won't know until we check here.
    {
        let mut panels = state.panels.lock().unwrap();
        for panel in panels.values_mut() {
            if panel.status.is_open() && app.get_webview_window(&panel.id).is_none() {
                panel.status = PanelStatus::Closed;
            }
        }
    }

    let panels = state.panels.lock().unwrap();
    let mut list: Vec<PanelInfo> = panels.values().cloned().collect();
    list.sort_by_key(|p| {
        crate::state::ALL_PANELS
            .iter()
            .position(|&id| id == p.id)
            .unwrap_or(99)
    });
    CmdResult::success(list)
}

/// Return the bridge HTTP port.
#[tauri::command]
pub fn get_bridge_port() -> CmdResult<u16> {
    CmdResult::success(BRIDGE_PORT)
}

/// Open an AI panel WebviewWindow. Idempotent — calling twice just focuses.
#[tauri::command]
pub fn open_panel(
    app:      AppHandle,
    state:    State<'_, AppState>,
    panel_id: String,
) -> CmdResult<()> {
    let (url, bridge_script) = {
        let panels = state.panels.lock().unwrap();
        match panels.get(&panel_id) {
            Some(p) => {
                let script = get_bridge_script(&panel_id).to_string();
                (p.url.clone(), script)
            }
            None => return CmdResult::fail(format!("Unknown panel_id: {panel_id}")),
        }
    };

    // Already open? Just focus it.
    if let Some(win) = app.get_webview_window(&panel_id) {
        let _ = win.show();
        let _ = win.set_focus();
        return CmdResult::success(());
    }

    let parsed_url: tauri::Url = match url.parse() {
        Ok(u) => u,
        Err(e) => return CmdResult::fail(format!("Bad URL: {e}")),
    };

    // ── Full initialization script ──
    // The outer IIFE provides shared helpers (trySelectors, report).
    // report() tries native Tauri IPC first, falls back to fetch.
    // This ensures it works even when the site's CSP blocks localhost fetch.
    let full_init_script = format!(
        r#"
(function() {{
    const PANEL_ID    = {panel_id_json};
    const BRIDGE_PORT = {port};
    const BRIDGE_URL  = `http://127.0.0.1:${{BRIDGE_PORT}}/bridge/event`;

    function trySelectors(selectors) {{
        for (const sel of selectors) {{
            try {{
                const el = document.querySelector(sel);
                if (el) return el;
            }} catch(e) {{}}
        }}
        return null;
    }}

    // report(): try native Tauri IPC first (bypasses CSP), fall back to fetch
    function report(type, extra) {{
        const payload = JSON.stringify({{ type, panel_id: PANEL_ID, ...extra }});

        // Attempt 1: Tauri native IPC (works even when CSP blocks fetch)
        try {{
            if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {{
                window.__TAURI_INTERNALS__.invoke('bridge_event', {{ type, panelId: PANEL_ID, ...extra }})
                    .catch(() => {{
                        // Tauri invoke failed, try fetch
                        reportViaFetch(payload);
                    }});
                return;
            }}
        }} catch(e) {{}}

        // Attempt 2: fetch to local bridge server
        reportViaFetch(payload);
    }}

    function reportViaFetch(payload) {{
        fetch(BRIDGE_URL, {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: payload,
        }}).catch(err => {{
            console.error('[OrchestratorBridge] both IPC and fetch failed:', err);
        }});
    }}

    window.__orchestratorBridge = {{
        panelId: PANEL_ID,
        report,
        trySelectors,
        sendMessage: function(text) {{
            console.error('[OrchestratorBridge] sendMessage not implemented for', PANEL_ID);
        }},
    }};

    {bridge_script}

    // Fire ready after page settles
    function fireReady() {{
        setTimeout(() => report('ready', {{}}), 1200);
    }}

    if (document.readyState === 'complete') {{
        fireReady();
    }} else {{
        window.addEventListener('load', fireReady);
    }}

    console.log('[OrchestratorBridge] injected for', PANEL_ID);
}})();
"#,
        panel_id_json = serde_json::to_string(&panel_id).unwrap(),
        port = BRIDGE_PORT,
        bridge_script = bridge_script,
    );

    let result = WebviewWindowBuilder::new(
        &app,
        &panel_id,
        WebviewUrl::External(parsed_url),
    )
    .title(format!("Vibe — {panel_id}"))
    .inner_size(980.0, 820.0)
    .min_inner_size(600.0, 500.0)
    .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
    .initialization_script(&full_init_script)
    .build();

    match result {
        Ok(_win) => {
            state.set_status(&panel_id, PanelStatus::Loading);

            // Fallback: if the JS bridge never fires (CSP blocks both IPC and fetch),
            // optimistically mark the panel Idle after 10 seconds so the UI isn't stuck.
            let state_clone = app.state::<AppState>().clone();
            let panel_id_clone = panel_id.clone();
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                // Only promote if still Loading (don't override a real ready/error)
                let should_promote = {
                    let panels = state_clone.panels.lock().unwrap();
                    matches!(
                        panels.get(&panel_id_clone).map(|p| &p.status),
                        Some(PanelStatus::Loading)
                    )
                };
                if should_promote && app_clone.get_webview_window(&panel_id_clone).is_some() {
                    state_clone.set_status(&panel_id_clone, PanelStatus::Idle);
                    let _ = app_clone.emit(
                        crate::bridge_server::EVENT_PANEL_READY,
                        crate::bridge_server::PanelEventPayload {
                            panel_id: panel_id_clone,
                            output: None,
                            message: None,
                        },
                    );
                }
            });

            CmdResult::success(())
        }
        Err(e) => CmdResult::fail(format!("WebviewWindowBuilder failed: {e}")),
    }
}

/// Close a panel WebviewWindow.
#[tauri::command]
pub fn close_panel(
    app:      AppHandle,
    state:    State<'_, AppState>,
    panel_id: String,
) -> CmdResult<()> {
    if let Some(win) = app.get_webview_window(&panel_id) {
        let _ = win.close();
        state.set_status(&panel_id, PanelStatus::Closed);
        CmdResult::success(())
    } else {
        // Window already gone — just sync state
        state.set_status(&panel_id, PanelStatus::Closed);
        CmdResult::success(())
    }
}

/// Send a message to a panel — injects it and submits.
#[tauri::command]
pub fn send_to_panel(
    app:      AppHandle,
    state:    State<'_, AppState>,
    panel_id: String,
    message:  String,
) -> CmdResult<()> {
    let win = match app.get_webview_window(&panel_id) {
        Some(w) => w,
        None => return CmdResult::fail(format!("Panel {panel_id} is not open")),
    };

    let escaped_msg = message
        .replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace("${", "\\${");

    let js = format!(
        r#"
(function() {{
    if (!window.__orchestratorBridge || !window.__orchestratorBridge.sendMessage) {{
        console.error('[OrchestratorBridge] bridge not ready on {panel_id}');
        return;
    }}
    window.__orchestratorBridge.sendMessage(`{msg}`);
}})();
"#,
        panel_id = panel_id,
        msg = escaped_msg,
    );

    match win.eval(&js) {
        Ok(_) => {
            state.set_status(&panel_id, PanelStatus::Generating);
            CmdResult::success(())
        }
        Err(e) => CmdResult::fail(format!("eval failed: {e}")),
    }
}

/// Add a Tauri command handler for native IPC bridge events from injected JS.
/// This is called when window.__TAURI_INTERNALS__.invoke('bridge_event', ...) fires.
#[tauri::command]
pub fn bridge_event(
    app:      AppHandle,
    state:    State<'_, AppState>,
    #[allow(non_snake_case)]
    r#type:   String,
    #[allow(non_snake_case)]
    panelId:  String,
    output:   Option<String>,
    message:  Option<String>,
) -> CmdResult<()> {
    use crate::bridge_server::{
        EVENT_PANEL_ERROR, EVENT_PANEL_GENERATING, EVENT_PANEL_OUTPUT,
        EVENT_PANEL_READY, PanelEventPayload,
    };

    match r#type.as_str() {
        "output" => {
            let out = output.unwrap_or_default();
            state.store_output(&panelId, out.clone());
            let _ = app.emit(EVENT_PANEL_OUTPUT, PanelEventPayload {
                panel_id: panelId, output: Some(out), message: None,
            });
        }
        "ready" => {
            state.set_status(&panelId, PanelStatus::Idle);
            let _ = app.emit(EVENT_PANEL_READY, PanelEventPayload {
                panel_id: panelId, output: None, message: None,
            });
        }
        "generating" => {
            state.set_status(&panelId, PanelStatus::Generating);
            let _ = app.emit(EVENT_PANEL_GENERATING, PanelEventPayload {
                panel_id: panelId, output: None, message: None,
            });
        }
        "error" => {
            let msg = message.unwrap_or_else(|| "unknown error".into());
            state.set_status(&panelId, PanelStatus::Error { message: msg.clone() });
            let _ = app.emit(EVENT_PANEL_ERROR, PanelEventPayload {
                panel_id: panelId, output: None, message: Some(msg),
            });
        }
        _ => {}
    }

    CmdResult::success(())
}

/// Clear stored output for a panel and reset it to Idle.
#[tauri::command]
pub fn reset_panel_output(
    state:    State<'_, AppState>,
    panel_id: String,
) -> CmdResult<()> {
    let mut panels = state.panels.lock().unwrap();
    match panels.get_mut(&panel_id) {
        Some(p) => {
            p.last_output = None;
            p.status = PanelStatus::Idle;
            CmdResult::success(())
        }
        None => CmdResult::fail(format!("Unknown panel_id: {panel_id}")),
    }
}
