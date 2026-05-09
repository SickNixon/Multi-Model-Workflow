// src-tauri/src/bridge.rs

/// Returns the site-specific JS bridge snippet for the given panel id.
/// Returns an empty string for unknown panel IDs (graceful fallback).
pub fn get_bridge_script(panel_id: &str) -> &'static str {
    match panel_id {
        "gemini"   => GEMINI_BRIDGE,
        "deepseek" => DEEPSEEK_BRIDGE,
        "grok"     => GROK_BRIDGE,
        "claude"   => CLAUDE_BRIDGE,
        _          => "",
    }
}

// ── Gemini (gemini.google.com) ────────────────────────────────────────────────
// COMPLETION STRATEGY (multiple layers, any one of them fires captureOutput):
//   1. Edge trigger: stop button visible → not visible
//   2. Send button re-enables (another done signal)
//   3. DOM quiet for 4s (fallback if stop button never appeared)
//   4. Emergency timer: 25s after send, always fires
const GEMINI_BRIDGE: &str = r#"
(function geminiInit() {
    const INPUT_SELECTORS = [
        'div[contenteditable="true"][aria-label*="Enter"]',
        'rich-textarea div[contenteditable="true"]',
        '.ql-editor[contenteditable="true"]',
        'div[contenteditable="true"]',
    ];
    const SEND_BTN_SELECTORS = [
        'button[aria-label*="Send message"]',
        'button[aria-label*="Send"]',
        'button[data-testid="send-button"]',
        'button[jsname="OCpkoe"]',
        'button[jsaction*="send"]',
    ];
    const STOP_SELECTORS = [
        'button[aria-label*="Stop"]',
        'button[aria-label*="stop"]',
        'button[jsaction*="stop"]',
        'button[data-tooltip*="Stop"]',
    ];

    function tryVisible(selectors) {
        for (const sel of selectors) {
            try { const el = document.querySelector(sel); if (el) return el; } catch(e) {}
        }
        return null;
    }

    window.__orchestratorBridge.sendMessage = function(text) {
        const input = trySelectors(INPUT_SELECTORS);
        if (!input) {
            window.__orchestratorBridge.report('error', { message: 'gemini: input not found' });
            return;
        }
        input.focus();
        document.execCommand('selectAll', false, null);
        document.execCommand('delete', false, null);
        document.execCommand('insertText', false, text);
        setTimeout(() => {
            const btn = trySelectors(SEND_BTN_SELECTORS);
            if (btn) { btn.click(); }
            else {
                input.dispatchEvent(new KeyboardEvent('keydown', {
                    key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true, cancelable: true
                }));
            }
            window.__orchestratorBridge.report('generating', {});
            watchForCompletion();
        }, 200);
    };

    window.__orchestratorBridge.captureOutput = captureOutput;

    function watchForCompletion() {
        let settled       = false;
        let stopWasSeen   = false;
        let lastMutation  = Date.now();
        const startTime   = Date.now();
        const SETTLE_MS   = 4000;

        function done() {
            if (settled) return;
            settled = true;
            clearInterval(poll);
            clearTimeout(emergency);
            observer.disconnect();
            // Small buffer so Gemini can finish rendering the last token
            setTimeout(captureOutput, 600);
        }

        // Emergency: always fire after 25s regardless of detection
        const emergency = setTimeout(() => {
            console.warn('[OrchestratorBridge:gemini] emergency capture at 25s');
            done();
        }, 25000);

        const area = document.querySelector('chat-history')
            || document.querySelector('[class*="conversation"]')
            || document.querySelector('main')
            || document.body;
        const observer = new MutationObserver(() => { lastMutation = Date.now(); });
        observer.observe(area, { childList: true, subtree: true, characterData: true });

        const poll = setInterval(() => {
            // 1. Stop-button edge trigger
            const stopNow = STOP_SELECTORS.some(sel => {
                try { return !!document.querySelector(sel); } catch(e) { return false; }
            });
            if (stopNow) { stopWasSeen = true; }
            if (stopWasSeen && !stopNow) { done(); return; }

            // 2. Send button re-enables (another completion signal)
            const sendBtn = tryVisible(SEND_BTN_SELECTORS);
            if (stopWasSeen && sendBtn && !sendBtn.disabled) { done(); return; }

            // 3. DOM settle fallback (when stop button never appeared)
            if (!stopWasSeen && Date.now() - lastMutation > SETTLE_MS
                    && Date.now() - startTime > 3000) {
                done(); return;
            }
        }, 400);
    }

    function captureOutput() {
        // PRIMARY: last model-response custom element
        const modelResponses = document.querySelectorAll('model-response');
        if (modelResponses.length > 0) {
            const text = modelResponses[modelResponses.length - 1].innerText?.trim();
            if (text && text.length > 10) {
                window.__orchestratorBridge.report('output', { output: text }); return;
            }
        }
        // FALLBACKS
        for (const sel of ['.model-response-text','message-content',
                '[data-message-author-role="model"]','.response-container-content','[class*="markdown"]']) {
            try {
                const els = document.querySelectorAll(sel);
                if (els.length > 0) {
                    const text = els[els.length - 1].innerText?.trim();
                    if (text && text.length > 10) {
                        window.__orchestratorBridge.report('output', { output: text }); return;
                    }
                }
            } catch(e) {}
        }
        const main = document.querySelector('main');
        window.__orchestratorBridge.report('output', {
            output: main ? main.innerText.trim().slice(-4000)
                        : '[Gemini: output capture failed — click CAPTURE]'
        });
    }
})();
"#;

