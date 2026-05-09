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
    // Remove WebDriver flag — biggest automation detector
    try { Object.defineProperty(navigator, 'webdriver', { get: () => false, configurable: true }); } catch(e) {}

    // NOTE: We do NOT hide window.webkit.messageHandlers because Tauri's native
    // IPC bridge depends on it. Without it, __TAURI_INTERNALS__.invoke() fails,
    // which is our primary channel for getting output back from AI panels.

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

    // Two-path report — covers all sites regardless of CSP strictness:
    //
    // PRIMARY: invoke('bridge_event') — custom app commands bypass Tauri's ACL
    //   system entirely and work from ANY WebView, including external sites like
    //   Gemini and Grok. Goes via webkit.messageHandlers (native bridge), which
    //   CSP cannot block. The Rust handler calls app.emit() to broadcast to ALL
    //   windows, so the React orchestrator window hears it via listen().
    //
    // WHY NOT plugin:event|emit: That is a Tauri plugin command — it requires
    //   core:event:allow-emit in capabilities. Panel windows were not in any
    //   capability, so it silently rejected → fell through to beacon → Gemini/Grok
    //   CSP blocked the beacon → nothing. That was the entire bug.
    //
    // FALLBACK: image beacon — belt-and-suspenders if __TAURI_INTERNALS__ absent.
    //   Still works for DeepSeek (permissive img-src CSP).
    // State store — Rust polls this via eval() instead of waiting for push events.
    // This sidesteps ALL CSP and IPC permission issues entirely.
    // Rust calls win.eval("JSON.stringify(window.__orchestratorBridge.__state)")
    // and gets back the current state snapshot.
    window.__orchestratorState = {{
        panelId:    PANEL_ID,
        status:     'loading',   // loading | ready | generating | done | error
        output:     null,
        outputSeq:  0,           // increments each time new output is stored
        error:      null,
    }};

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
        // Also try beacon as opportunistic push for DeepSeek (permissive CSP)
        reportViaBeacon(type, extra);
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
        // Bridge can veto or override the ready signal by setting __readyCheck.
        // Return true = proceed normally. Return string = report that as error msg.
        // Return false = veto silently (bridge will fire ready itself when appropriate).
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

    // Auto-diagnostic: 4 seconds after load, dump DOM structure to /tmp
    // so the dev can read it without opening DevTools
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
                buttons: Array.from(document.querySelectorAll('button')).filter(e=>e.offsetParent).slice(0,12).map(e=>(({{
                    label: (e.getAttribute('aria-label')||'').slice(0,40),
                    txt: (e.innerText||'').slice(0,20)
                }}))),
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

// ── Idle fallback timer ───────────────────────────────────────────────────────

fn schedule_idle_fallback(app: AppHandle, panel_id: String) {
    // Claude uses Cloudflare Turnstile — auto-promoting to READY is misleading.
    // The JS fireReady() now handles it correctly via __readyCheck.
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
    // Reset state before sending so we can detect fresh output
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
            let _ = app.emit(crate::bridge_server::EVENT_PANEL_GENERATING,
                crate::bridge_server::PanelEventPayload { panel_id: panel_id.clone(), output: None, message: None });

            // Rust polls window.__orchestratorState every second.
            // This bypasses ALL CSP/IPC issues — Rust → WebView eval is always allowed.
            let app2     = app.clone();
            let pid      = panel_id.clone();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);

            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
                    if std::time::Instant::now() > deadline { break; }

                    let win2 = match app2.get_webview_window(&pid) {
                        Some(w) => w,
                        None    => break,
                    };

                    // Eval returns JSON string of current state
                    let snapshot_js = "JSON.stringify(window.__orchestratorState || null)";
                    if win2.eval(snapshot_js).is_err() { break; }

                    // We can't get a return value from eval() in Tauri 2 directly,
                    // so instead we eval a beacon that fires only when done.
                    let poll_js = format!(r#"
(function() {{
    const s = window.__orchestratorState;
    if (!s || s.status !== 'done') return;
    const seq = s.outputSeq || 0;
    const last = window.__lastReportedSeq || 0;
    if (seq <= last) return;
    window.__lastReportedSeq = seq;
    // Beacon back — Rust is listening
    const json  = JSON.stringify({{ type:'output', panel_id:{pid_json}, output: s.output || '' }});
    const CHUNK = 1800;
    const total = Math.ceil(json.length / CHUNK);
    const id    = Date.now() + '-poll';
    for (let i = 0; i < total; i++) {{
        const img = new Image();
        img.src = `http://127.0.0.1:{port}/ping?id=${{id}}&i=${{i}}&t=${{total}}&d=${{encodeURIComponent(json.slice(i*CHUNK,(i+1)*CHUNK))}}`;
    }}
}})();
"#,
                        pid_json = serde_json::to_string(&pid).unwrap(),
                        port     = crate::bridge_server::BRIDGE_PORT,
                    );

                    if win2.eval(&poll_js).is_ok() {
                        // Check if Rust state shows done (beacon fired, bridge_server updated it)
                        let app_state = app2.state::<AppState>();
                        let panels    = app_state.panels.lock().unwrap();
                        if let Some(p) = panels.get(&pid) {
                            if matches!(p.status, PanelStatus::Idle) {
                                break; // output received and processed
                            }
                        }
                    }
                }
            });

            CmdResult::success(())
        }
        Err(e) => CmdResult::fail(format!("eval failed: {e}")),
    }
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
            state.store_output(&panel_id, out.clone());
            let _ = app.emit(EVENT_PANEL_OUTPUT, PanelEventPayload { panel_id, output: Some(out), message: None });
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

