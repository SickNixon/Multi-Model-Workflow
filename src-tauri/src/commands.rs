// src-tauri/src/commands.rs

use crate::bridge::get_bridge_script;
use crate::bridge_server::BRIDGE_PORT;
use crate::state::{AppState, PanelInfo, PanelStatus};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

#[derive(Debug, serde::Serialize)]
pub struct CmdResult<T: serde::Serialize> {
    pub ok:    bool,
    pub data:  Option<T>,
    pub error: Option<String>,
}
impl<T: serde::Serialize> CmdResult<T> {
    fn success(data: T) -> Self { Self { ok: true, data: Some(data), error: None } }
    fn fail(msg: impl Into<String>) -> Self { Self { ok: false, data: None, error: Some(msg.into()) } }
}

// ── Anti-bot script ───────────────────────────────────────────────────────────
// Hides WKWebView fingerprints that Cloudflare Turnstile detects.
// Most importantly: window.webkit.messageHandlers — present in WKWebView, absent in real browsers.
const ANTI_BOT_SCRIPT: &str = r#"
(function() {
    try { Object.defineProperty(navigator, 'webdriver', { get: () => false, configurable: true }); } catch(e) {}

    // THIS IS THE BIG ONE: Cloudflare checks for window.webkit.messageHandlers
    // Real Safari has window.webkit but no messageHandlers. WKWebView has both.
    try {
        if (window.webkit && window.webkit.messageHandlers) {
            Object.defineProperty(window, 'webkit', { get: () => ({}), configurable: true, enumerable: false });
        }
    } catch(e) {}

    try {
        Object.defineProperty(navigator, 'plugins', {
            get: () => { const p = [
                { name: 'Chrome PDF Plugin', filename: 'internal-pdf-viewer', description: '' },
                { name: 'Chrome PDF Viewer', filename: 'mhjfbmdgcfjbbpaeojofohoefgiehjai', description: '' },
                { name: 'Native Client', filename: 'internal-nacl-plugin', description: '' },
            ]; p.__proto__ = PluginArray.prototype; return p; }, configurable: true
        });
    } catch(e) {}

    try { Object.defineProperty(navigator, 'languages', { get: () => ['en-US', 'en'], configurable: true }); } catch(e) {}
    try { if (!window.chrome) window.chrome = { runtime: {}, loadTimes: function(){}, csi: function(){}, app: {} }; } catch(e) {}
    try {
        const orig = navigator.permissions.query.bind(navigator.permissions);
        navigator.permissions.query = (p) => p.name === 'notifications'
            ? Promise.resolve({ state: Notification.permission }) : orig(p);
    } catch(e) {}
})();
"#;

// ── Init script builder ───────────────────────────────────────────────────────

fn build_init_script(panel_id: &str, bridge_script: &str) -> String {
    format!(r#"
{anti_bot}
(function() {{
    const PANEL_ID    = {panel_id_json};
    const BRIDGE_PORT = {port};

    function trySelectors(selectors) {{
        for (const sel of selectors) {{
            try {{ const el = document.querySelector(sel); if (el) return el; }} catch(e) {{}}
        }}
        return null;
    }}

    // Image beacon: bypasses connect-src CSP — img-src is usually unrestricted
    function report(type, extra) {{
        const json  = JSON.stringify({{ type, panel_id: PANEL_ID, ...extra }});
        const CHUNK = 1800;
        const total = Math.ceil(json.length / CHUNK);
        const id    = `${{Date.now()}}-${{Math.random().toString(36).slice(2)}}`;
        for (let i = 0; i < total; i++) {{
            const img = new Image();
            img.src   = `http://127.0.0.1:${{BRIDGE_PORT}}/ping?id=${{id}}&i=${{i}}&t=${{total}}&d=${{encodeURIComponent(json.slice(i*CHUNK,(i+1)*CHUNK))}}`;
        }}
    }}

    window.__orchestratorBridge = {{ panelId: PANEL_ID, report, trySelectors,
        sendMessage: function(text) {{ console.error('[OrchestratorBridge] sendMessage not implemented for', PANEL_ID); }},
    }};

    {bridge_script}

    function fireReady() {{ setTimeout(() => report('ready', {{}}), 1500); }}
    if (document.readyState === 'complete') {{ fireReady(); }}
    else {{ window.addEventListener('load', fireReady); }}

    console.log('[OrchestratorBridge] injected for', PANEL_ID);
}})();
"#,
        anti_bot      = ANTI_BOT_SCRIPT,
        panel_id_json = serde_json::to_string(panel_id).unwrap(),
        port          = BRIDGE_PORT,
        bridge_script = bridge_script,
    )
}

