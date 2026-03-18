import type { TabId, EngineHealth } from '../App';

const tabs: { id: TabId; label: string }[] = [
  { id: 'overview', label: 'Home' },
  { id: 'live', label: 'Live Demo' },
  { id: 'transpile', label: 'Playground' },
  { id: 'investigate', label: 'Investigate' },
];

interface Props {
  tab: TabId;
  setTab: (t: TabId) => void;
  engine: EngineHealth | null;
  wasmReady: boolean;
  now: Date;
}

export default function Header({ tab, setTab, engine, wasmReady }: Props) {
  const engineOk = engine?.status === 'ok';

  return (
    <header className="sticky top-0 z-50 bg-[var(--color-surface)]/95 backdrop-blur border-b border-[var(--color-border)]">
      <div className="max-w-[1400px] mx-auto px-6 h-14 flex items-center gap-8">
        {/* Logo */}
        <button onClick={() => setTab('overview')} className="flex items-center gap-2.5 shrink-0 cursor-pointer group">
          <div className="w-7 h-7 rounded-lg bg-gradient-to-br from-[var(--color-accent)] to-[var(--color-cyan)] flex items-center justify-center text-[11px] font-bold text-white shadow-lg shadow-[var(--color-accent)]/20 group-hover:shadow-[var(--color-accent)]/40 transition-shadow">U</div>
          <div className="flex items-baseline gap-1.5">
            <span className="text-[var(--color-text-bright)] font-bold tracking-wide text-[15px]">UniQL</span>
            <span className="text-[9px] text-[var(--color-text-dim)] font-mono">v0.3.0</span>
          </div>
        </button>

        {/* Nav */}
        <nav className="flex items-center gap-1 flex-1">
          {tabs.map((t) => (
            <button
              key={t.id}
              onClick={() => setTab(t.id)}
              className={`px-3 py-1.5 rounded-md text-[13px] font-medium transition-all cursor-pointer ${
                tab === t.id
                  ? 'text-[var(--color-text-bright)] bg-[var(--color-surface-3)]'
                  : 'text-[var(--color-text-dim)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-2)]'
              }`}
            >
              {t.label}
            </button>
          ))}
        </nav>

        {/* Right side */}
        <div className="flex items-center gap-4 shrink-0">
          {/* Status */}
          <div className="flex items-center gap-3 text-[11px]">
            <StatusPill ok={wasmReady} label="WASM" />
            <StatusPill ok={engineOk} label="Engine" />
          </div>
          {/* GitHub */}
          <a
            href="https://github.com/zheimr/uniQL"
            target="_blank"
            rel="noopener noreferrer"
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-md border border-[var(--color-border)] text-[11px] text-[var(--color-text-dim)] hover:text-[var(--color-text)] hover:border-[var(--color-border-bright)] transition-all"
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>
            GitHub
          </a>
        </div>
      </div>
    </header>
  );
}

function StatusPill({ ok, label }: { ok: boolean; label: string }) {
  return (
    <span className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full ${
      ok ? 'bg-[var(--color-green)]/10 text-[var(--color-green)]' : 'bg-[var(--color-surface-3)] text-[var(--color-text-dim)]'
    }`}>
      <span className={`w-1.5 h-1.5 rounded-full ${ok ? 'bg-[var(--color-green)]' : 'bg-[var(--color-text-dim)]'}`} />
      {label}
    </span>
  );
}
