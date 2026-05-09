// src-tauri/src/commands.rs

use crate::bridge::get_bridge_script;
use crate::bridge_server::BRIDGE_PORT;
use crate::state::{AppState, PanelInfo, PanelStatus};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_shell::ShellExt;
use std::fs;

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
const ANTI_BOT_SCRIPT: &str = r#"
(function() {
    try { Object.defineProperty(navigator, 'webdriver', { get: () => false, configurable: true }); } catch(e) {}
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

    window.__orchestratorState = {{
        panelId:    PANEL_ID,
        status:     'loading',
        output:     null,
        outputSeq:  0,
        error:      null,
    }};

    // PRIMARY: Native IPC (bypasses ALL CSP)
    function report(type, extra) {{
        const s = window.__orchestratorState;
        if (type === 'ready')      {{ s.status = 'ready'; }}
        if (type === 'generating') {{ s.status = 'generating'; }}
        if (type === 'error')      {{ s.status = 'error'; s.error = extra?.message ?? 'unknown'; }}
        if (type === 'output')     {{
            s.status    = 'done';
            s.output    = extra?.output ?? '';
            s.outputSeq = (s.outputSeq || 0) + 1;
        }}

        // IPC call – now available thanks to capabilities
        if (window.__TAURI_INTERNALS__?.invoke) {{
            window.__TAURI_INTERNALS__.invoke('bridge_event', {{
                event_type: type,
                panel_id:   PANEL_ID,
                output:     extra?.output ?? null,
                message:    extra?.message ?? null
            }}).catch(() => reportViaBeacon(type, extra));
        }} else {{
            reportViaBeacon(type, extra);
        }}
    }}

    function reportViaBeacon(type, extra) {{
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

    function fireReady() {{
        const check = window.__orchestratorBridge?.__readyCheck;
        if (typeof check === 'function') {{
            const result = check();
            if (result === false) return;
            if (typeof result === 'string') {{ report('error', {{ message: result }}); return; }}
        }}
        setTimeout(() => report('ready', {{}}), 1500);
    }}
    if (document.readyState === 'complete') {{ fireReady(); }}
    else {{ window.addEventListener('load', fireReady); }}

    // Auto‑diagnostic (kept)
    window.addEventListener('load', function() {{
        setTimeout(() => {{
            const diag = {{
                url: location.href,
                secureContext: window.isSecureContext,
                webkit: !!window.webkit,
                tauriInternals: !!window.__TAURI_INTERNALS__,
                textareas: Array.from(document.querySelectorAll('textarea')).slice(0,5).map(e => ({{
                    ph: (e.placeholder||'').slice(0,60), cls: (e.className||'').slice(0,60),
                    id: e.id, vis: !!e.offsetParent
                }})),
                contenteditables: Array.from(document.querySelectorAll('[contenteditable]')).slice(0,5).map(e => ({{
                    label: (e.getAttribute('aria-label')||'').slice(0,60),
                    cls: (e.className||'').slice(0,60), tag: e.tagName, vis: !!e.offsetParent
                }})),
                shadowHosts: Array.from(document.querySelectorAll('*')).filter(e=>e.shadowRoot).map(e=>e.tagName).slice(0,10),
                buttons: Array.from(document.querySelectorAll('button')).filter(e=>e.offsetParent).slice(0,12).map(e=>({{
                    label: (e.getAttribute('aria-label')||'').slice(0,40),
                    txt: (e.innerText||'').slice(0,20)
                }})),
                responseEls: ['model-response','message-content','[data-message-author-role]','[class*="markdown"]','[class*="response"]'].map(sel => {{
                    try {{ const els = document.querySelectorAll(sel); return {{sel, count: els.length, sampleClass: els[0]?.className?.slice(0,60)}}; }}
                    catch(ex) {{ return {{sel, err: ex.message}}; }}
                }})
            }};
            const img = new Image();
            img.src = `http://127.0.0.1:${{BRIDGE_PORT}}/diag?panel=${{PANEL_ID}}&data=${{encodeURIComponent(JSON.stringify(diag))}}`;
        }}, 4000);
    }});

    console.log('[OrchestratorBridge] injected for', PANEL_ID);
}})();
"#,
        anti_bot      = ANTI_BOT_SCRIPT,
        panel_id_json = serde_json::to_string(panel_id).unwrap(),
        port          = BRIDGE_PORT,
        bridge_script = bridge_script,
    )
}