// ── Idle fallback timer ───────────────────────────────────────────────────────

fn schedule_idle_fallback(app: AppHandle, panel_id: String) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(12)).await;
        let state = app.state::<AppState>();
        let promote = {
            let panels = state.panels.lock().unwrap();
            matches!(panels.get(&panel_id).map(|p| &p.status), Some(PanelStatus::Loading))
        };
        if promote && app.get_webview_window(&panel_id).is_some() {
            state.set_status(&panel_id, PanelStatus::Idle);
            let _ = app.emit(crate::bridge_server::EVENT_PANEL_READY,
                crate::bridge_server::PanelEventPayload { panel_id, output: None, message: None });
        }
    });
}

// ── Core open logic (shared by command + auto-open on startup) ────────────────

pub fn open_panel_window(app: &AppHandle, panel_id: &str) {
    if app.get_webview_window(panel_id).is_some() { return; }

    let state = app.state::<AppState>();
    let (url, bridge_script) = {
        let panels = state.panels.lock().unwrap();
        match panels.get(panel_id) {
            Some(p) => (p.url.clone(), get_bridge_script(panel_id).to_string()),
            None => return,
        }
    };

    let parsed_url: tauri::Url = match url.parse() { Ok(u) => u, Err(_) => return };
    let init_script = build_init_script(panel_id, &bridge_script);

    let result = WebviewWindowBuilder::new(app, panel_id, WebviewUrl::External(parsed_url))
        .title(format!("Vibe — {panel_id}"))
        .inner_size(1024.0, 860.0)
        .min_inner_size(600.0, 500.0)
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
        .initialization_script(&init_script)
        .data_directory(dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("vibe-orchestrator")
            .join(panel_id))
        .visible(false)
        .build();

    if result.is_ok() {
        state.set_status(panel_id, PanelStatus::Loading);
        schedule_idle_fallback(app.clone(), panel_id.to_string());
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_panel_states(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Vec<PanelInfo>> {
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
    list.sort_by_key(|p| crate::state::ALL_PANELS.iter().position(|&id| id == p.id).unwrap_or(99));
    CmdResult::success(list)
}

#[tauri::command]
pub fn get_bridge_port() -> CmdResult<u16> { CmdResult::success(BRIDGE_PORT) }

#[tauri::command]
pub fn open_panel(app: AppHandle, state: State<'_, AppState>, panel_id: String) -> CmdResult<()> {
    if let Some(win) = app.get_webview_window(&panel_id) {
        let _ = win.show(); let _ = win.set_focus();
        return CmdResult::success(());
    }
    if !state.panels.lock().unwrap().contains_key(&panel_id) {
        return CmdResult::fail(format!("Unknown panel_id: {panel_id}"));
    }
    open_panel_window(&app, &panel_id);
    CmdResult::success(())
}

#[tauri::command]
pub fn show_panel(app: AppHandle, panel_id: String) -> CmdResult<()> {
    match app.get_webview_window(&panel_id) {
        Some(win) => { let _ = win.show(); let _ = win.set_focus(); CmdResult::success(()) }
        None => CmdResult::fail(format!("Panel {panel_id} not open")),
    }
}

#[tauri::command]
pub fn hide_panel(app: AppHandle, panel_id: String) -> CmdResult<()> {
    match app.get_webview_window(&panel_id) {
        Some(win) => { let _ = win.hide(); CmdResult::success(()) }
        None => CmdResult::fail(format!("Panel {panel_id} not open")),
    }
}

#[tauri::command]
pub fn close_panel(app: AppHandle, state: State<'_, AppState>, panel_id: String) -> CmdResult<()> {
    if let Some(win) = app.get_webview_window(&panel_id) { let _ = win.close(); }
    state.set_status(&panel_id, PanelStatus::Closed);
    CmdResult::success(())
}

#[tauri::command]
pub fn send_to_panel(app: AppHandle, state: State<'_, AppState>, panel_id: String, message: String) -> CmdResult<()> {
    let win = match app.get_webview_window(&panel_id) {
        Some(w) => w,
        None => return CmdResult::fail(format!("Panel {panel_id} not open")),
    };
    let escaped = message.replace('\\', "\\\\").replace('`', "\\`").replace("${", "\\${");
    let js = format!(r#"(function(){{
    if (!window.__orchestratorBridge?.sendMessage) {{ console.error('[OrchestratorBridge] not ready on {pid}'); return; }}
    window.__orchestratorBridge.sendMessage(`{msg}`);
}})();"#, pid=panel_id, msg=escaped);
    match win.eval(&js) {
        Ok(_) => { state.set_status(&panel_id, PanelStatus::Generating); CmdResult::success(()) }
        Err(e) => CmdResult::fail(format!("eval failed: {e}")),
    }
}

#[tauri::command]
pub fn bridge_event(
    app:     AppHandle,
    state:   State<'_, AppState>,
    r#type:  String,
    #[allow(non_snake_case)] panelId: String,
    output:  Option<String>,
    message: Option<String>,
) -> CmdResult<()> {
    use crate::bridge_server::*;
    match r#type.as_str() {
        "output" => {
            let out = output.unwrap_or_default();
            state.store_output(&panelId, out.clone());
            let _ = app.emit(EVENT_PANEL_OUTPUT, PanelEventPayload { panel_id: panelId, output: Some(out), message: None });
        }
        "ready" => {
            state.set_status(&panelId, PanelStatus::Idle);
            let _ = app.emit(EVENT_PANEL_READY, PanelEventPayload { panel_id: panelId, output: None, message: None });
        }
        "generating" => {
            state.set_status(&panelId, PanelStatus::Generating);
            let _ = app.emit(EVENT_PANEL_GENERATING, PanelEventPayload { panel_id: panelId, output: None, message: None });
        }
        "error" => {
            let msg = message.unwrap_or_else(|| "unknown".into());
            state.set_status(&panelId, PanelStatus::Error { message: msg.clone() });
            let _ = app.emit(EVENT_PANEL_ERROR, PanelEventPayload { panel_id: panelId, output: None, message: Some(msg) });
        }
        _ => {}
    }
    CmdResult::success(())
}

