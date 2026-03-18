import type { TabId, EngineHealth } from '../App';

const tabs: { id: TabId; label: string; accent?: string }[] = [
  { id: 'overview', label: 'OVERVIEW' },
  { id: 'live', label: 'AETHERIS LIVE', accent: 'var(--color-green)' },
  { id: 'transpile', label: 'TRANSPILE' },
  { id: 'investigate', label: 'INVESTIGATE' },
];

interface Props {
  tab: TabId;
  setTab: (t: TabId) => void;
  engine: EngineHealth | null;
  wasmReady: boolean;
  now: Date;
}

export default function Header({ tab, setTab, engine, wasmReady, now }: Props) {
  const engineOk = engine?.status === 'ok';
  const backendsUp = engine?.backends.filter((b) => b.status === 'reachable').length ?? 0;

  return (
    <header className="sticky top-0 z-50 bg-[var(--color-surface)]/95 backdrop-blur border-b border-[var(--color-border)]">
      <div className="max-w-[1400px] mx-auto px-4 h-12 flex items-center gap-6">
        {/* Logo */}
        <div className="flex items-center gap-2 mr-2 shrink-0">
          <div className="w-6 h-6 rounded bg-gradient-to-br from-[var(--color-accent)] to-[var(--color-cyan)] flex items-center justify-center text-[10px] font-bold text-white">U</div>
          <span className="text-[var(--color-text-bright)] font-semibold tracking-wide text-sm">UNIQL</span>
          <span className="text-[9px] text-[var(--color-text-dim)] font-mono">v0.3</span>
        </div>

        {/* Tabs */}
        <nav className="flex items-center gap-1 flex-1">
          {tabs.map((t) => (
            <button
              key={t.id}
              onClick={() => setTab(t.id)}
              className={`px-3 py-1.5 rounded text-xs font-medium tracking-wider transition-all cursor-pointer ${
                tab === t.id
                  ? 'bg-[var(--color-accent-dim)] text-[var(--color-accent)] border border-[var(--color-accent)]/30'
                  : 'text-[var(--color-text-dim)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-2)]'
              }`}
              style={tab === t.id && t.accent ? { color: t.accent, borderColor: `${t.accent}33`, background: `${t.accent}15` } : undefined}
            >
              {t.label}
            </button>
          ))}
        </nav>

        {/* Status indicators */}
        <div className="flex items-center gap-4 shrink-0 text-xs">
          <StatusDot ok={wasmReady} label="WASM" />
          <StatusDot ok={engineOk} label="ENGINE" />
          {engineOk && (
            <span className="text-[var(--color-text-dim)] font-mono">
              {backendsUp} backend{backendsUp !== 1 ? 's' : ''}
            </span>
          )}
          <span className="text-[var(--color-text-dim)] font-mono">{now.toLocaleTimeString('tr-TR')}</span>
        </div>
      </div>
    </header>
  );
}

function StatusDot({ ok, label }: { ok: boolean; label: string }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className={`w-1.5 h-1.5 rounded-full ${ok ? 'bg-[var(--color-green)]' : 'bg-[var(--color-text-dim)]'}`} />
      <span className={ok ? 'text-[var(--color-green)]' : 'text-[var(--color-text-dim)]'}>{label}</span>
    </span>
  );
}