// ── Idle fallback (skip Claude) ──────────────────────────────────────────────
fn schedule_idle_fallback(app: AppHandle, panel_id: String) {
    if panel_id == "claude" { return; }
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

// ── Open panel (shared) ──────────────────────────────────────────────────────
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

    if let Ok(win) = result {
        state.set_status(panel_id, PanelStatus::Loading);
        schedule_idle_fallback(app.clone(), panel_id.to_string());

        // Close button → hide instead of destroy.
        // This keeps the WebView (and its session) alive in the background.
        let win_clone = win.clone();
        win.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = win_clone.hide();
            }
        });
    }
}

// ── Commands ─────────────────────────────────────────────────────────────────

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
    if (window.__orchestratorState) {{
        window.__orchestratorState.status    = 'generating';
        window.__orchestratorState.output    = null;
        window.__orchestratorState.outputSeq = window.__orchestratorState.outputSeq || 0;
    }}
    window.__orchestratorBridge.sendMessage(`{msg}`);
}})();"#, pid = panel_id, msg = escaped);

    match win.eval(&js) {
        Ok(_) => {
            state.set_status(&panel_id, PanelStatus::Generating);
            let _ = app.emit(
                crate::bridge_server::EVENT_PANEL_GENERATING,
                crate::bridge_server::PanelEventPayload { panel_id: panel_id.clone(), output: None, message: None },
            );
            // IPC blocked by connect-src CSP, beacon blocked by img-src CSP,
            // postMessage unregistered for external WebViews in Tauri 2.
            // Title-poll is the only CSP-immune channel: eval() writes, win.title() reads.
            spawn_title_poll(app.clone(), panel_id);
            CmdResult::success(())
        }
        Err(e) => CmdResult::fail(format!("eval failed: {e}")),
    }
}

/// Polls the WebView for output by encoding it in document.title.
/// eval() pushes JS natively (no CSP). win.title() reads NSWindow.title (no network).
fn spawn_title_poll(app: AppHandle, panel_id: String) {
    tauri::async_runtime::spawn(async move {
        const POLL_MS: u64 = 1500;
        const TIMEOUT_SECS: u64 = 180;
        const PREFIX: &str = "__VOUT__:";

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TIMEOUT_SECS);

        let poll_js = r#"(function(){
    var s = window.__orchestratorState;
    if (!s || s.status !== 'done' || window.__vibe_title_set) return;
    window.__vibe_title_set = true;
    try { document.title = '__VOUT__:' + JSON.stringify(s.output || ''); } catch(e) {}
})();"#;

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
            if std::time::Instant::now() > deadline { break; }

            let win = match app.get_webview_window(&panel_id) {
                Some(w) => w,
                None    => break,
            };

            // Already delivered via IPC (e.g. DeepSeek beacon works) — bail.
            {
                let state = app.state::<AppState>();   // <-- garde le State vivant
                let panels = state.panels.lock().unwrap();
                if panels.get(&panel_id).and_then(|p| p.last_output.as_ref()).is_some() {
                    break;
                }
            }

            let _ = win.eval(poll_js);
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            let title = match win.title() { Ok(t) => t, Err(_) => continue };
            if !title.starts_with(PREFIX) { continue; }

            let output = match serde_json::from_str::<String>(&title[PREFIX.len()..]) {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Restore window title.
            let _ = win.eval("if(window.__vibe_title_set){document.title=window.__vibe_orig_title||'';window.__vibe_title_set=false;}");

            let app_state = app.state::<AppState>();
            if app_state.store_output(&panel_id, output.clone()) {
                use crate::bridge_server::*;
                let _ = app.emit(EVENT_PANEL_OUTPUT, PanelEventPayload {
                    panel_id: panel_id.clone(), output: Some(output), message: None,
                });
            }
            break;
        }
    });
}

