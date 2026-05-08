// src/components/OrchestratorPanel.tsx
// The master control surface. Contains:
//   - Prompt composer with routing mode selector
//   - Active panel target toggles
//   - Conversation/output history log

import { useRef, useEffect, KeyboardEvent } from 'react';
import { useStore } from '../store';
import {
  type PanelId,
  type RoutingMode,
  ALL_PANEL_IDS,
  PANEL_LABELS,
  PANEL_COLORS,
} from '../types';

// ── Routing mode selector ─────────────────────────────────────────────────────

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
    const current = routing.targets;
    const next = current.includes(id)
      ? current.filter(t => t !== id)
      : [...current, id];
    if (next.length > 0) setRouting({ targets: next });
  };

  return (
    <div style={styles.routingRow}>
      {/* Mode pills */}
      <div style={styles.modePills}>
        {modes.map(m => (
          <button
            key={m.id}
            className={routing.mode === m.id ? 'btn-primary' : 'btn-ghost'}
            style={{ fontSize: 10, padding: '4px 10px' }}
            onClick={() => setRouting({ mode: m.id })}
          >
            {m.label}
          </button>
        ))}
      </div>

      {/* Target toggles */}
      <div style={styles.targetToggles}>
        <span style={styles.routeLabel}>ROUTE TO:</span>
        {ALL_PANEL_IDS.map(id => {
          const isOpen   = panels[id]?.status?.status !== 'closed';
          const isTarget = routing.targets.includes(id);
          const color    = PANEL_COLORS[id];
          return (
            <button
              key={id}
              onClick={() => toggleTarget(id)}
              disabled={!isOpen}
              style={{
                ...styles.targetBtn,
                borderColor: isTarget ? color : 'var(--border)',
                color:       isTarget ? color : 'var(--text-dim)',
                background:  isTarget ? `${color}15` : 'transparent',
                opacity:     isOpen ? 1 : 0.35,
              }}
            >
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

  // Auto-scroll to bottom on new messages
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [history.length]);

  if (history.length === 0) {
    return (
      <div style={styles.emptyLog}>
        <span className="dim">// no messages yet — open some panels and fire away</span>
      </div>
    );
  }

  return (
    <div style={styles.logOuter}>
      <div style={styles.logHeader}>
        <span className="dim" style={{ fontSize: 10 }}>MESSAGE LOG</span>
        <button
          className="btn-ghost"
          style={{ fontSize: 9, padding: '2px 8px' }}
          onClick={clearHistory}
        >
          CLEAR
        </button>
      </div>
      <div ref={scrollRef} style={styles.logScroll}>
        {history.map(entry => {
          const isUser   = entry.from === 'user';
          const fromColor = isUser ? 'var(--accent)' : PANEL_COLORS[entry.from as PanelId] ?? 'var(--text-secondary)';
          const fromLabel = isUser ? 'YOU' : (entry.from as string).toUpperCase();
          return (
            <div key={entry.id} style={{ ...styles.logEntry, borderLeftColor: fromColor }}>
              <div style={styles.logEntryHeader}>
                <span style={{ color: fromColor, fontSize: 10, fontWeight: 700 }}>{fromLabel}</span>
                <span className="dim" style={{ fontSize: 9 }}>
                  {new Date(entry.timestamp).toLocaleTimeString()}
                </span>
              </div>
              <div style={styles.logEntryBody} className={isUser ? '' : 'muted'}>
                {entry.content}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ── Prompt composer ───────────────────────────────────────────────────────────

function PromptComposer() {
  const draft      = useStore(s => s.promptDraft);
  const setDraft   = useStore(s => s.setPromptDraft);
  const sendPrompt = useStore(s => s.sendPrompt);
  const panels     = useStore(s => s.panels);

  const hasOpenPanels = ALL_PANEL_IDS.some(
    id => panels[id]?.status?.status !== 'closed' && panels[id]?.status?.status !== undefined
  );

  const hasGenerating = ALL_PANEL_IDS.some(
    id => panels[id]?.status?.status === 'generating'
  );

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // Cmd+Enter or Ctrl+Enter to send
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      if (draft.trim() && hasOpenPanels && !hasGenerating) {
        void sendPrompt(draft);
      }
    }
  };

  return (
    <div style={styles.composer}>
      <textarea
        value={draft}
        onChange={e => setDraft(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder={
          hasOpenPanels
            ? 'Type a prompt… ⌘↵ to send'
            : 'Open at least one panel first'
        }
        disabled={!hasOpenPanels || hasGenerating}
        style={{
          ...styles.textarea,
          opacity: hasOpenPanels ? 1 : 0.5,
        }}
      />
      <button
        className="btn-primary"
        disabled={!draft.trim() || !hasOpenPanels || hasGenerating}
        onClick={() => void sendPrompt(draft)}
        style={{
          ...styles.sendBtn,
          opacity: (draft.trim() && hasOpenPanels && !hasGenerating) ? 1 : 0.4,
        }}
      >
        {hasGenerating ? '▌ WAIT…' : 'SEND ⌘↵'}
      </button>
    </div>
  );
}

// ── Main export ───────────────────────────────────────────────────────────────

export function OrchestratorPanel() {
  return (
    <div style={styles.root}>
      {/* Header */}
      <div style={styles.header}>
        <span className="display" style={styles.title}>VIBE ORCHESTRATOR</span>
        <span className="dim" style={{ fontSize: 10 }}>BRIDGE :7539</span>
      </div>

      {/* Routing config */}
      <RoutingSelector />

      {/* Message log — takes remaining space */}
      <MessageLog />

      {/* Prompt composer — pinned to bottom */}
      <PromptComposer />
    </div>
  );
}

// ── Styles ────────────────────────────────────────────────────────────────────

const styles = {
  root: {
    display: 'flex',
    flexDirection: 'column' as const,
    height: '100%',
    gap: 12,
    padding: 16,
    background: 'var(--bg-surface)',
    borderLeft: '1px solid var(--border)',
  },

  header: {
    display: 'flex',
    alignItems: 'baseline',
    justifyContent: 'space-between',
    borderBottom: '1px solid var(--border)',
    paddingBottom: 10,
  },

  title: {
    fontSize: 22,
    color: 'var(--accent)',
    letterSpacing: '0.08em',
  },

  routingRow: {
    display: 'flex',
    flexDirection: 'column' as const,
    gap: 8,
    padding: '10px 12px',
    background: 'var(--bg-raised)',
    borderRadius: 4,
    border: '1px solid var(--border)',
  },

  modePills: {
    display: 'flex',
    gap: 6,
  },

  targetToggles: {
    display: 'flex',
    alignItems: 'center',
    gap: 6,
    flexWrap: 'wrap' as const,
  },

  routeLabel: {
    fontSize: 9,
    color: 'var(--text-dim)',
    letterSpacing: '0.1em',
    marginRight: 4,
  },

  targetBtn: {
    padding: '3px 9px',
    fontSize: 10,
    border: '1px solid',
    borderRadius: 3,
    letterSpacing: '0.06em',
    transition: 'all 120ms ease',
    cursor: 'pointer',
    fontFamily: 'var(--font-mono)',
  } as React.CSSProperties,

  emptyLog: {
    flex: 1,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    fontSize: 11,
  },

  logOuter: {
    flex: 1,
    display: 'flex',
    flexDirection: 'column' as const,
    minHeight: 0,
    border: '1px solid var(--border)',
    borderRadius: 4,
    overflow: 'hidden',
  },

  logHeader: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    padding: '5px 10px',
    background: 'var(--bg-raised)',
    borderBottom: '1px solid var(--border)',
    flexShrink: 0,
  },

  logScroll: {
    flex: 1,
    overflowY: 'auto' as const,
    padding: 10,
    display: 'flex',
    flexDirection: 'column' as const,
    gap: 8,
  },

  logEntry: {
    borderLeft: '2px solid',
    paddingLeft: 10,
    display: 'flex',
    flexDirection: 'column' as const,
    gap: 3,
  },

  logEntryHeader: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
  },

  logEntryBody: {
    fontSize: 12,
    lineHeight: 1.6,
    userSelect: 'text' as const,
    whiteSpace: 'pre-wrap' as const,
    wordBreak: 'break-word' as const,
    maxHeight: 200,
    overflowY: 'auto' as const,
  },

  composer: {
    display: 'flex',
    gap: 8,
    alignItems: 'flex-end',
    flexShrink: 0,
  },

  textarea: {
    flex: 1,
    background: 'var(--bg-raised)',
    border: '1px solid var(--border)',
    borderRadius: 4,
    color: 'var(--text-primary)',
    fontFamily: 'var(--font-mono)',
    fontSize: 13,
    lineHeight: 1.5,
    padding: '10px 12px',
    resize: 'none' as const,
    minHeight: 80,
    maxHeight: 200,
    outline: 'none',
    transition: 'border-color 0.15s',
  },

  sendBtn: {
    height: 80,
    minWidth: 80,
    fontSize: 11,
    letterSpacing: '0.06em',
    flexShrink: 0,
    borderRadius: 4,
    transition: 'opacity 0.15s',
  },
};