// ── DeepSeek (chat.deepseek.com) ─────────────────────────────────────────────
// STATUS: WORKING — do not touch without confirmed regression
const DEEPSEEK_BRIDGE: &str = r#"
(function deepseekInit() {
    function setReactValue(el, value) {
        try {
            const nativeSetter = Object.getOwnPropertyDescriptor(
                window.HTMLTextAreaElement.prototype, 'value'
            ).set;
            nativeSetter.call(el, value);
            el.dispatchEvent(new Event('input',  { bubbles: true }));
            el.dispatchEvent(new Event('change', { bubbles: true }));
        } catch(e) { el.value = value; el.dispatchEvent(new Event('input', { bubbles: true })); }
    }

    function findInput() {
        return document.querySelector('textarea[placeholder="Message DeepSeek"]')
            || document.querySelector('textarea[placeholder*="Message"]')
            || Array.from(document.querySelectorAll('textarea')).find(t => t.offsetParent);
    }

    function findSendBtn() {
        const btns = Array.from(document.querySelectorAll('button')).filter(b => b.offsetParent);
        return btns.find(b => {
            const lbl = (b.getAttribute('aria-label') || '').toLowerCase();
            return lbl.includes('send') || b.type === 'submit';
        }) || null;
    }

    window.__orchestratorBridge.sendMessage = function(text) {
        const input = findInput();
        if (!input) {
            window.__orchestratorBridge.report('error', { message: 'deepseek: input not found' });
            return;
        }
        setReactValue(input, text);
        setTimeout(() => {
            const btn = findSendBtn();
            if (btn) { btn.click(); }
            else {
                input.dispatchEvent(new KeyboardEvent('keydown', {
                    key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true, cancelable: true
                }));
            }
            window.__orchestratorBridge.report('generating', {});
            watchForCompletion();
        }, 300);
    };

    window.__orchestratorBridge.captureOutput = captureOutput;

    function watchForCompletion() {
        let last = Date.now(), done = false;
        const area = document.querySelector('main') || document.body;
        const obs = new MutationObserver(() => { last = Date.now(); });
        obs.observe(area, { childList: true, subtree: true, characterData: true });
        const poll = setInterval(() => {
            if (Date.now() - last > 3000) {
                clearInterval(poll); obs.disconnect();
                if (!done) { done = true; captureOutput(); }
            }
        }, 400);
    }

    function captureOutput() {
        const els = Array.from(document.querySelectorAll([
            '[class*="ds-markdown"]','[class*="markdown-body"]',
            '[class*="chat-message"]:not([class*="input"])','[class*="message-content"]',
        ].join(','))).filter(e => e.offsetParent && e.innerText?.trim().length > 5);
        let output = els.length ? els[els.length - 1].innerText.trim() : '';
        if (!output) {
            const main = document.querySelector('main');
            output = main ? main.innerText.trim().slice(-3000) : '[deepseek: no output found]';
        }
        window.__orchestratorBridge.report('output', { output });
    }
})();
"#;

