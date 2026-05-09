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

// ── Shared helpers (inlined into each site script) ────────────────────────────

/// Tries a list of selectors in order, returns the first match or null.
/// Included in the outer IIFE by commands.rs — available to all site scripts.
const SELECTOR_FALLBACK_FN: &str = r#"
    function trySelectors(selectors) {
        for (const sel of selectors) {
            try {
                const el = document.querySelector(sel);
                if (el) return el;
            } catch(e) { /* invalid selector — skip */ }
        }
        return null;
    }
"#;

// ── Gemini (gemini.google.com) ────────────────────────────────────────────────
// Input:  div[contenteditable="true"] inside rich-textarea component
// Submit: click send button (aria-label contains "Send message")
// Output: last model-response custom element
//
// COMPLETION DETECTION: Gemini shows a stop-generation button while streaming.
// We poll for that button to disappear + DOM to quiet for 2.5s.
// This is more reliable than pure MutationObserver settle on a streaming site.
//
// VERIFY: selectors after any Gemini UI update. Open DevTools on gemini.google.com
// and inspect the contenteditable inside the prompt container.
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
        'button.send-button',
        'button[data-testid="send-button"]',
        'button[jsname="OCpkoe"]',
        'button[jsaction*="send"]',
    ];

    // Stop-generation button is shown the entire time Gemini is streaming.
    // When it disappears, generation is done.
    const STOP_SELECTORS = [
        'button[aria-label*="Stop"]',
        'button[aria-label*="stop"]',
        'button[jsaction*="stop"]',
        'button[data-tooltip*="Stop"]',
    ];

    window.__orchestratorBridge.sendMessage = function(text) {
        const input = trySelectors(INPUT_SELECTORS);
        if (!input) {
            window.__orchestratorBridge.report('error', { message: 'gemini input selector failed' });
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
        let lastMutation = Date.now();
        let settled      = false;
        // Require DOM to be quiet for 2.5s AND no stop-button visible.
        const SETTLE_MS  = 2500;
        const MAX_WAIT   = 180000; // 3 min hard ceiling
        const startTime  = Date.now();

        const area = document.querySelector('chat-history')
            || document.querySelector('[class*="conversation"]')
            || document.querySelector('main')
            || document.body;

        const observer = new MutationObserver(() => { lastMutation = Date.now(); });
        observer.observe(area, { childList: true, subtree: true, characterData: true });

        const poll = setInterval(() => {
            if (Date.now() - startTime > MAX_WAIT) {
                clearInterval(poll); observer.disconnect();
                if (!settled) { settled = true; captureOutput(); }
                return;
            }

            const stopVisible = STOP_SELECTORS.some(sel => {
                try { return !!document.querySelector(sel); } catch(e) { return false; }
            });

            const domQuiet   = Date.now() - lastMutation > SETTLE_MS;
            // Wait at least 1.5s before firing to avoid premature capture
            const minElapsed = Date.now() - startTime > 1500;

            if (!stopVisible && domQuiet && minElapsed) {
                clearInterval(poll); observer.disconnect();
                if (!settled) { settled = true; captureOutput(); }
            }
        }, 400);
    }

    function captureOutput() {
        // PRIMARY: model-response is a custom element Gemini uses for every AI turn.
        // Grab the LAST one — that is the most recent response.
        const modelResponses = document.querySelectorAll('model-response');
        if (modelResponses.length > 0) {
            const last = modelResponses[modelResponses.length - 1];
            const text = last.innerText?.trim();
            if (text && text.length > 10) {
                window.__orchestratorBridge.report('output', { output: text });
                return;
            }
        }

        // FALLBACK CHAIN
        const FALLBACKS = [
            '.model-response-text',
            'message-content',
            '[data-message-author-role="model"]',
            '.response-container-content',
            '[class*="markdown"]',
        ];
        for (const sel of FALLBACKS) {
            try {
                const els = document.querySelectorAll(sel);
                if (els.length > 0) {
                    const text = els[els.length - 1].innerText?.trim();
                    if (text && text.length > 10) {
                        window.__orchestratorBridge.report('output', { output: text });
                        return;
                    }
                }
            } catch(e) { /* bad selector — skip */ }
        }

        // NUCLEAR FALLBACK
        const main = document.querySelector('main');
        const fallback = main
            ? main.innerText.trim().slice(-4000)
            : '[Gemini: output capture failed — click CAPTURE to retry]';
        window.__orchestratorBridge.report('output', { output: fallback });
    }
})();
"#;

