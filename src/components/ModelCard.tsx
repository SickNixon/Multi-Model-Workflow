// src/components/ModelCard.tsx
import { type PanelId, type PanelInfo, PANEL_COLORS } from '../types';
import { useStore } from '../store';

interface Props { panelId: PanelId; info: PanelInfo | undefined; }

function StatusDot({ info }: { info: PanelInfo | undefined }) {
  if (!info || info.status.status === 'closed') return <span style={{ ...dot, background: 'var(--text-dim)' }} />;
  const colors: Record<string, string> = {
    loading: '#7fa8c2', idle: '#22c55e', generating: 'var(--amber)', done: '#22c55e', error: '#ef4444',
  };
  const isGen = info.status.status === 'generating';
  return <span style={{ ...dot, background: colors[info.status.status] ?? 'var(--text-dim)',
    animationName: isGen ? 'pulse-amber' : 'none', animationDuration: '1.4s', animationIterationCount: 'infinite' }} />;
}

function StatusLabel({ info }: { info: PanelInfo | undefined }) {
  if (!info) return <span style={{ color: 'var(--text-dim)', fontSize: 10 }}>CLOSED</span>;
  const s = info.status.status;
  const labels: Record<string, string> = { closed: 'CLOSED', loading: 'LOADING…', idle: 'READY', generating: 'GENERATING…', done: 'DONE', error: 'ERROR' };
  const colours: Record<string, string> = { closed: 'var(--text-dim)', loading: 'var(--text-secondary)', idle: 'var(--green)', generating: 'var(--amber)', done: 'var(--green)', error: 'var(--red)' };
  return <span style={{ color: colours[s] ?? 'var(--muted)', fontSize: 10, letterSpacing: '0.1em' }}>{labels[s] ?? s.toUpperCase()}</span>;
}

const dot: React.CSSProperties = { display: 'inline-block', width: 7, height: 7, borderRadius: '50%', flexShrink: 0 };

export function ModelCard({ panelId, info }: Props) {
  const openPanel  = useStore(s => s.openPanel);
  const closePanel = useStore(s => s.closePanel);
  const showPanel  = useStore(s => s.showPanel);
  const hidePanel  = useStore(s => s.hidePanel);

  const status      = info?.status.status;
  const isOpen      = status !== 'closed' && status !== undefined;
  const isGenerating = status === 'generating';
  const accentColor = PANEL_COLORS[panelId];

  return (
    <div style={{ ...card, borderColor: isOpen ? accentColor : 'var(--border)', boxShadow: isOpen ? `0 0 12px ${accentColor}22` : 'none' }}>
      {/* Header */}
      <div style={row}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <StatusDot info={info} />
          <span className="display" style={{ fontSize: 20, color: accentColor }}>{panelId.toUpperCase()}</span>
        </div>
        <StatusLabel info={info} />
      </div>

      {/* Output preview */}
      <div style={{ ...preview, opacity: isOpen ? 1 : 0.3,
        animationName: isGenerating ? 'pulse-amber' : 'none',
        animationDuration: '1.4s', animationIterationCount: 'infinite' }}>
        {isGenerating ? '▌  generating…'
          : (info?.last_output
            ? info.last_output.slice(0, 120) + (info.last_output.length > 120 ? '…' : '')
            : '— no output yet —')}
      </div>

      {/* Actions */}
      <div style={row}>
        {!isOpen ? (
          <button className="btn-primary" style={{ background: accentColor, fontSize: 11 }} onClick={() => openPanel(panelId)}>
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
            <button className="btn-ghost" style={{ fontSize: 11, color: 'var(--red)', borderColor: 'var(--red)', marginLeft: 'auto' }} onClick={() => closePanel(panelId)}>
              CLOSE
            </button>
          </>
        )}
      </div>
    </div>
  );
}

const card: React.CSSProperties = {
  display: 'flex', flexDirection: 'column', gap: 10, padding: '14px 16px',
  background: 'var(--bg-surface)', border: '1px solid var(--border)', borderRadius: 6,
  transition: 'border-color 0.2s, box-shadow 0.2s',
};
const row: React.CSSProperties = { display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 6 };
const preview: React.CSSProperties = {
  fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-secondary)', lineHeight: 1.5,
  minHeight: 36, padding: '6px 8px', background: 'var(--bg-raised)', borderRadius: 3,
  overflow: 'hidden', whiteSpace: 'pre-wrap', wordBreak: 'break-word',
};
