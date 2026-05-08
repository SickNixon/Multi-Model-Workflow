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

// ── Anti-bot / browser spoofing script ───────────────────────────────────────
// Injected before page JS runs. Tricks Cloudflare and others into thinking
// this is a real Chrome browser session. Runs on every navigation.
const ANTI_BOT_SCRIPT: &str = r#"
(function() {
    // Remove WebDriver flag (biggest Cloudflare trigger)
    try {
        Object.defineProperty(navigator, 'webdriver', { get: () => false, configurable: true });
    } catch(e) {}

    // Fake plugins (real browsers have these)
    try {
        Object.defineProperty(navigator, 'plugins', {
            get: () => {
                const p = [
                    { name: 'Chrome PDF Plugin', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
                    { name: 'Chrome PDF Viewer', filename: 'mhjfbmdgcfjbbpaeojofohoefgiehjai', description: '' },
                    { name: 'Native Client', filename: 'internal-nacl-plugin', description: '' },
                ];
                p.__proto__ = PluginArray.prototype;
                return p;
            },
            configurable: true
        });
    } catch(e) {}

    // Languages
    try {
        Object.defineProperty(navigator, 'languages', { get: () => ['en-US', 'en'], configurable: true });
    } catch(e) {}

    // Chrome runtime object (WKWebView doesn't have this by default)
    try {
        if (!window.chrome) {
            window.chrome = { runtime: {}, loadTimes: function(){}, csi: function(){}, app: {} };
        }
    } catch(e) {}

    // Permissions API (Cloudflare checks this)
    try {
        const origQuery = window.navigator.permissions.query.bind(navigator.permissions);
        window.navigator.permissions.query = (params) => {
            if (params.name === 'notifications') {
                return Promise.resolve({ state: Notification.permission });
            }
            return origQuery(params);
        };
    } catch(e) {}
})();
"#;

#[tauri::command]
pub fn get_panel_states(
    app:   AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<Vec<PanelInfo>> {
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
        crate::state::ALL_PANELS.iter().position(|&id| id == p.id).unwrap_or(99)
    });
    CmdResult::success(list)
}

#[tauri::command]
pub fn get_bridge_port() -> CmdResult<u16> {
    CmdResult::success(BRIDGE_PORT)
}

#[tauri::command]
pub fn open_panel(
    app:      AppHandle,
    state:    State<'_, AppState>,
    panel_id: String,
) -> CmdResult<()> {
    let (url, bridge_script) = {
        let panels = state.panels.lock().unwrap();
        match panels.get(&panel_id) {
            Some(p) => (p.url.clone(), get_bridge_script(&panel_id).to_string()),
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

    let full_init_script = format!(
        r#"
{anti_bot}
(function() {{
    const PANEL_ID    = {panel_id_json};
    const BRIDGE_PORT = {port};
    const BRIDGE_URL  = `http://127.0.0.1:${{BRIDGE_PORT}}/bridge/event`;

    function trySelectors(selectors) {{
        for (const sel of selectors) {{
            try {{ const el = document.querySelector(sel); if (el) return el; }} catch(e) {{}}
        }}
        return null;
    }}

    function report(type, extra) {{
        const body = JSON.stringify({{ type, panel_id: PANEL_ID, ...extra }});
        // Try native Tauri IPC first (bypasses CSP entirely)
        try {{
            if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {{
                window.__TAURI_INTERNALS__.invoke('bridge_event', {{ type, panelId: PANEL_ID, ...extra }})
                    .catch(() => reportViaFetch(body));
                return;
            }}
        }} catch(e) {{}}
        reportViaFetch(body);
    }}

    function reportViaFetch(body) {{
        fetch(BRIDGE_URL, {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body,
        }}).catch(err => console.error('[OrchestratorBridge] fetch failed:', err));
    }}

    window.__orchestratorBridge = {{ panelId: PANEL_ID, report, trySelectors,
        sendMessage: function(text) {{
            console.error('[OrchestratorBridge] sendMessage not implemented for', PANEL_ID);
        }},
    }};

    {bridge_script}

    function fireReady() {{
        setTimeout(() => report('ready', {{}}), 1500);
    }}
    if (document.readyState === 'complete') {{ fireReady(); }}
    else {{ window.addEventListener('load', fireReady); }}

    console.log('[OrchestratorBridge] injected for', PANEL_ID);
}})();
"#,
        anti_bot      = ANTI_BOT_SCRIPT,
        panel_id_json = serde_json::to_string(&panel_id).unwrap(),
        port          = BRIDGE_PORT,
        bridge_script = bridge_script,
    );

    let result = WebviewWindowBuilder::new(
        &app,
        &panel_id,
        WebviewUrl::External(parsed_url),
    )
    .title(format!("Vibe — {panel_id}"))
    .inner_size(1024.0, 860.0)
    .min_inner_size(600.0, 500.0)
    .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
    .initialization_script(&full_init_script)
    // Persist cookies/session per panel so logins and CAPTCHA solves survive restarts
    .data_directory(dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("vibe-orchestrator")
        .join(&panel_id))
    .visible(false)
    .build();

    match result {
        Ok(_) => {
            state.set_status(&panel_id, PanelStatus::Loading);

            // 10s fallback: promote Loading→Idle if JS bridge never fires
            let app_clone      = app.clone();
            let panel_id_clone = panel_id.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                let state = app_clone.state::<AppState>();
                let should_promote = {
                    let panels = state.panels.lock().unwrap();
                    matches!(panels.get(&panel_id_clone).map(|p| &p.status), Some(PanelStatus::Loading))
                };
                if should_promote && app_clone.get_webview_window(&panel_id_clone).is_some() {
                    state.set_status(&panel_id_clone, PanelStatus::Idle);
                    let _ = app_clone.emit(
                        crate::bridge_server::EVENT_PANEL_READY,
                        crate::bridge_server::PanelEventPayload {
                            panel_id: panel_id_clone, output: None, message: None,
                        },
                    );
                }
            });

            CmdResult::success(())
        }
        Err(e) => CmdResult::fail(format!("WebviewWindowBuilder failed: {e}")),
    }
}