// ── DeepSeek (chat.deepseek.com) ─────────────────────────────────────────────
// CONFIRMED via diagnostic: textarea placeholder = "Message DeepSeek"
// STATUS: WORKING — do not touch without a confirmed regression
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
            '[class*="ds-markdown"]',
            '[class*="markdown-body"]',
            '[class*="chat-message"]:not([class*="input"])',
            '[class*="message-content"]',
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
// Input: textarea (current grok.com) or div[contenteditable] (fallback)
//
// KEY FIXES vs previous version:
// 1. findInput() now uses getBoundingClientRect() to detect visible elements
//    instead of offsetParent (which can be null even for visible elements
//    in certain stacking contexts / CSS transforms).
// 2. submit() searches the input's form or closest ancestor for submit button
//    before falling back to aria-label search and finally keyboard events.
// 3. Retry loop gives up to 8 seconds for the page to be ready.
const GROK_BRIDGE: &str = r#"
(function grokInit() {

    function isVisible(el) {
        if (!el) return false;
        const rect = el.getBoundingClientRect();
        return rect.width > 10 && rect.height > 10;
    }

    function findInput() {
        // Try labelled selectors first (most stable across UI updates)
        const labelled = [
            'textarea[aria-label*="Message"]',
            'textarea[aria-label*="Ask"]',
            'textarea[placeholder*="Ask"]',
            'textarea[placeholder*="Message"]',
            'textarea[placeholder*="Grok"]',
            'div[contenteditable="true"][aria-label*="Message"]',
            'div[contenteditable="true"][aria-label*="Ask"]',
        ];
        for (const sel of labelled) {
            try {
                const el = document.querySelector(sel);
                if (el && isVisible(el)) return el;
            } catch(e) {}
        }

        // Fallback: first visible textarea
        const textareas = Array.from(document.querySelectorAll('textarea'));
        const visTA = textareas.find(isVisible);
        if (visTA) return visTA;

        // Fallback: first visible contenteditable
        const divs = Array.from(document.querySelectorAll('div[contenteditable="true"]'));
        return divs.find(isVisible) || null;
    }

    function setInputValue(el, text) {
        el.focus();
        el.click();

        if (el.tagName === 'TEXTAREA' || el.tagName === 'INPUT') {
            // React controlled input: must use native setter to trigger synthetic events
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
            // contenteditable (React ProseMirror / Lexical style)
            el.focus();
            document.execCommand('selectAll', false, null);
            document.execCommand('delete', false, null);
            document.execCommand('insertText', false, text);
        }
    }

    function submit(el) {
        // 1. Look for submit button in the same form
        const form = el.closest('form');
        if (form) {
            const formBtn = form.querySelector('button[type="submit"], button:not([disabled])');
            if (formBtn && isVisible(formBtn)) { formBtn.click(); return; }
        }

        // 2. Look for a button in the nearest ancestor that contains the input
        const parent = el.parentElement?.parentElement?.parentElement;
        if (parent) {
            const btns = Array.from(parent.querySelectorAll('button')).filter(isVisible);
            const sendBtn = btns.find(b => {
                const label = (b.getAttribute('aria-label') || '').toLowerCase();
                const txt   = (b.textContent || '').toLowerCase().trim();
                return label.includes('send') || txt === 'send' || b.type === 'submit';
            });
            if (sendBtn) { sendBtn.click(); return; }
        }

        // 3. Global aria-label search
        const allBtns = Array.from(document.querySelectorAll('button')).filter(isVisible);
        const labelBtn = allBtns.find(b => {
            const label = (b.getAttribute('aria-label') || '').toLowerCase();
            return label.includes('send') || b.type === 'submit';
        });
        if (labelBtn) { labelBtn.click(); return; }

        // 4. Enter key as last resort
        ['keydown', 'keypress', 'keyup'].forEach(evt =>
            el.dispatchEvent(new KeyboardEvent(evt, {
                key: 'Enter', code: 'Enter', keyCode: 13,
                which: 13, bubbles: true, cancelable: true
            }))
        );
    }

    window.__orchestratorBridge.sendMessage = function(text) {
        let attempts = 0;
        const MAX_ATTEMPTS = 8;

        function tryIt() {
            attempts++;
            const input = findInput();
            if (!input) {
                if (attempts < MAX_ATTEMPTS) {
                    console.warn('[OrchestratorBridge:grok] no visible input, retry', attempts);
                    setTimeout(tryIt, 1000);
                } else {
                    window.__orchestratorBridge.report('error', { message: 'grok: no input after 8 attempts' });
                }
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
        let lastMutation = Date.now();
        let settled = false;
        const SETTLE_MS = 3500;
        const MAX_WAIT  = 90000;
        const startTime = Date.now();

        const area = document.querySelector('main') || document.body;
        const observer = new MutationObserver(() => { lastMutation = Date.now(); });
        observer.observe(area, { childList: true, subtree: true, characterData: true });

        const poll = setInterval(() => {
            const elapsed = Date.now() - startTime;
            if (Date.now() - lastMutation > SETTLE_MS || elapsed > MAX_WAIT) {
                clearInterval(poll); observer.disconnect();
                if (!settled) { settled = true; captureOutput(); }
            }
        }, 400);
    }

    function captureOutput() {
        // Grok uses generic class names — grab last substantial text block
        const allBlocks = Array.from(document.querySelectorAll(
            '[class*="message"], [class*="response"], [class*="assistant"], article, [role="article"]'
        )).filter(el => isVisible(el) && (el.innerText?.trim().length || 0) > 10);

        let output = '';
        for (let i = allBlocks.length - 1; i >= 0; i--) {
            const text = allBlocks[i].innerText?.trim();
            if (text && text.length > 10) { output = text; break; }
        }
        if (!output) {
            const main = document.querySelector('main');
            if (main) output = main.innerText.trim().slice(-4000);
        }
        window.__orchestratorBridge.report('output', {
            output: output || '[Grok: output capture failed — click CAPTURE]'
        });
    }
})();
"#;

// ── Claude (claude.ai) ────────────────────────────────────────────────────────
// Input:  div[contenteditable="true"] — ProseMirror-based rich editor
// Submit: click send button or Enter
// Output: last [data-is-streaming="false"] div in the conversation
//
// STATUS: May hit Cloudflare Turnstile on first load. Complete it once;
// session is persisted in the data directory and it won't ask again.
//
// VERIFY: send button aria-label after any Claude UI update.
const CLAUDE_BRIDGE: &str = r#"
(function claudeInit() {
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
            console.error('[OrchestratorBridge:claude] input not found. Check INPUT_SELECTORS.');
            window.__orchestratorBridge.report('error', { message: 'input selector failed' });
            return;
        }

        input.focus();
        document.execCommand('selectAll', false, null);
        document.execCommand('delete', false, null);
        document.execCommand('insertText', false, text);

        setTimeout(() => {
            const sendBtn = trySelectors(SEND_BTN_SELECTORS);
            if (sendBtn) {
                sendBtn.click();
            } else {
                input.dispatchEvent(new KeyboardEvent('keydown', {
                    key: 'Enter', code: 'Enter', keyCode: 13,
                    bubbles: true, cancelable: true,
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
            document.querySelector('main') ||
            document.body;

        const observer = new MutationObserver(() => {
            lastMutationTime = Date.now();
        });
        observer.observe(responseArea, {
            childList: true, subtree: true, characterData: true,
        });

        const poll = setInterval(() => {
            const stillStreaming = document.querySelector('[data-is-streaming="true"]');
            if (!stillStreaming && Date.now() - lastMutationTime > SETTLE_MS) {
                clearInterval(poll);
                observer.disconnect();
                if (!settled) {
                    settled = true;
                    extractAndReport();
                }
            }
        }, 300);
    }

    function extractAndReport() {
        const doneMessages = document.querySelectorAll('[data-is-streaming="false"]');
        const outputEl = doneMessages.length > 0
            ? doneMessages[doneMessages.length - 1]
            : null;

        const output = outputEl
            ? outputEl.innerText.trim()
            : '[OrchestratorBridge:claude] output selector failed — check OUTPUT_SELECTORS';

        if (!outputEl) {
            console.error('[OrchestratorBridge:claude] output not found.');
        }

        window.__orchestratorBridge.report('output', { output });
    }
})();
"#;
