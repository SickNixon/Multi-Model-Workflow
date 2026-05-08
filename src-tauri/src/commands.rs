// src-tauri/src/commands.rs
// All Tauri commands exposed to the frontend via invoke().
// Each command is a thin layer — real logic lives in state.rs / bridge_server.rs.
//
// COMMAND INVENTORY:
//   get_panel_states     → returns current state of all panels
//   open_panel           → creates a WebviewWindow for an AI site
//   close_panel          → closes a WebviewWindow
//   send_to_panel        → injects text + submits in a WebviewWindow
//   reset_panel_output   → clears last_output for a panel
//   get_bridge_port      → returns the HTTP bridge port (for UI display)

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
#[tauri::command]
pub fn get_panel_states(state: State<'_, AppState>) -> CmdResult<Vec<PanelInfo>> {
    let panels = state.panels.lock().unwrap();
    let mut list: Vec<PanelInfo> = panels.values().cloned().collect();
    // Stable ordering: use the ALL_PANELS const order
    list.sort_by_key(|p| {
        crate::state::ALL_PANELS
            .iter()
            .position(|&id| id == p.id)
            .unwrap_or(99)
    });
    CmdResult::success(list)
}

/// Return the bridge HTTP port (so the UI can display it).
#[tauri::command]
pub fn get_bridge_port() -> CmdResult<u16> {
    CmdResult::success(BRIDGE_PORT)
}

/// Open an AI panel WebviewWindow. Idempotent — calling twice just focuses.
#[tauri::command]
pub fn open_panel(
    app:     AppHandle,
    state:   State<'_, AppState>,
    panel_id: String,
) -> CmdResult<()> {
    // Validate panel ID
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

    // Parse URL
    let parsed_url: tauri::Url = match url.parse() {
        Ok(u) => u,
        Err(e) => return CmdResult::fail(format!("Bad URL: {e}")),
    };

    // Build the full initialization script:
    // 1. Inject the bridge sentinel + site-specific logic
    // 2. Wrap in a self-invoking function to avoid polluting global scope further
    let full_init_script = format!(
        r#"
// ── Vibe Orchestrator Bridge — initialisation script ──
// This runs before the page's own JS on every navigation.
(function() {{
    const PANEL_ID    = {panel_id_json};
    const BRIDGE_PORT = {port};
    const BRIDGE_URL  = `http://127.0.0.1:${{BRIDGE_PORT}}/bridge/event`;

    // ── Shared helper: try selectors in order, return first match ──
    function trySelectors(selectors) {{
        for (const sel of selectors) {{
            try {{
                const el = document.querySelector(sel);
                if (el) return el;
            }} catch(e) {{ /* invalid selector — skip */ }}
        }}
        return null;
    }}

    // Report helper — fire-and-forget POST to our local bridge server
    function report(type, extra) {{
        fetch(BRIDGE_URL, {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify({{ type, panel_id: PANEL_ID, ...extra }})
        }}).catch(err => {{
            console.error('[OrchestratorBridge] report failed:', err);
        }});
    }}

    // Attach the bridge object — site-specific code fills in sendMessage()
    window.__orchestratorBridge = {{
        panelId: PANEL_ID,
        report,
        trySelectors,
        // Placeholder — overwritten by site-specific init below
        sendMessage: function(text) {{
            console.error('[OrchestratorBridge] sendMessage not implemented for', PANEL_ID);
        }},
    }};

    // Site-specific bridge implementation
    {bridge_script}

    // Tell the Rust backend we're alive
    // Use a small delay to let the page DOM settle first
    window.addEventListener('load', function() {{
        setTimeout(() => report('ready', {{}}), 800);
    }});

    console.log('[OrchestratorBridge] sentinel injected for', PANEL_ID);
}})();
"#,
        panel_id_json = serde_json::to_string(&panel_id).unwrap(),
        port = BRIDGE_PORT,
        bridge_script = bridge_script,
    );

    // Create the WebviewWindow
    let result = WebviewWindowBuilder::new(
        &app,
        &panel_id,
        WebviewUrl::External(parsed_url),
    )
    .title(format!("Vibe — {panel_id}"))
    .inner_size(980.0, 820.0)
    .min_inner_size(600.0, 500.0)
    .initialization_script(&full_init_script)
    // Disable Tauri's default CSP override so the external site loads normally
    .build();

    match result {
        Ok(_) => {
            state.set_status(&panel_id, PanelStatus::Loading);
            CmdResult::success(())
        }
        Err(e) => CmdResult::fail(format!("WebviewWindowBuilder failed: {e}")),
    }
}

/// Close a panel WebviewWindow.
#[tauri::command]
pub fn close_panel(
    app:     AppHandle,
    state:   State<'_, AppState>,
    panel_id: String,
) -> CmdResult<()> {
    if let Some(win) = app.get_webview_window(&panel_id) {
        let _ = win.close();
        state.set_status(&panel_id, PanelStatus::Closed);
        CmdResult::success(())
    } else {
        CmdResult::fail(format!("Panel {panel_id} is not open"))
    }
}

/// Send a message to a panel — injects it into the input and submits.
/// The panel must be open and in Idle or Done state.
#[tauri::command]
pub fn send_to_panel(
    app:     AppHandle,
    state:   State<'_, AppState>,
    panel_id: String,
    message: String,
) -> CmdResult<()> {
    let win = match app.get_webview_window(&panel_id) {
        Some(w) => w,
        None => return CmdResult::fail(format!("Panel {panel_id} is not open")),
    };

    // Escape message for safe embedding in a JS template literal.
    // Template literals only need backtick and ${} escaped.
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

/// Clear stored output for a panel and reset it to Idle.
#[tauri::command]
pub fn reset_panel_output(
    state:   State<'_, AppState>,
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