// ── Grok (grok.com) ───────────────────────────────────────────────────────────
// Grok may navigate to a new page after sending (grok.com → grok.com/chat/xxx).
// The init script re-runs on each navigation, so watchForCompletion won't carry
// over. For now we focus on reliable INPUT injection and DOM diagnostics.
// Output capture fires on the new page after the DOM settles.
//
// DEBUGGING: when findInput() fails, the bridge dumps the full DOM inventory
// to the message log via the bridge server so we can fix selectors without
// opening DevTools on the WebView.
const GROK_BRIDGE: &str = r#"
(function grokInit() {

    function isVisible(el) {
        if (!el) return false;
        try {
            const rect = el.getBoundingClientRect();
            return rect.width > 10 && rect.height > 10;
        } catch(e) { return false; }
    }

    function findInput() {
        // Try every reasonable selector before giving up
        const CANDIDATES = [
            'textarea[aria-label*="Message"]',
            'textarea[aria-label*="Ask"]',
            'textarea[placeholder*="Ask"]',
            'textarea[placeholder*="Message"]',
            'textarea[placeholder*="Grok"]',
            'textarea[placeholder*="anything"]',
            'div[contenteditable="true"][aria-label*="Message"]',
            'div[contenteditable="true"][aria-label*="Ask"]',
            'div[contenteditable="true"]',
        ];
        for (const sel of CANDIDATES) {
            try {
                const el = document.querySelector(sel);
                if (el && isVisible(el)) {
                    console.log('[OrchestratorBridge:grok] found input via:', sel);
                    return el;
                }
            } catch(e) {}
        }
        // Last resort: first visible textarea anywhere on page
        const allTA = Array.from(document.querySelectorAll('textarea'));
        const vis = allTA.find(isVisible);
        if (vis) {
            console.log('[OrchestratorBridge:grok] found textarea via brute-force:', vis.placeholder, vis.className?.slice(0,60));
            return vis;
        }
        return null;
    }

    function dumpDomDiagnostic(context) {
        // Sends a DOM dump via beacon so we can see it in the message log
        const diag = {
            context,
            url: location.href,
            textareas: Array.from(document.querySelectorAll('textarea')).slice(0, 8).map(e => ({
                ph: (e.placeholder || '').slice(0, 80),
                cls: (e.className || '').slice(0, 80),
                id: e.id,
                visible: isVisible(e),
                rect: (() => { try { const r = e.getBoundingClientRect(); return `${Math.round(r.width)}x${Math.round(r.height)}`; } catch(ex) { return '?'; } })(),
            })),
            contenteditables: Array.from(document.querySelectorAll('[contenteditable]')).slice(0, 6).map(e => ({
                tag: e.tagName,
                label: (e.getAttribute('aria-label') || '').slice(0, 80),
                cls: (e.className || '').slice(0, 80),
                visible: isVisible(e),
            })),
            buttons: Array.from(document.querySelectorAll('button')).filter(isVisible).slice(0, 10).map(e => ({
                label: (e.getAttribute('aria-label') || '').slice(0, 60),
                txt: (e.innerText || '').slice(0, 30).trim(),
                type: e.type,
            })),
        };
        window.__orchestratorBridge.report('output', {
            output: '[GROK DOM DIAGNOSTIC]\n' + JSON.stringify(diag, null, 2)
        });
    }

    function setInputValue(el, text) {
        el.focus();
        el.click();
        if (el.tagName === 'TEXTAREA' || el.tagName === 'INPUT') {
            try {
                const nativeSetter = Object.getOwnPropertyDescriptor(
                    window.HTMLTextAreaElement.prototype, 'value'
                ).set;
                nativeSetter.call(el, text);
            } catch(e) { el.value = text; }
            ['input', 'change'].forEach(evt =>
                el.dispatchEvent(new Event(evt, { bubbles: true, cancelable: true }))
            );
        } else {
            el.focus();
            document.execCommand('selectAll', false, null);
            document.execCommand('delete', false, null);
            document.execCommand('insertText', false, text);
        }
    }

    function submit(el) {
        // 1. Submit button inside the same form
        const form = el.closest('form');
        if (form) {
            const fb = form.querySelector('button[type="submit"]')
                    || Array.from(form.querySelectorAll('button')).find(isVisible);
            if (fb) { console.log('[OrchestratorBridge:grok] submit via form button'); fb.click(); return; }
        }
        // 2. Nearest ancestor button search (walk up 4 levels)
        let parent = el.parentElement;
        for (let i = 0; i < 4; i++) {
            if (!parent) break;
            const btns = Array.from(parent.querySelectorAll('button')).filter(isVisible);
            const sb = btns.find(b => {
                const lbl = (b.getAttribute('aria-label') || '').toLowerCase();
                const txt = (b.textContent || '').toLowerCase().trim();
                return lbl.includes('send') || txt === 'send' || b.type === 'submit';
            });
            if (sb) { console.log('[OrchestratorBridge:grok] submit via ancestor button'); sb.click(); return; }
            parent = parent.parentElement;
        }
        // 3. Global scan
        const global = Array.from(document.querySelectorAll('button')).filter(isVisible).find(b => {
            const lbl = (b.getAttribute('aria-label') || '').toLowerCase();
            return lbl.includes('send') || b.type === 'submit';
        });
        if (global) { console.log('[OrchestratorBridge:grok] submit via global scan'); global.click(); return; }
        // 4. Enter key
        console.log('[OrchestratorBridge:grok] submit via Enter key');
        ['keydown', 'keypress', 'keyup'].forEach(evt =>
            el.dispatchEvent(new KeyboardEvent(evt, {
                key: 'Enter', code: 'Enter', keyCode: 13,
                which: 13, bubbles: true, cancelable: true
            }))
        );
    }

    window.__orchestratorBridge.sendMessage = function(text) {
        let attempts = 0;
        const MAX = 8;

        function tryIt() {
            attempts++;
            const input = findInput();
            if (!input) {
                console.warn('[OrchestratorBridge:grok] no visible input, attempt', attempts);
                if (attempts === 3) {
                    // After 3 fails, dump DOM so we can diagnose
                    dumpDomDiagnostic('sendMessage attempt ' + attempts + ' - no input found');
                }
                if (attempts < MAX) { setTimeout(tryIt, 1000); }
                else { window.__orchestratorBridge.report('error', { message: 'grok: no input after 8 attempts' }); }
                return;
            }
            setInputValue(input, text);
            setTimeout(() => {
                submit(input);
                window.__orchestratorBridge.report('generating', {});
                watchForCompletion();
            }, 500);
        }
        tryIt();
    };

    window.__orchestratorBridge.captureOutput = captureOutput;

    function watchForCompletion() {
        let last = Date.now(), done = false;
        const SETTLE_MS = 3500;
        const MAX_WAIT  = 90000;
        const startTime = Date.now();
        const area = document.querySelector('main') || document.body;
        const obs = new MutationObserver(() => { last = Date.now(); });
        obs.observe(area, { childList: true, subtree: true, characterData: true });
        const poll = setInterval(() => {
            if (Date.now() - last > SETTLE_MS || Date.now() - startTime > MAX_WAIT) {
                clearInterval(poll); obs.disconnect();
                if (!done) { done = true; captureOutput(); }
            }
        }, 400);
    }

    function captureOutput() {
        const allBlocks = Array.from(document.querySelectorAll(
            '[class*="message"],[class*="response"],[class*="assistant"],article,[role="article"]'
        )).filter(el => isVisible(el) && (el.innerText?.trim().length || 0) > 10);
        let output = '';
        for (let i = allBlocks.length - 1; i >= 0; i--) {
            const text = allBlocks[i].innerText?.trim();
            if (text && text.length > 10) { output = text; break; }
        }
        if (!output) {
            const main = document.querySelector('main');
            output = main ? main.innerText.trim().slice(-4000) : '[Grok: output capture failed]';
        }
        window.__orchestratorBridge.report('output', { output });
    }
})();
"#;

