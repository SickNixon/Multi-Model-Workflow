// src/components/OrchestratorPanel.tsx
import { useRef, useEffect, KeyboardEvent, useState, useCallback } from 'react';
import { useStore } from '../store';
import { type PanelId, type RoutingMode, ALL_PANEL_IDS, PANEL_LABELS, PANEL_COLORS } from '../types';

// ── Speech recognition types ──────────────────────────────────────────────────
declare global {
  interface Window {
    SpeechRecognition: typeof SpeechRecognition;
    webkitSpeechRecognition: typeof SpeechRecognition;
  }
}

// ── Speech-to-text hook ───────────────────────────────────────────────────────
function useSpeechToText(onTranscript: (text: string) => void) {
  const [listening, setListening]   = useState(false);
  const [supported, setSupported]   = useState(false);
  const recogRef = useRef<SpeechRecognition | null>(null);

  useEffect(() => {
    const SR = window.SpeechRecognition || window.webkitSpeechRecognition;
    setSupported(!!SR);
    if (!SR) return;

    const r = new SR();
    r.continuous      = true;
    r.interimResults  = true;
    r.lang            = 'en-US';

    let finalAccum = '';

    r.onresult = (e) => {
      let interim = '';
      for (let i = e.resultIndex; i < e.results.length; i++) {
        const t = e.results[i][0].transcript;
        if (e.results[i].isFinal) { finalAccum += t + ' '; }
        else                      { interim = t; }
      }
      // Send final text to store as it accumulates
      if (finalAccum) {
        onTranscript(finalAccum.trim());
        finalAccum = '';
      }
    };

    r.onerror = (e) => {
      console.error('[STT]', e.error);
      setListening(false);
    };
    r.onend = () => setListening(false);

    recogRef.current = r;
    return () => { r.abort(); };
  }, [onTranscript]);

  const toggle = useCallback(() => {
    const r = recogRef.current;
    if (!r) return;
    if (listening) {
      r.stop();
      setListening(false);
    } else {
      try {
        r.start();
        setListening(true);
      } catch(e) {
        console.error('[STT] start failed:', e);
      }
    }
  }, [listening]);

  return { listening, supported, toggle };
}

// ── Routing selector ──────────────────────────────────────────────────────────
function RoutingSelector() {
  const routing    = useStore(s => s.routing);
  const setRouting = useStore(s => s.setRouting);
  const panels     = useStore(s => s.panels);

  const modes: { id: RoutingMode; label: string }[] = [
    { id: 'broadcast',  label: 'BROADCAST' },
    { id: 'sequential', label: 'SEQUENCE' },
    { id: 'single',     label: 'SINGLE' },
  ];

  const toggleTarget = (id: PanelId) => {
    const next = routing.targets.includes(id)
      ? routing.targets.filter(t => t !== id)
      : [...routing.targets, id];
    if (next.length > 0) setRouting({ targets: next });
  };

  return (
    <div style={styles.routingBox}>
      <div style={{ display: 'flex', gap: 6 }}>
        {modes.map(m => (
          <button key={m.id}
            className={routing.mode === m.id ? 'btn-primary' : 'btn-ghost'}
            style={{ fontSize: 10, padding: '4px 10px' }}
            onClick={() => setRouting({ mode: m.id })}>
            {m.label}
          </button>
        ))}
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' as const }}>
        <span style={{ fontSize: 9, color: 'var(--text-dim)', letterSpacing: '0.1em', marginRight: 4 }}>TO:</span>
        {ALL_PANEL_IDS.map(id => {
          const isOpen   = panels[id]?.status?.status !== 'closed';
          const isTarget = routing.targets.includes(id);
          const color    = PANEL_COLORS[id];
          return (
            <button key={id} onClick={() => toggleTarget(id)} disabled={!isOpen}
              style={{ padding: '3px 9px', fontSize: 10, border: '1px solid', borderRadius: 3,
                letterSpacing: '0.06em', cursor: 'pointer', fontFamily: 'var(--font-mono)',
                borderColor: isTarget ? color : 'var(--border)',
                color:       isTarget ? color : 'var(--text-dim)',
                background:  isTarget ? `${color}15` : 'transparent',
                opacity:     isOpen ? 1 : 0.35, transition: 'all 120ms ease' }}>
              {PANEL_LABELS[id].slice(0, 3).toUpperCase()}
            </button>
          );
        })}
      </div>
    </div>
  );
}

