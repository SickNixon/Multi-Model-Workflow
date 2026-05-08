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
// Input:  div[contenteditable="true"] inside a rich-textarea component
// Submit: click the send button (aria-label contains "Send message")
// Output: last .model-response-text or [data-message-author-role="model"]
//
// VERIFY: Gemini's DOM structure as of mid-2024. Uses a Shadow DOM in places.
// If the input isn't found, open DevTools on gemini.google.com and look for
// the contenteditable div inside the prompt container.
const GEMINI_BRIDGE: &str = r#"
(function geminiInit() {
    // trySelectors() is defined in the outer IIFE wrapper (commands.rs)

    const INPUT_SELECTORS = [
        // VERIFY: these are the most likely candidates as of 2024
        'div[contenteditable="true"][aria-label*="Enter"]',
        'rich-textarea div[contenteditable="true"]',
        '.ql-editor[contenteditable="true"]',
        'div[contenteditable="true"]',    // last-resort fallback
    ];

    const SEND_BTN_SELECTORS = [
        'button[aria-label*="Send message"]',
        'button[aria-label*="Send"]',
        'button.send-button',
        'button[data-testid="send-button"]',
    ];

    const OUTPUT_SELECTORS = [
        '.model-response-text',
        'model-response .response-content',
        '[data-message-author-role="model"]:last-child',
        '.response-container:last-child',
    ];

    // ── sendMessage implementation ──

    window.__orchestratorBridge.sendMessage = function(text) {
        const input = trySelectors(INPUT_SELECTORS);
        if (!input) {
            console.error('[OrchestratorBridge:gemini] input not found. Check INPUT_SELECTORS.');
            window.__orchestratorBridge.report('error', { message: 'input selector failed' });
            return;
        }

        // Set contenteditable content via execCommand (works in WKWebView)
        input.focus();
        document.execCommand('selectAll', false, null);
        document.execCommand('delete', false, null);
        document.execCommand('insertText', false, text);

        // Give React a tick to process the input event
        setTimeout(() => {
            const sendBtn = trySelectors(SEND_BTN_SELECTORS);
            if (sendBtn) {
                sendBtn.click();
            } else {
                // Fallback: simulate Enter key
                input.dispatchEvent(new KeyboardEvent('keydown', {
                    key: 'Enter', code: 'Enter', keyCode: 13,
                    bubbles: true, cancelable: true
                }));
            }
            window.__orchestratorBridge.report('generating', {});
            watchForCompletion();
        }, 150);
    };

    // ── Generation completion detection ──

    function watchForCompletion() {
        let settled = false;
        let lastMutationTime = Date.now();
        const SETTLE_MS = 2500; // ms without DOM changes = done

        // Target: the whole response area
        const responseArea =
            document.querySelector('chat-history') ||
            document.querySelector('.conversation-container') ||
            document.body;

        const observer = new MutationObserver(() => {
            lastMutationTime = Date.now();
        });

        observer.observe(responseArea, {
            childList: true,
            subtree: true,
            characterData: true,
        });

        // Poll until settled
        const poll = setInterval(() => {
            if (Date.now() - lastMutationTime > SETTLE_MS) {
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
        const outputEl = trySelectors(OUTPUT_SELECTORS);
        const output = outputEl
            ? outputEl.innerText.trim()
            : '[OrchestratorBridge:gemini] output selector failed — check OUTPUT_SELECTORS';

        if (!outputEl) {
            console.error('[OrchestratorBridge:gemini] output not found. Check OUTPUT_SELECTORS.');
        }

        window.__orchestratorBridge.report('output', { output });
    }
})();
"#;

// ── DeepSeek (chat.deepseek.com) ─────────────────────────────────────────────
// Input:  textarea (standard, not contenteditable)
// Submit: Enter key or send button
// Note:   DeepSeek has a "thinking" mode — output may have a <think> block.
//         We capture full innerText which includes thinking; strip if needed.
//
// VERIFY: Selectors as of 2024. DeepSeek's input is a straightforward textarea.
const DEEPSEEK_BRIDGE: &str = r#"
(function deepseekInit() {
    const INPUT_SELECTORS = [
        'textarea[placeholder*="Message"]',
        'textarea[placeholder*="message"]',
        '#chat-input',
        'textarea',   // last-resort
    ];

    const SEND_BTN_SELECTORS = [
        'button[aria-label*="Send"]',
        'button.send-button',
        'div[role="button"][aria-label*="send"]',
    ];

    const OUTPUT_SELECTORS = [
        // The last assistant message content div
        '.ds-markdown:last-of-type',
        '.message-content.assistant:last-child',
        '[class*="assistant"]:last-child [class*="content"]',
        '[data-role="assistant"]:last-child',
    ];

    // React-controlled textarea value setter trick
    // Standard element.value = ... doesn't trigger React's onChange
    function setReactTextareaValue(el, value) {
        const nativeSetter = Object.getOwnPropertyDescriptor(
            window.HTMLTextAreaElement.prototype, 'value'
        ).set;
        nativeSetter.call(el, value);
        el.dispatchEvent(new Event('input', { bubbles: true }));
    }

    window.__orchestratorBridge.sendMessage = function(text) {
        const input = trySelectors(INPUT_SELECTORS);
        if (!input) {
            console.error('[OrchestratorBridge:deepseek] input not found. Check INPUT_SELECTORS.');
            window.__orchestratorBridge.report('error', { message: 'input selector failed' });
            return;
        }

        input.focus();
        setReactTextareaValue(input, text);

        setTimeout(() => {
            const sendBtn = trySelectors(SEND_BTN_SELECTORS);
            if (sendBtn) {
                sendBtn.click();
            } else {
                input.dispatchEvent(new KeyboardEvent('keydown', {
                    key: 'Enter', code: 'Enter', keyCode: 13,
                    bubbles: true, cancelable: true
                }));
            }
            window.__orchestratorBridge.report('generating', {});
            watchForCompletion();
        }, 150);
    };

    function watchForCompletion() {
        let settled = false;
        let lastMutationTime = Date.now();
        const SETTLE_MS = 2500;

        const responseArea =
            document.querySelector('.chat-content') ||
            document.querySelector('main') ||
            document.body;

        const observer = new MutationObserver(() => {
            lastMutationTime = Date.now();
        });

        observer.observe(responseArea, {
            childList: true, subtree: true, characterData: true,
        });

        const poll = setInterval(() => {
            if (Date.now() - lastMutationTime > SETTLE_MS) {
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
        // Get all assistant messages and pick the last
        const allOutputs = document.querySelectorAll(
            '.ds-markdown, [class*="assistant"] [class*="content"], [data-role="assistant"]'
        );
        const outputEl = allOutputs.length > 0 ? allOutputs[allOutputs.length - 1] : null;
        const output = outputEl
            ? outputEl.innerText.trim()
            : '[OrchestratorBridge:deepseek] output selector failed — check OUTPUT_SELECTORS';

        if (!outputEl) {
            console.error('[OrchestratorBridge:deepseek] output not found.');
        }

        window.__orchestratorBridge.report('output', { output });
    }
})();
"#;

// ── Grok (grok.com) ───────────────────────────────────────────────────────────
// Input:  textarea — Grok uses a standard textarea
// Submit: click the send button
// VERIFY: Open grok.com in DevTools to confirm selectors
const GROK_BRIDGE: &str = r#"
(function grokInit() {
    const INPUT_SELECTORS = [
        'textarea[data-testid="grok-compose-input"]',
        'textarea[placeholder*="Ask"]',
        'textarea[placeholder*="Grok"]',
        'textarea[placeholder*="anything"]',
        'textarea[class*="compose"]',
        'textarea',   // last resort
    ];

    const SEND_BTN_SELECTORS = [
        'button[data-testid="grok-compose-send"]',
        'button[aria-label*="Send"]',
        'button[type="submit"]',
        'button[class*="send"]',
        'button[class*="Send"]',
    ];

    function setReactTextareaValue(el, value) {
        const nativeSetter = Object.getOwnPropertyDescriptor(
            window.HTMLTextAreaElement.prototype, 'value'
        ).set;
        nativeSetter.call(el, value);
        el.dispatchEvent(new Event('input',  { bubbles: true }));
        el.dispatchEvent(new Event('change', { bubbles: true }));
    }

    window.__orchestratorBridge.sendMessage = function(text) {
        const input = trySelectors(INPUT_SELECTORS);
        if (!input) {
            console.error('[OrchestratorBridge:grok] input not found — open DevTools on grok.com to find the correct selector');
            window.__orchestratorBridge.report('error', { message: 'grok input selector failed' });
            return;
        }

        input.focus();
        setReactTextareaValue(input, text);

        setTimeout(() => {
            const sendBtn = trySelectors(SEND_BTN_SELECTORS);
            if (sendBtn) {
                sendBtn.click();
            } else {
                // Fallback: Enter key
                input.dispatchEvent(new KeyboardEvent('keydown', {
                    key: 'Enter', code: 'Enter', keyCode: 13,
                    bubbles: true, cancelable: true, shiftKey: false,
                }));
            }
            window.__orchestratorBridge.report('generating', {});
            watchForCompletion();
        }, 300);
    };

    function watchForCompletion() {
        let settled = false;
        let lastMutation = Date.now();
        const SETTLE_MS = 3000;

        const area = document.querySelector('main') || document.body;
        const observer = new MutationObserver(() => { lastMutation = Date.now(); });
        observer.observe(area, { childList: true, subtree: true, characterData: true });

        const poll = setInterval(() => {
            if (Date.now() - lastMutation > SETTLE_MS) {
                clearInterval(poll);
                observer.disconnect();
                if (!settled) { settled = true; extractAndReport(); }
            }
        }, 300);
    }

    function extractAndReport() {
        // Try multiple output selectors — Grok's DOM changes frequently
        const candidates = document.querySelectorAll([
            '[data-testid*="response"]',
            '[class*="message"]:not([class*="user"])',
            '[class*="assistant"]',
            '[class*="response"]',
            'article',
        ].join(','));

        const outputEl = candidates.length > 0 ? candidates[candidates.length - 1] : null;
        const output = outputEl
            ? outputEl.innerText.trim()
            : '[OrchestratorBridge:grok] output selector failed — check DevTools on grok.com';

        if (!outputEl) console.error('[OrchestratorBridge:grok] could not find output element');
        window.__orchestratorBridge.report('output', { output });
    }
})();
"#;

// ── Claude (claude.ai) ────────────────────────────────────────────────────────
// Input:  div[contenteditable="true"] — ProseMirror-based rich editor
// Submit: click send button or Enter
// Output: last [data-is-streaming="false"] div in the conversation
//
// This one we know well. The ProseMirror editor requires execCommand to set text.
// VERIFY: the send button aria-label after any Claude UI update.
const CLAUDE_BRIDGE: &str = r#"
(function claudeInit() {
    const INPUT_SELECTORS = [
        // ProseMirror contenteditable inside the prompt composer
        'div[contenteditable="true"].ProseMirror',
        '[data-testid="composer-input"] div[contenteditable="true"]',
        'div[contenteditable="true"][aria-label*="message"]',
        'div[contenteditable="true"]',    // last-resort
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

        // ProseMirror contenteditable: use execCommand
        input.focus();
        // Clear existing content
        document.execCommand('selectAll', false, null);
        document.execCommand('delete', false, null);
        // Insert text
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
        // Claude streams fast but may pause — give it a solid window
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
            // Also check if streaming attribute is gone as a secondary signal
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
        // Claude marks done messages with data-is-streaming="false"
        // Get the last one
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