// ── Claude (claude.ai) ────────────────────────────────────────────────────────
// CLOUDFLARE BYPASS: claude.ai uses Cloudflare Turnstile which detects
// window.webkit.messageHandlers as a bot fingerprint (this object exists in
// WKWebView but not in real browsers). We hide it after a 50ms delay — long
// enough for Tauri's own init script to capture its reference, but before
// Cloudflare runs its challenge. After hiding, we use beacon-only reporting
// so we don't depend on the IPC bridge being accessible via window.webkit.
const CLAUDE_BRIDGE: &str = r#"
(function claudeInit() {
    // ── Cloudflare bypass ─────────────────────────────────────────────────────
    // Tauri's init script runs synchronously at document-start and captures a
    // closure reference to webkit.messageHandlers.tauri. After that reference
    // is captured (< 10ms), we can safely hide window.webkit from Cloudflare's
    // bot detector (which runs after DOM parsing, well after our 50ms timeout).
    setTimeout(() => {
        try {
            Object.defineProperty(window, 'webkit', {
                get: () => undefined, configurable: true
            });
            console.log('[OrchestratorBridge:claude] webkit hidden from Cloudflare');
        } catch(e) {
            console.warn('[OrchestratorBridge:claude] could not hide webkit:', e);
        }
    }, 50);

    // ── Beacon-only report override ───────────────────────────────────────────
    // After hiding webkit, __TAURI_INTERNALS__.invoke may not work reliably.
    // Use the local HTTP beacon exclusively for Claude's output reporting.
    window.__orchestratorBridge.report = function(type, extra) {
        const payload = JSON.stringify({ type, panel_id: PANEL_ID, ...extra });
        const CHUNK = 1800;
        const total = Math.ceil(payload.length / CHUNK);
        const id = Date.now() + '-cl-' + Math.random().toString(36).slice(2);
        for (let i = 0; i < total; i++) {
            const img = new Image();
            img.src = `http://127.0.0.1:${BRIDGE_PORT}/ping?id=${id}&i=${i}&t=${total}&d=${encodeURIComponent(payload.slice(i * CHUNK, (i + 1) * CHUNK))}`;
        }
    };

    // ── Input / submit ────────────────────────────────────────────────────────
    const INPUT_SELECTORS = [
        'div[contenteditable="true"].ProseMirror',
        '[data-testid="composer-input"] div[contenteditable="true"]',
        'div[contenteditable="true"][aria-label*="message"]',
        'div[contenteditable="true"]',
    ];
    const SEND_BTN_SELECTORS = [
        'button[aria-label="Send Message"]',
        'button[aria-label*="Send"]',
        'button[data-testid="send-button"]',
        'button[type="submit"]',
    ];

    window.__orchestratorBridge.sendMessage = function(text) {
        const input = trySelectors(INPUT_SELECTORS);
        if (!input) {
            window.__orchestratorBridge.report('error', { message: 'claude: input not found' });
            return;
        }
        input.focus();
        document.execCommand('selectAll', false, null);
        document.execCommand('delete', false, null);
        document.execCommand('insertText', false, text);
        setTimeout(() => {
            const sendBtn = trySelectors(SEND_BTN_SELECTORS);
            if (sendBtn) { sendBtn.click(); }
            else {
                input.dispatchEvent(new KeyboardEvent('keydown', {
                    key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true, cancelable: true,
                }));
            }
            window.__orchestratorBridge.report('generating', {});
            watchForCompletion();
        }, 150);
    };

    function watchForCompletion() {
        let settled = false;
        let lastMutationTime = Date.now();
        const SETTLE_MS = 3000;

        const responseArea =
            document.querySelector('[data-testid="conversation-turn-list"]') ||
            document.querySelector('main') || document.body;
        const observer = new MutationObserver(() => { lastMutationTime = Date.now(); });
        observer.observe(responseArea, { childList: true, subtree: true, characterData: true });

        const poll = setInterval(() => {
            const stillStreaming = document.querySelector('[data-is-streaming="true"]');
            if (!stillStreaming && Date.now() - lastMutationTime > SETTLE_MS) {
                clearInterval(poll); observer.disconnect();
                if (!settled) { settled = true; extractAndReport(); }
            }
        }, 300);
    }

    function extractAndReport() {
        const doneMessages = document.querySelectorAll('[data-is-streaming="false"]');
        const outputEl = doneMessages.length > 0
            ? doneMessages[doneMessages.length - 1] : null;
        const output = outputEl
            ? outputEl.innerText.trim()
            : '[claude: output selector failed]';
        if (!outputEl) console.error('[OrchestratorBridge:claude] output not found');
        window.__orchestratorBridge.report('output', { output });
    }
})();
"#;
