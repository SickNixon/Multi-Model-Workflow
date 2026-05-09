// src/components/ModelCard.tsx
import { invoke } from '@tauri-apps/api/core';
import { type PanelId, type PanelInfo, PANEL_COLORS } from '../types';
import { useStore } from '../store';

interface Props { panelId: PanelId; info: PanelInfo | undefined; }

const dot: React.CSSProperties = {
  display: 'inline-block', width: 7, height: 7, borderRadius: '50%', flexShrink: 0,
};

function StatusDot({ info, isBrowser }: { info: PanelInfo | undefined; isBrowser: boolean }) {
  if (isBrowser) return <span style={{ ...dot, background: '#a78bfa' }} title="Opens in system browser" />;
  const s = info?.status.status;
  const colors: Record<string, string> = {
    loading: '#7fa8c2', idle: '#22c55e', generating: 'var(--amber)', done: '#22c55e', error: '#ef4444',
  };
  const isGen = s === 'generating';
  if (!s || s === 'closed') return <span style={{ ...dot, background: 'var(--text-dim)' }} />;
  return <span style={{
    ...dot, background: colors[s] ?? 'var(--text-dim)',
    animationName: isGen ? 'pulse-amber' : 'none',
    animationDuration: '1.4s', animationIterationCount: 'infinite',
  }} />;
}

function StatusLabel({ info, isBrowser }: { info: PanelInfo | undefined; isBrowser: boolean }) {
  if (isBrowser) return <span style={{ color: '#a78bfa', fontSize: 10, letterSpacing: '0.1em' }}>BROWSER</span>;
  const s = info?.status.status ?? 'closed';
  const labels: Record<string, string> = {
    closed: 'CLOSED', loading: 'LOADING…', idle: 'READY',
    generating: 'GENERATING…', done: 'DONE', error: 'ERROR',
  };
  const colours: Record<string, string> = {
    closed: 'var(--text-dim)', loading: 'var(--text-secondary)', idle: 'var(--green)',
    generating: 'var(--amber)', done: 'var(--green)', error: 'var(--red)',
  };
  return (
    <span style={{ color: colours[s] ?? 'var(--muted)', fontSize: 10, letterSpacing: '0.1em' }}>
      {labels[s] ?? s.toUpperCase()}
    </span>
  );
}

export function ModelCard({ panelId, info }: Props) {
  const openPanel    = useStore(s => s.openPanel);
  const closePanel   = useStore(s => s.closePanel);
  const showPanel    = useStore(s => s.showPanel);
  const hidePanel    = useStore(s => s.hidePanel);
  const capturePanel = useStore(s => s.capturePanel);

  // Claude never loads in a WebView — Cloudflare Turnstile always blocks it.
  // OPEN → launches claude.ai in the system browser instead.
  const isBrowser    = panelId === 'claude';
  const status       = info?.status.status;
  const isOpen       = !isBrowser && status !== 'closed' && status !== undefined;
  const isGenerating = status === 'generating';
  const accentColor  = PANEL_COLORS[panelId];

  return (
    <div style={{
      ...card,
      borderColor: isOpen ? accentColor : isBrowser ? '#a78bfa44' : 'var(--border)',
      boxShadow:   isOpen ? `0 0 12px ${accentColor}22` : 'none',
    }}>
      {/* Header row */}
      <div style={row}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <StatusDot info={info} isBrowser={isBrowser} />
          <span className="display" style={{ fontSize: 20, color: accentColor }}>
            {panelId.toUpperCase()}
          </span>
        </div>
        <StatusLabel info={info} isBrowser={isBrowser} />
      </div>

      {/* Output preview — not shown for browser panels */}
      {!isBrowser && (
        <div style={{
          ...preview,
          opacity: isOpen ? 1 : 0.3,
          animationName: isGenerating ? 'pulse-amber' : 'none',
          animationDuration: '1.4s', animationIterationCount: 'infinite',
        }}>
          {isGenerating
            ? '▌  generating…'
            : (info?.last_output
              ? info.last_output.slice(0, 140) + (info.last_output.length > 140 ? '…' : '')
              : '— no output yet —')}
        </div>
      )}

      {/* Claude browser-only info */}
      {isBrowser && (
        <div style={{ fontSize: 11, color: 'var(--text-secondary)', lineHeight: 1.6, padding: '4px 0' }}>
          Claude opens in your system browser — Cloudflare blocks WebView access.
          Log in once in your browser and keep that tab open while orchestrating.
        </div>
      )}

      {/* Action buttons */}
      <div style={{ ...row, flexWrap: 'wrap', gap: 6 }}>
        {isBrowser ? (
          /* Claude: single browser-open button */
          <button
            className="btn-primary"
            style={{ background: '#7c3aed', fontSize: 11 }}
            onClick={() => openPanel(panelId)}
            title="Open claude.ai in your system browser"
          >
            OPEN IN BROWSER ↗
          </button>
        ) : !isOpen ? (
          <button
            className="btn-primary"
            style={{ background: accentColor, fontSize: 11 }}
            onClick={() => openPanel(panelId)}
          >
            OPEN
          </button>
        ) : (
          <>
            <button className="btn-ghost" style={{ fontSize: 11 }} onClick={() => showPanel(panelId)}>
              VIEW
            </button>
            <button className="btn-ghost" style={{ fontSize: 11 }} onClick={() => hidePanel(panelId)}>
              HIDE
            </button>
            <button
              className="btn-ghost"
              style={{ fontSize: 11, color: 'var(--accent)', borderColor: 'var(--accent)' }}
              onClick={() => capturePanel(panelId)}
              title="Capture current output"
            >
              CAPTURE
            </button>
            <button
              className="btn-ghost"
              style={{ fontSize: 10, color: 'var(--text-dim)', borderColor: 'var(--border)' }}
              onClick={() => void invoke('open_panel_devtools', { panelId })}
              title="Open DevTools for this panel (debug)"
            >
              DEV
            </button>
            <button
              className="btn-ghost"
              style={{ fontSize: 11, color: 'var(--red)', borderColor: 'var(--red)', marginLeft: 'auto' }}
              onClick={() => closePanel(panelId)}
            >
              CLOSE
            </button>
          </>
        )}
      </div>
    </div>
  );

const card: React.CSSProperties = {
  display: 'flex', flexDirection: 'column', gap: 10,
  padding: '14px 16px', background: 'var(--bg-surface)',
  border: '1px solid var(--border)', borderRadius: 6,
  transition: 'border-color 0.2s, box-shadow 0.2s',
};
const row: React.CSSProperties = {
  display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 6,
};
const preview: React.CSSProperties = {
  fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-secondary)',
  lineHeight: 1.5, minHeight: 36, padding: '6px 8px',
  background: 'var(--bg-raised)', borderRadius: 3,
  overflow: 'hidden', whiteSpace: 'pre-wrap', wordBreak: 'break-word',
};