#[tauri::command]
pub fn reset_panel_output(state: State<'_, AppState>, panel_id: String) -> CmdResult<()> {
    let mut panels = state.panels.lock().unwrap();
    match panels.get_mut(&panel_id) {
        Some(p) => { p.last_output = None; p.status = PanelStatus::Idle; CmdResult::success(()) }
        None => CmdResult::fail(format!("Unknown panel_id: {panel_id}")),
    }
}

/// Manually trigger output capture in a panel — called by the CAPTURE button.
/// Evals JS that calls the panel's own captureOutput() function.
#[tauri::command]
pub fn capture_panel_output(app: AppHandle, panel_id: String) -> CmdResult<()> {
    let win = match app.get_webview_window(&panel_id) {
        Some(w) => w,
        None => return CmdResult::fail(format!("Panel {panel_id} not open")),
    };
    let js = r#"
(function() {
    if (window.__orchestratorBridge?.captureOutput) {
        window.__orchestratorBridge.captureOutput();
    } else {
        // Generic fallback: grab last big text block on the page
        const blocks = Array.from(document.querySelectorAll('p, div, article'))
            .filter(el => el.offsetParent && el.innerText?.trim().length > 50);
        const last = blocks[blocks.length - 1];
        if (last && window.__orchestratorBridge) {
            window.__orchestratorBridge.report('output', { output: last.innerText.trim() });
        }
    }
})();
"#;
    let _ = win.eval(js);
    CmdResult::success(())
}