// ── Message log ───────────────────────────────────────────────────────────────
function MessageLog() {
  const history      = useStore(s => s.history);
  const clearHistory = useStore(s => s.clearHistory);
  const scrollRef    = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
  }, [history.length]);

  if (history.length === 0) {
    return <div style={styles.emptyLog}><span className="dim">// no messages yet — open panels and fire away</span></div>;
  }

  return (
    <div style={styles.logOuter}>
      <div style={styles.logHeader}>
        <span className="dim" style={{ fontSize: 10 }}>MESSAGE LOG</span>
        <button className="btn-ghost" style={{ fontSize: 9, padding: '2px 8px' }} onClick={clearHistory}>CLEAR</button>
      </div>
      <div ref={scrollRef} style={styles.logScroll}>
        {history.map(entry => {
          const isUser    = entry.from === 'user';
          const isRouting = typeof entry.content === 'string' && entry.content.startsWith('→');
          const fromColor = isUser ? 'var(--accent)' : PANEL_COLORS[entry.from as PanelId] ?? 'var(--text-secondary)';
          const fromLabel = isUser ? 'YOU' : (entry.from as string).toUpperCase();
          return (
            <div key={entry.id} style={{ ...styles.logEntry, borderLeftColor: fromColor, opacity: isRouting ? 0.5 : 1 }}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <span style={{ color: fromColor, fontSize: 10, fontWeight: 700 }}>{fromLabel}</span>
                <span className="dim" style={{ fontSize: 9 }}>{new Date(entry.timestamp).toLocaleTimeString()}</span>
              </div>
              {!isRouting && (
                <div style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: isUser ? 'var(--text-primary)' : 'var(--text-secondary)', lineHeight: 1.6, userSelect: 'text', whiteSpace: 'pre-wrap', wordBreak: 'break-word', maxHeight: 200, overflowY: 'auto' }}>
                  {entry.content}
                </div>
              )}
              {isRouting && <div style={{ fontSize: 10, color: 'var(--text-dim)', fontStyle: 'italic' }}>{entry.content}</div>}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ── Prompt composer with STT ──────────────────────────────────────────────────
function PromptComposer() {
  const draft             = useStore(s => s.promptDraft);
  const setDraft          = useStore(s => s.setPromptDraft);
  const appendDraft       = useStore(s => s.appendPromptDraft);
  const sendPrompt        = useStore(s => s.sendPrompt);
  const panels            = useStore(s => s.panels);

  const hasOpenPanels  = ALL_PANEL_IDS.some(id => panels[id]?.status?.status !== 'closed' && panels[id]?.status?.status !== undefined);
  const hasGenerating  = ALL_PANEL_IDS.some(id => panels[id]?.status?.status === 'generating');

  const handleTranscript = useCallback((text: string) => {
    appendDraft(text);
  }, [appendDraft]);

  const { listening, supported, toggle } = useSpeechToText(handleTranscript);

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      if (draft.trim() && hasOpenPanels && !hasGenerating) void sendPrompt(draft);
    }
  };

  const canSend = draft.trim() && hasOpenPanels && !hasGenerating;

  return (
    <div style={styles.composerWrap}>
      <textarea
        value={draft}
        onChange={e => setDraft(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder={hasOpenPanels ? 'Type or speak your prompt… Enter to send' : 'Opening panels…'}
        disabled={!hasOpenPanels || hasGenerating}
        style={{ ...styles.textarea, opacity: hasOpenPanels ? 1 : 0.5 }}
      />
      <div style={styles.composerBtns}>
        {supported && (
          <button
            onClick={toggle}
            title={listening ? 'Stop listening' : 'Start voice input'}
            style={{
              ...styles.micBtn,
              background:   listening ? 'var(--amber)' : 'var(--bg-raised)',
              color:        listening ? 'var(--bg-void)' : 'var(--text-secondary)',
              borderColor:  listening ? 'var(--amber)' : 'var(--border)',
              animationName: listening ? 'pulse-amber' : 'none',
              animationDuration: '1s',
              animationIterationCount: 'infinite',
            }}>
            {listening ? '⏹' : '🎙️'}
          </button>
        )}
        <button
          className="btn-primary"
          disabled={!canSend}
          onClick={() => void sendPrompt(draft)}
          style={{ ...styles.sendBtn, opacity: canSend ? 1 : 0.4 }}>
          {hasGenerating ? '▌' : 'SEND ↵'}
        </button>
      </div>
    </div>
  );
}

// ── Root ──────────────────────────────────────────────────────────────────────
export function OrchestratorPanel() {
  return (
    <div style={styles.root}>
      <div style={styles.header}>
        <span className="display" style={{ fontSize: 22, color: 'var(--accent)', letterSpacing: '0.08em' }}>VIBE ORCHESTRATOR</span>
        <span className="dim" style={{ fontSize: 10 }}>BRIDGE :7539</span>
      </div>
      <RoutingSelector />
      <MessageLog />
      <PromptComposer />
    </div>
  );
}

// ── Styles ────────────────────────────────────────────────────────────────────
const styles = {
  root:        { display: 'flex', flexDirection: 'column' as const, height: '100%', gap: 12, padding: 16, background: 'var(--bg-surface)', borderLeft: '1px solid var(--border)' },
  header:      { display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', borderBottom: '1px solid var(--border)', paddingBottom: 10 },
  routingBox:  { display: 'flex', flexDirection: 'column' as const, gap: 8, padding: '10px 12px', background: 'var(--bg-raised)', borderRadius: 4, border: '1px solid var(--border)' },
  emptyLog:    { flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 11 },
  logOuter:    { flex: 1, display: 'flex', flexDirection: 'column' as const, minHeight: 0, border: '1px solid var(--border)', borderRadius: 4, overflow: 'hidden' },
  logHeader:   { display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '5px 10px', background: 'var(--bg-raised)', borderBottom: '1px solid var(--border)', flexShrink: 0 },
  logScroll:   { flex: 1, overflowY: 'auto' as const, padding: 10, display: 'flex', flexDirection: 'column' as const, gap: 8 },
  logEntry:    { borderLeft: '2px solid', paddingLeft: 10, display: 'flex', flexDirection: 'column' as const, gap: 3 },
  composerWrap:{ display: 'flex', gap: 8, alignItems: 'flex-end', flexShrink: 0 },
  textarea:    { flex: 1, background: 'var(--bg-raised)', border: '1px solid var(--border)', borderRadius: 4, color: 'var(--text-primary)', fontFamily: 'var(--font-mono)', fontSize: 13, lineHeight: 1.5, padding: '10px 12px', resize: 'none' as const, minHeight: 80, maxHeight: 180, outline: 'none' },
  composerBtns:{ display: 'flex', flexDirection: 'column' as const, gap: 6, flexShrink: 0 },
  micBtn:      { width: 44, height: 44, border: '1px solid', borderRadius: 4, cursor: 'pointer', fontSize: 18, display: 'flex', alignItems: 'center', justifyContent: 'center', transition: 'all 150ms ease', fontFamily: 'var(--font-mono)' } as React.CSSProperties,
  sendBtn:     { height: 60, width: 70, fontSize: 11, letterSpacing: '0.06em', borderRadius: 4, transition: 'opacity 0.15s', flexShrink: 0 } as React.CSSProperties,
};