#[tauri::command]
pub fn bridge_event(
    app:        AppHandle,
    state:      State<'_, AppState>,
    event_type: String,
    panel_id:   String,
    output:     Option<String>,
    message:    Option<String>,
) -> CmdResult<()> {
    use crate::bridge_server::*;
    match event_type.as_str() {
        "output" => {
            let out = output.unwrap_or_default();
            if state.store_output(&panel_id, out.clone()) {
                let _ = app.emit(EVENT_PANEL_OUTPUT, PanelEventPayload { panel_id, output: Some(out), message: None });
            }
        }
        "ready" => {
            state.set_status(&panel_id, PanelStatus::Idle);
            let _ = app.emit(EVENT_PANEL_READY, PanelEventPayload { panel_id, output: None, message: None });
        }
        "generating" => {
            state.set_status(&panel_id, PanelStatus::Generating);
            let _ = app.emit(EVENT_PANEL_GENERATING, PanelEventPayload { panel_id, output: None, message: None });
        }
        "error" => {
            let msg = message.unwrap_or_else(|| "unknown".into());
            state.set_status(&panel_id, PanelStatus::Error { message: msg.clone() });
            let _ = app.emit(EVENT_PANEL_ERROR, PanelEventPayload { panel_id, output: None, message: Some(msg) });
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

#[tauri::command]
pub fn capture_panel_output(app: AppHandle, panel_id: String) -> CmdResult<()> {
    let win = match app.get_webview_window(&panel_id) {
        Some(w) => w,
        None => return CmdResult::fail(format!("Panel {panel_id} not open")),
    };

    let pid = serde_json::to_string(&panel_id).unwrap();
    let js = format!(r#"
(function() {{
    function sendOutput(text) {{
        if (window.__TAURI_INTERNALS__?.invoke) {{
            window.__TAURI_INTERNALS__.invoke('bridge_event', {{
                event_type: 'output',
                panel_id:   {pid},
                output:     text,
                message:    null,
            }}).catch(() => sendViaBeacon(text));
            return;
        }}
        sendViaBeacon(text);
    }}
    function sendViaBeacon(text) {{
        const payload = JSON.stringify({{ type: "output", panel_id: {pid}, output: text }});
        const CHUNK = 1800;
        const total = Math.ceil(payload.length / CHUNK);
        const id    = Date.now() + '-cap';
        for (let i = 0; i < total; i++) {{
            const img = new Image();
            img.src = `http://127.0.0.1:{port}/ping?id=${{id}}&i=${{i}}&t=${{total}}&d=${{encodeURIComponent(payload.slice(i*CHUNK,(i+1)*CHUNK))}}`;
        }}
    }}

    if (window.__orchestratorBridge?.captureOutput) {{
        window.__orchestratorBridge.captureOutput();
        return;
    }}

    const diag = {{
        bridgeInjected: !!window.__orchestratorBridge,
        url: location.href,
        isSecureContext: window.isSecureContext,
        webkit: !!window.webkit,
        tauriInternals: !!window.__TAURI_INTERNALS__,
        textareas: Array.from(document.querySelectorAll('textarea')).slice(0,6).map(e => ({{
            ph: (e.placeholder||'').slice(0,80), cls: (e.className||'').slice(0,80), id: e.id, visible: !!e.offsetParent
        }})),
        contenteditables: Array.from(document.querySelectorAll('[contenteditable]')).slice(0,6).map(e => ({{
            tag: e.tagName, label: (e.getAttribute('aria-label')||'').slice(0,80), cls: (e.className||'').slice(0,80), visible: !!e.offsetParent
        }})),
        visibleButtons: Array.from(document.querySelectorAll('button')).slice(0,15).filter(e=>e.offsetParent).map(e=>({{
            label: (e.getAttribute('aria-label')||'').slice(0,50), txt: (e.innerText||'').slice(0,30)
        }})),
    }};
    sendOutput('DIAGNOSTIC: ' + JSON.stringify(diag, null, 2));
}})();
"#, pid = pid, port = BRIDGE_PORT);

    let _ = win.eval(&js);
    CmdResult::success(())
}

// ── NEW: Actually working reset and browser open ─────────────────────────────

#[tauri::command]
pub fn reset_claude_session(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    // Close Claude window if open
    if let Some(win) = app.get_webview_window("claude") {
        let _ = win.close();
    }
    state.set_status("claude", PanelStatus::Closed);

    // Wipe the session directory so Turnstile is fresh
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("vibe-orchestrator")
        .join("claude");
    if data_dir.exists() {
        let _ = fs::remove_dir_all(&data_dir);
        let _ = fs::create_dir_all(&data_dir);
    }

    CmdResult::success(())
}

#[tauri::command]
#[allow(deprecated)]
pub fn open_in_browser(app: AppHandle, url: String) -> CmdResult<()> {
    if let Err(e) = app.shell().open(url, None) {
        return CmdResult::fail(format!("Failed to open browser: {e}"));
    }
    CmdResult::success(())
}

/// Opens the WebView DevTools for the given panel — dev/debug use only.
/// Lets you inspect the DOM, see console output, and verify
/// `window.__TAURI_INTERNALS__` is available in external WebViews.
#[tauri::command]
pub fn open_panel_devtools(app: AppHandle, panel_id: String) -> CmdResult<()> {
    match app.get_webview_window(&panel_id) {
        Some(win) => { win.open_devtools(); CmdResult::success(()) }
        None => CmdResult::fail(format!("Panel {panel_id} not open")),
    }
}