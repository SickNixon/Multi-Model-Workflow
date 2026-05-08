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
        let settled = false;
        const SETTLE_MS = 3500;

        const area = document.querySelector('chat-history')
            || document.querySelector('[class*="conversation"]')
            || document.querySelector('main')
            || document.body;

        const observer = new MutationObserver(() => { lastMutation = Date.now(); });
        observer.observe(area, { childList: true, subtree: true, characterData: true });

        const poll = setInterval(() => {
            if (Date.now() - lastMutation > SETTLE_MS) {
                clearInterval(poll);
                observer.disconnect();
                if (!settled) { settled = true; captureOutput(); }
            }
        }, 400);
    }

    function captureOutput() {
        // Try progressively broader selectors
        const candidates = [
            // Current Gemini DOM (2024-2025)
            ...Array.from(document.querySelectorAll('model-response')),
            ...Array.from(document.querySelectorAll('.model-response-text')),
            ...Array.from(document.querySelectorAll('[data-message-author-role="model"]')),
            ...Array.from(document.querySelectorAll('message-content')),
            ...Array.from(document.querySelectorAll('.response-container')),
            ...Array.from(document.querySelectorAll('[class*="response"][class*="content"]')),
            // Nuclear fallback: grab all paragraphs inside main
            ...Array.from(document.querySelectorAll('main p')),
        ];

        // Pick the last non-empty one
        let output = '';
        for (let i = candidates.length - 1; i >= 0; i--) {
            const text = candidates[i].innerText?.trim();
            if (text && text.length > 10) { output = text; break; }
        }

        if (!output) {
            // Last resort: grab all visible text from main excluding input area
            const main = document.querySelector('main');
            if (main) output = main.innerText.trim().slice(-3000);
        }

        window.__orchestratorBridge.report('output', { output: output || '[Gemini: output capture failed — click CAPTURE to retry]' });
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
const GROK_BRIDGE: &str = r#"
(function grokInit() {
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
        // Try textarea first (most likely)
        const textareas = document.querySelectorAll('textarea');
        for (const t of textareas) {
            if (t.offsetParent !== null) return t; // visible textarea
        }
        // Fallback: contenteditable
        const editables = document.querySelectorAll('div[contenteditable="true"]');
        for (const e of editables) {
            if (e.offsetParent !== null) return e;
        }
        return null;
    }

    function findSendButton() {
        const btns = document.querySelectorAll('button');
        for (const btn of btns) {
            const label = (btn.getAttribute('aria-label') || '').toLowerCase();
            const type  = btn.getAttribute('type') || '';
            if (label.includes('send') || type === 'submit') return btn;
        }
        // Try SVG buttons (Grok uses icon buttons)
        for (const btn of btns) {
            if (btn.querySelector('svg') && btn.offsetParent !== null) {
                // Last visible button with SVG is usually send
                // Return the last one as fallback
                const allVisible = Array.from(btns).filter(b => b.querySelector('svg') && b.offsetParent);
                if (allVisible.length > 0) return allVisible[allVisible.length - 1];
            }
        }
        return null;
    }

    window.__orchestratorBridge.sendMessage = function(text) {
        const input = findInput();
        if (!input) {
            console.error('[OrchestratorBridge:grok] no input found');
            window.__orchestratorBridge.report('error', { message: 'grok input not found' });
            return;
        }

        if (input.tagName === 'TEXTAREA') {
            setReactValue(input, text);
        } else {
            input.focus();
            document.execCommand('selectAll', false, null);
            document.execCommand('insertText', false, text);
        }

        setTimeout(() => {
            const btn = findSendButton();
            if (btn) {
                btn.click();
            } else {
                input.dispatchEvent(new KeyboardEvent('keydown', {
                    key: 'Enter', code: 'Enter', keyCode: 13,
                    bubbles: true, cancelable: true, shiftKey: false,
                }));
            }
            window.__orchestratorBridge.report('generating', {});
            watchForCompletion();
        }, 400);
    };

    window.__orchestratorBridge.captureOutput = captureOutput;

    function watchForCompletion() {
        let lastMutation = Date.now();
        let settled = false;
        const SETTLE_MS = 3500;

        const area = document.querySelector('main') || document.body;
        const observer = new MutationObserver(() => { lastMutation = Date.now(); });
        observer.observe(area, { childList: true, subtree: true, characterData: true });

        const poll = setInterval(() => {
            if (Date.now() - lastMutation > SETTLE_MS) {
                clearInterval(poll);
                observer.disconnect();
                if (!settled) { settled = true; captureOutput(); }
            }
        }, 400);
    }

    function captureOutput() {
        // Grab all visible text blocks, pick the last meaningful one
        const allBlocks = Array.from(document.querySelectorAll(
            '[class*="message"], [class*="response"], [class*="assistant"], article, [role="article"]'
        )).filter(el => el.offsetParent !== null);

        let output = '';
        for (let i = allBlocks.length - 1; i >= 0; i--) {
            const text = allBlocks[i].innerText?.trim();
            if (text && text.length > 10) { output = text; break; }
        }

        if (!output) {
            const main = document.querySelector('main');
            if (main) output = main.innerText.trim().slice(-3000);
        }

        window.__orchestratorBridge.report('output', { output: output || '[Grok: output capture failed]' });
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