/// Show a panel window (bring it to front so user can interact / sign in).
#[tauri::command]
pub fn show_panel(app: AppHandle, panel_id: String) -> CmdResult<()> {
    match app.get_webview_window(&panel_id) {
        Some(win) => {
            let _ = win.show();
            let _ = win.set_focus();
            CmdResult::success(())
        }
        None => CmdResult::fail(format!("Panel {panel_id} is not open")),
    }
}

/// Hide a panel window (runs in background, bridge keeps working).
#[tauri::command]
pub fn hide_panel(app: AppHandle, panel_id: String) -> CmdResult<()> {
    match app.get_webview_window(&panel_id) {
        Some(win) => { let _ = win.hide(); CmdResult::success(()) }
        None => CmdResult::fail(format!("Panel {panel_id} is not open")),
    }
}

#[tauri::command]
pub fn close_panel(
    app:      AppHandle,
    state:    State<'_, AppState>,
    panel_id: String,
) -> CmdResult<()> {
    if let Some(win) = app.get_webview_window(&panel_id) {
        let _ = win.close();
    }
    state.set_status(&panel_id, PanelStatus::Closed);
    CmdResult::success(())
}

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

    let escaped = message.replace('\\', "\\\\").replace('`', "\\`").replace("${", "\\${");
    let js = format!(
        r#"(function() {{
    if (!window.__orchestratorBridge?.sendMessage) {{
        console.error('[OrchestratorBridge] bridge not ready on {pid}');
        return;
    }}
    window.__orchestratorBridge.sendMessage(`{msg}`);
}})();"#,
        pid = panel_id,
        msg = escaped,
    );

    match win.eval(&js) {
        Ok(_) => { state.set_status(&panel_id, PanelStatus::Generating); CmdResult::success(()) }
        Err(e) => CmdResult::fail(format!("eval failed: {e}")),
    }
}

#[tauri::command]
pub fn bridge_event(
    app:      AppHandle,
    state:    State<'_, AppState>,
    r#type:   String,
    #[allow(non_snake_case)] panelId: String,
    output:   Option<String>,
    message:  Option<String>,
) -> CmdResult<()> {
    use crate::bridge_server::{EVENT_PANEL_ERROR, EVENT_PANEL_GENERATING, EVENT_PANEL_OUTPUT, EVENT_PANEL_READY, PanelEventPayload};
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
            let msg = message.unwrap_or_else(|| "unknown error".into());
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
