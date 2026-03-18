import { useState, useEffect } from 'react';
import type { EngineHealth, TabId } from '../App';

const ENGINE_URL = `http://${window.location.hostname}:9090`;

interface Props {
  engine: EngineHealth | null;
  wasm: unknown;
  transpile: (q: string, b: string) => string | null;
  setTab: (t: TabId) => void;
}

const heroQuery = `SHOW timeseries FROM victoria
WHERE __name__ = "vsphere_host_cpu_usage_average"
  AND clustername = "DELLR750_Cluster"`;

const transpileTargets = [
  { backend: 'promql', label: 'PromQL' },
  { backend: 'logql', label: 'LogQL' },
  { backend: 'logsql', label: 'LogsQL' },
];

const pipelineLayers = [
  'Lexer', 'Parser', 'AST', 'Semantic', 'TypeCheck', 'Macro',
  'Optimize', 'Plan', 'Validate', 'IR Gen', 'Transpile', 'Format',
];

const layerColors = [
  '#7c5cfc', '#8b6cf7', '#9b7cf2', '#ab8ced', '#bb9ce8', '#cb8ce3',
  '#e09c5e', '#e0b640', '#90d050', '#50c8a0', '#40c8d0', '#50b8e0',
];

interface LiveStats {
  snmpDevices: number;
  vms: number;
  services: string;
  engineMs: number;
}

