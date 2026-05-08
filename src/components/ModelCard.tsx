// src/components/ModelCard.tsx
// A card in the panel grid representing one AI model.
// Shows: name, open/close toggle, current status, last output snippet.

import { type PanelId, type PanelInfo, PANEL_COLORS } from '../types';
import { useStore } from '../store';

interface Props {
  panelId: PanelId;
  info: PanelInfo | undefined;
}

// ── Status badge ──────────────────────────────────────────────────────────────

function StatusDot({ info }: { info: PanelInfo | undefined }) {
  if (!info) return <span style={styles.dot.closed} />;
  const s = info.status.status;
  if (s === 'closed')     return <span style={styles.dot.closed} title="Closed" />;
  if (s === 'loading')    return <span style={{ ...styles.dot.base, background: '#7fa8c2' }} title="Loading…" />;
  if (s === 'idle')       return <span style={{ ...styles.dot.base, background: '#22c55e' }} title="Ready" />;
  if (s === 'generating') return <span style={{ ...styles.dot.generating }} title="Generating…" />;
  if (s === 'done')       return <span style={{ ...styles.dot.base, background: '#22c55e' }} title="Done" />;
  if (s === 'error')      return <span style={{ ...styles.dot.base, background: '#ef4444' }} title="Error" />;
  return null;
}

function StatusLabel({ info }: { info: PanelInfo | undefined }) {
  if (!info) return <span className="dim">CLOSED</span>;
  const s = info.status.status;
  const map: Record<string, string> = {
    closed: 'CLOSED', loading: 'LOADING…', idle: 'READY',
    generating: 'GENERATING…', done: 'DONE', error: 'ERROR',
  };
  const colour: Record<string, string> = {
    closed: 'var(--text-dim)', loading: 'var(--text-secondary)',
    idle: 'var(--green)', generating: 'var(--amber)',
    done: 'var(--green)', error: 'var(--red)',
  };
  return <span style={{ color: colour[s] ?? 'var(--muted)', fontSize: 10, letterSpacing: '0.1em' }}>{map[s] ?? s.toUpperCase()}</span>;
}

// ── Main component ────────────────────────────────────────────────────────────

export function ModelCard({ panelId, info }: Props) {
  const openPanel  = useStore(s => s.openPanel);
  const closePanel = useStore(s => s.closePanel);
  const isOpen     = info?.status.status !== 'closed' && info?.status.status !== undefined;
  const accentColor = PANEL_COLORS[panelId];

  const isGenerating = info?.status.status === 'generating';

  return (
    <div style={{
      ...styles.card,
      borderColor: isOpen ? accentColor : 'var(--border)',
      boxShadow: isOpen ? `0 0 12px ${accentColor}22` : 'none',
    }}>
      {/* Header */}
      <div style={styles.header}>
        <div style={styles.headerLeft}>
          <StatusDot info={info} />
          <span className="display" style={{ fontSize: 20, color: accentColor }}>
            {panelId.toUpperCase()}
          </span>
        </div>
        <StatusLabel info={info} />
      </div>

      {/* Output preview */}
      <div style={{
        ...styles.preview,
        opacity: isOpen ? 1 : 0.3,
        animationName: isGenerating ? 'pulse-amber' : 'none',
        animationDuration: '1.4s',
        animationIterationCount: 'infinite',
      }}>
        {isGenerating
          ? '▌  generating…'
          : (info?.last_output
            ? info.last_output.slice(0, 120) + (info.last_output.length > 120 ? '…' : '')
            : '— no output yet —'
          )
        }
      </div>

      {/* Action buttons */}
      <div style={styles.actions}>
        {!isOpen ? (
          <button
            className="btn-primary"
            style={{ background: accentColor, fontSize: 11 }}
            onClick={() => openPanel(panelId)}
          >
            OPEN
          </button>
        ) : (
          <button
            className="btn-ghost"
            style={{ fontSize: 11, color: 'var(--red)', borderColor: 'var(--red)' }}
            onClick={() => closePanel(panelId)}
          >
            CLOSE
          </button>
        )}
      </div>
    </div>
  );
}

// ── Styles ────────────────────────────────────────────────────────────────────

const styles = {
  card: {
    display: 'flex',
    flexDirection: 'column' as const,
    gap: 10,
    padding: '14px 16px',
    background: 'var(--bg-surface)',
    border: '1px solid var(--border)',
    borderRadius: 6,
    transition: 'border-color 0.2s, box-shadow 0.2s',
    cursor: 'default',
  } as React.CSSProperties,

  header: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
  } as React.CSSProperties,

  headerLeft: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
  } as React.CSSProperties,

  preview: {
    fontFamily: 'var(--font-mono)',
    fontSize: 11,
    color: 'var(--text-secondary)',
    lineHeight: 1.5,
    minHeight: 36,
    padding: '6px 8px',
    background: 'var(--bg-raised)',
    borderRadius: 3,
    overflow: 'hidden',
    whiteSpace: 'pre-wrap' as const,
    wordBreak: 'break-word' as const,
  } as React.CSSProperties,

  actions: {
    display: 'flex',
    gap: 8,
    marginTop: 2,
  } as React.CSSProperties,

  dot: {
    base: {
      display: 'inline-block',
      width: 7,
      height: 7,
      borderRadius: '50%',
      flexShrink: 0,
    } as React.CSSProperties,
    closed: {
      display: 'inline-block',
      width: 7,
      height: 7,
      borderRadius: '50%',
      background: 'var(--text-dim)',
      flexShrink: 0,
    } as React.CSSProperties,
    generating: {
      display: 'inline-block',
      width: 7,
      height: 7,
      borderRadius: '50%',
      background: 'var(--amber)',
      flexShrink: 0,
      animationName: 'pulse-amber',
      animationDuration: '1.4s',
      animationIterationCount: 'infinite',
    } as React.CSSProperties,
  },
};