/// Manually trigger output capture + DOM diagnostic in a panel.
/// Results come back via image beacon → panel:output event → message log.
#[tauri::command]
pub fn capture_panel_output(app: AppHandle, panel_id: String) -> CmdResult<()> {
    let win = match app.get_webview_window(&panel_id) {
        Some(w) => w,
        None => return CmdResult::fail(format!("Panel {panel_id} not open")),
    };

    let pid = serde_json::to_string(&panel_id).unwrap();

    let js = format!(r#"
(function() {{
    // Use IPC first (works on all panels now that withGlobalTauri=true),
    // fall back to beacon for belt-and-suspenders.
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

    // First try the bridge's own captureOutput
    if (window.__orchestratorBridge?.captureOutput) {{
        window.__orchestratorBridge.captureOutput();
        return;
    }}

    // Bridge not injected — run diagnostic and report as output
    const diag = {{
        bridgeInjected: !!window.__orchestratorBridge,
        url: location.href,
        isSecureContext: window.isSecureContext,
        webkit: !!window.webkit,
        webkitHandlers: !!(window.webkit && window.webkit.messageHandlers),
        speechAPI: !!(window.SpeechRecognition || window.webkitSpeechRecognition),
        textareas: [],
        contenteditables: [],
        customEls: [],
        visibleButtons: [],
    }};

    document.querySelectorAll('textarea').forEach((e, i) => {{
        if (i < 6) diag.textareas.push({{
            ph: (e.placeholder||'').slice(0,80),
            cls: (e.className||'').slice(0,80),
            id: e.id, visible: !!e.offsetParent
        }});
    }});

    document.querySelectorAll('[contenteditable]').forEach((e, i) => {{
        if (i < 6) diag.contenteditables.push({{
            tag: e.tagName,
            label: (e.getAttribute('aria-label')||'').slice(0,80),
            cls: (e.className||'').slice(0,80),
            visible: !!e.offsetParent
        }});
    }});

    ['model-response','message-content','chat-history','rich-textarea'].forEach(tag => {{
        const els = document.querySelectorAll(tag);
        if (els.length) diag.customEls.push({{ tag, count: els.length }});
    }});

    document.querySelectorAll('button').forEach((e, i) => {{
        if (i < 15 && e.offsetParent) diag.visibleButtons.push({{
            label: (e.getAttribute('aria-label')||'').slice(0,50),
            txt: (e.innerText||'').slice(0,30),
        }});
    }});

    sendOutput('DIAGNOSTIC: ' + JSON.stringify(diag, null, 2));
}})();
"#,
        pid  = pid,
        port = BRIDGE_PORT,
    );

    let _ = win.eval(&js);
    CmdResult::success(())
}