export default function OverviewTab({ engine, wasm, transpile, setTab }: Props) {
  const [typedQuery, setTypedQuery] = useState('');
  const [typingDone, setTypingDone] = useState(false);
  const [activeTarget, setActiveTarget] = useState(0);
  const [stats, setStats] = useState<LiveStats>({ snmpDevices: 0, vms: 0, services: '0/0', engineMs: 0 });

  // Typing animation
  useEffect(() => {
    let i = 0;
    setTypedQuery('');
    setTypingDone(false);
    const interval = setInterval(() => {
      if (i < heroQuery.length) {
        setTypedQuery(heroQuery.slice(0, i + 1));
        i++;
      } else {
        clearInterval(interval);
        setTypingDone(true);
      }
    }, 18);
    return () => clearInterval(interval);
  }, []);

  // Cycle transpile targets
  useEffect(() => {
    if (!typingDone) return;
    const i = setInterval(() => setActiveTarget((p) => (p + 1) % 3), 2500);
    return () => clearInterval(i);
  }, [typingDone]);

  // Fetch live AETHERIS stats
  useEffect(() => {
    const fetchStats = async () => {
      try {
        const q = (query: string) => fetch(`${ENGINE_URL}/v1/query`, {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ query }),
        }).then(r => r.json());

        const [snmpRes, vmRes, svcUpRes, svcTotalRes] = await Promise.allSettled([
          q('SHOW timeseries FROM victoria WHERE __name__ = "count(snmpv2_device_up==1)"'),
          q('SHOW timeseries FROM victoria WHERE __name__ = "count(count by (vmname)(vsphere_vm_cpu_usage_average))"'),
          q('SHOW timeseries FROM victoria WHERE __name__ = "count(up==1)"'),
          q('SHOW timeseries FROM victoria WHERE __name__ = "count(up)"'),
        ]);

        const getVal = (res: PromiseSettledResult<any>) =>
          res.status === 'fulfilled' ? parseInt(res.value?.data?.data?.result?.[0]?.value?.[1] || '0') : 0;
        const getMs = (res: PromiseSettledResult<any>) =>
          res.status === 'fulfilled' ? (res.value?.metadata?.total_time_ms || 0) : 0;

        setStats({
          snmpDevices: getVal(snmpRes),
          vms: getVal(vmRes),
          services: `${getVal(svcUpRes)}/${getVal(svcTotalRes)}`,
          engineMs: getMs(svcUpRes),
        });
      } catch { /* graceful fail */ }
    };
    fetchStats();
  }, []);

  const engineOk = engine?.status === 'ok';

  return (
    <div className="space-y-4 pt-4 animate-fade-in">
      {/* Hero: AETHERIS + UNIQL branding */}
      <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-5 relative overflow-hidden">
        {/* Subtle gradient accent */}
        <div className="absolute top-0 left-0 right-0 h-[1px] bg-gradient-to-r from-transparent via-[var(--color-accent)] to-transparent opacity-60" />
        <div className="flex flex-col lg:flex-row lg:items-center gap-6">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-3 mb-2">
              <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-[var(--color-accent)] to-[var(--color-cyan)] flex items-center justify-center text-xs font-bold text-white shadow-lg shadow-[var(--color-accent)]/20">U</div>
              <h1 className="text-2xl font-bold text-[var(--color-text-bright)]">
                Write Once, Query Everything
              </h1>
            </div>
            <p className="text-[var(--color-text-dim)] text-sm mb-3">
              Unified observability query language — single syntax for metrics, logs, and traces across all backends
            </p>
            <div className="flex items-center gap-2 flex-wrap">
              {['Rust Engine', 'WASM Transpiler', '12-Layer Pipeline', 'Sub-ms Parse'].map((tag) => (
                <span key={tag} className="text-[10px] px-2 py-0.5 rounded border border-[var(--color-border)] text-[var(--color-text-dim)] bg-[var(--color-surface-3)]">{tag}</span>
              ))}
            </div>
          </div>
          {/* Live AETHERIS stats */}
          <div className="flex items-center gap-5 shrink-0">
            <LiveStat value={String(stats.snmpDevices)} label="SNMP Devices" color="var(--color-cyan)" pulse />
            <LiveStat value={String(stats.vms)} label="VMs" color="var(--color-green)" pulse />
            <LiveStat value={stats.services} label="Services" color="var(--color-accent)" pulse />
            <LiveStat value={`${stats.engineMs}ms`} label="Engine" color="var(--color-amber)" />
          </div>
        </div>
        {/* AETHERIS dogfood badge */}
        <div className="absolute top-3 right-4 flex items-center gap-1.5 text-[9px] text-[var(--color-green)] uppercase tracking-widest font-semibold">
          <span className="relative flex h-2 w-2">
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-[var(--color-green)] opacity-75" />
            <span className="relative inline-flex rounded-full h-2 w-2 bg-[var(--color-green)]" />
          </span>
          Kocaeli BB — Live
        </div>
      </div>

      {/* Main 2-col: Transpile demo + Pipeline */}
      <div className="grid grid-cols-1 lg:grid-cols-5 gap-4">
        {/* Left: Transpile animation (3 col) */}
        <div className="lg:col-span-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
          <div className="flex items-center justify-between px-4 py-2 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
            <span className="text-xs font-semibold text-[var(--color-accent)] tracking-wider">UNIQL TRANSPILER</span>
            <div className="flex items-center gap-3">
              <span className="text-[10px] text-[var(--color-text-dim)] font-mono">browser WASM — zero server</span>
              {!!wasm && <span className="text-[10px] px-1.5 py-0.5 rounded bg-[var(--color-green-dim)] text-[var(--color-green)] font-semibold">WASM READY</span>}
            </div>
          </div>
          {/* Query editor with syntax highlighting effect */}
          <div className="p-4 border-b border-[var(--color-border)] bg-[var(--color-surface)]/50">
            <div className="text-[10px] text-[var(--color-text-dim)] mb-1 font-mono">uniql://</div>
            <pre className="font-mono text-sm leading-relaxed min-h-[100px]">
              <code>{typedQuery.split('\n').map((line, i) => {
                // Simple syntax coloring
                const colored = line
                  .replace(/(SHOW|FROM|WHERE|AND|WITHIN|COMPUTE|GROUP BY)/g, '<kw>$1</kw>')
                  .replace(/(".*?")/g, '<str>$1</str>');
                return (
                  <span key={i}>
                    {colored.split(/(<kw>.*?<\/kw>|<str>.*?<\/str>)/).map((part, j) => {
                      if (part.startsWith('<kw>')) return <span key={j} className="text-[var(--color-accent)]">{part.replace(/<\/?kw>/g, '')}</span>;
                      if (part.startsWith('<str>')) return <span key={j} className="text-[var(--color-green)]">{part.replace(/<\/?str>/g, '')}</span>;
                      return <span key={j} className="text-[var(--color-text)]">{part}</span>;
                    })}
                    {'\n'}
                  </span>
                );
              })}</code>
              <span className="inline-block w-[2px] h-[1em] bg-[var(--color-accent)] ml-0.5 align-middle" style={{ animation: 'typing-cursor 1s infinite' }} />
            </pre>
          </div>
          {/* 3 transpile outputs */}
          {typingDone && !!wasm && (
            <div className="grid grid-cols-3 divide-x divide-[var(--color-border)]">
              {transpileTargets.map((t, i) => {
                const result = transpile(heroQuery, t.backend);
                return (
                  <div key={t.backend} className={`p-3 transition-all duration-500 ${i === activeTarget ? 'bg-[var(--color-accent)]/8' : ''}`}>
                    <div className="flex items-center gap-2 mb-1">
                      <span className={`text-[10px] font-semibold uppercase tracking-wider ${i === activeTarget ? 'text-[var(--color-accent)]' : 'text-[var(--color-text-dim)]'}`}>{t.label}</span>
                      {i === activeTarget && <span className="w-1.5 h-1.5 rounded-full bg-[var(--color-accent)] animate-pulse" />}
                    </div>
                    <code className={`text-[11px] font-mono break-all leading-relaxed ${i === activeTarget ? 'text-[var(--color-cyan)]' : 'text-[var(--color-text-dim)]'}`}>{result}</code>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* Right: Pipeline + Engine status (2 col) */}
        <div className="lg:col-span-2 space-y-4">
          {/* Pipeline layers */}
          <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4">
            <div className="flex items-center justify-between mb-3">
              <span className="text-xs font-semibold text-[var(--color-text-dim)] uppercase tracking-wider">12-Layer Compiler Pipeline</span>
              <span className="text-[10px] text-[var(--color-accent)] font-mono">&lt;1ms</span>
            </div>
            <div className="grid grid-cols-4 gap-1.5">
              {pipelineLayers.map((name, i) => (
                <div
                  key={name}
                  className="rounded px-2 py-1.5 text-[10px] font-semibold text-center border transition-all hover:scale-105"
                  style={{ background: `${layerColors[i]}18`, borderColor: `${layerColors[i]}40`, color: layerColors[i] }}
                >
                  {name}
                </div>
              ))}
            </div>
            {/* Flow arrow */}
            <div className="flex items-center gap-1 mt-3 text-[10px] text-[var(--color-text-dim)]">
              <span>Input</span>
              <div className="flex-1 h-[1px] bg-gradient-to-r from-[var(--color-accent)] via-[var(--color-cyan)] to-[var(--color-green)]" />
              <span>Output</span>
            </div>
          </div>

          {/* Engine status */}
          <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4">
            <div className="flex items-center justify-between mb-3">
              <span className="text-xs font-semibold text-[var(--color-text-dim)] uppercase tracking-wider">Engine Status</span>
              <StatusBadge ok={engineOk} label={engineOk ? `v${engine?.version}` : 'OFFLINE'} />
            </div>
            {engine?.backends.map((b) => (
              <div key={b.name} className="flex items-center justify-between py-2 border-t border-[var(--color-border)]">
                <div className="flex items-center gap-2">
                  <span className={`w-2 h-2 rounded-full ${b.status === 'reachable' ? 'bg-[var(--color-green)]' : 'bg-[var(--color-red)]'}`} />
                  <span className="text-xs text-[var(--color-text)] font-medium">{b.name}</span>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-[10px] text-[var(--color-text-dim)] font-mono">{b.type}</span>
                  <span className={`text-[9px] px-1.5 py-0.5 rounded font-semibold ${b.status === 'reachable' ? 'bg-[var(--color-green-dim)] text-[var(--color-green)]' : 'bg-[var(--color-red-dim)] text-[var(--color-red)]'}`}>
                    {b.status === 'reachable' ? 'UP' : 'DOWN'}
                  </span>
                </div>
              </div>
            ))}
          </div>

          {/* Tech stack */}
          <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4">
            <div className="text-xs font-semibold text-[var(--color-text-dim)] uppercase tracking-wider mb-2">Architecture</div>
            <div className="space-y-1.5 text-[11px]">
              {[
                { label: 'Engine', value: 'Rust + Tokio', color: 'var(--color-accent)' },
                { label: 'Browser', value: 'WASM (wasm-bindgen)', color: 'var(--color-cyan)' },
                { label: 'Metrics', value: 'VictoriaMetrics', color: 'var(--color-green)' },
                { label: 'Logs', value: 'VictoriaLogs', color: 'var(--color-amber)' },
                { label: 'Platform', value: 'AETHERIS (6 modules)', color: 'var(--color-text)' },
              ].map(r => (
                <div key={r.label} className="flex items-center justify-between">
                  <span className="text-[var(--color-text-dim)]">{r.label}</span>
                  <span className="font-mono" style={{ color: r.color }}>{r.value}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* Bottom row: quick action cards */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
        <ActionCard
          icon={<span className="relative flex h-3 w-3"><span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-[var(--color-green)] opacity-75" /><span className="relative inline-flex rounded-full h-3 w-3 bg-[var(--color-green)]" /></span>}
          title="AETHERIS Live"
          desc="721 SNMP cihazı, 370 VM — gerçek zamanlı"
          color="var(--color-green)"
          onClick={() => setTab('live')}
        />
        <ActionCard icon="{}" title="Transpile" desc="WASM ile anında UNIQL → PromQL/LogQL" color="var(--color-accent)" onClick={() => setTab('transpile')} />
        <ActionCard icon="?" title="Investigate" desc="Alert → 3 paralel sorgu → root cause" color="var(--color-amber)" onClick={() => setTab('investigate')} />
        <ComparisonCard />
      </div>
    </div>
  );
}

function LiveStat({ value, label, color, pulse }: { value: string; label: string; color: string; pulse?: boolean }) {
  return (
    <div className="text-center">
      <div className="flex items-center justify-center gap-1.5">
        {pulse && <span className="w-1.5 h-1.5 rounded-full animate-pulse" style={{ background: color }} />}
        <span className="text-xl font-bold font-mono" style={{ color }}>{value}</span>
      </div>
      <div className="text-[10px] text-[var(--color-text-dim)] uppercase tracking-wider">{label}</div>
    </div>
  );
}

function StatusBadge({ ok, label }: { ok: boolean; label: string }) {
  return (
    <span className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-[10px] font-semibold ${
      ok ? 'bg-[var(--color-green-dim)] text-[var(--color-green)]' : 'bg-[var(--color-red-dim)] text-[var(--color-red)]'
    }`}>
      <span className={`w-1.5 h-1.5 rounded-full ${ok ? 'bg-[var(--color-green)]' : 'bg-[var(--color-red)]'}`} />
      {label}
    </span>
  );
}

function ActionCard({ icon, title, desc, color, onClick }: { icon: string | React.ReactElement; title: string; desc: string; color: string; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4 text-left hover:border-[var(--color-border-bright)] transition-all cursor-pointer group"
    >
      <div className="w-8 h-8 rounded-lg flex items-center justify-center mb-3 text-sm font-mono font-bold"
        style={{ background: `${color}18`, color, border: `1px solid ${color}40` }}>
        {typeof icon === 'string' ? icon : icon}
      </div>
      <div className="text-sm font-semibold text-[var(--color-text-bright)] group-hover:text-white">{title}</div>
      <div className="text-[11px] text-[var(--color-text-dim)] mt-0.5">{desc}</div>
    </button>
  );
}

function ComparisonCard() {
  const rows = [
    { label: 'Query Language', before: '6+ different', after: '1 UNIQL' },
    { label: 'RCA', before: 'Manual', after: 'Auto 3-query' },
    { label: 'Vendor Lock', before: 'PromQL/SPL/KQL', after: 'Neutral' },
    { label: 'Parse Speed', before: '10-50ms', after: '<1ms WASM' },
  ];
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4">
      <div className="text-sm font-semibold text-[var(--color-text-bright)] mb-2">Before → After</div>
      <table className="w-full text-[10px]">
        <thead>
          <tr>
            <th className="text-left text-[var(--color-text-dim)] font-normal pb-1"></th>
            <th className="text-center text-[var(--color-red)] font-normal pb-1">Today</th>
            <th className="text-center text-[var(--color-green)] font-normal pb-1">UNIQL</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.label} className="border-t border-[var(--color-border)]">
              <td className="py-1 text-[var(--color-text-dim)]">{r.label}</td>
              <td className="py-1 text-center text-[var(--color-red)]/70">{r.before}</td>
              <td className="py-1 text-center text-[var(--color-green)]">{r.after}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
