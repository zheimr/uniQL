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
  AND clustername = "DELLR750_Cluster"
WITHIN last 1h`;

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

  useEffect(() => {
    let i = 0;
    setTypedQuery('');
    setTypingDone(false);
    const interval = setInterval(() => {
      if (i < heroQuery.length) { setTypedQuery(heroQuery.slice(0, i + 1)); i++; }
      else { clearInterval(interval); setTypingDone(true); }
    }, 18);
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    if (!typingDone) return;
    const i = setInterval(() => setActiveTarget((p) => (p + 1) % 3), 2500);
    return () => clearInterval(i);
  }, [typingDone]);

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
          snmpDevices: getVal(snmpRes), vms: getVal(vmRes),
          services: `${getVal(svcUpRes)}/${getVal(svcTotalRes)}`,
          engineMs: getMs(svcUpRes),
        });
      } catch { /* graceful fail */ }
    };
    fetchStats();
  }, []);

  const targets = [
    { backend: 'promql', label: 'PromQL', sub: 'VictoriaMetrics' },
    { backend: 'logql', label: 'LogQL', sub: 'Grafana Loki' },
    { backend: 'logsql', label: 'LogsQL', sub: 'VictoriaLogs' },
  ];

  const engineOk = engine?.status === 'ok';

  return (
    <div className="animate-fade-in">
      {/* ─── Hero ─────────────────────────────────────────────────────── */}
      <section className="py-16 text-center">
        <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-[var(--color-accent)]/30 bg-[var(--color-accent)]/8 text-[11px] text-[var(--color-accent)] font-medium mb-6">
          <span className="w-1.5 h-1.5 rounded-full bg-[var(--color-accent)]" />
          Open Source Unified Observability Query Language
        </div>
        <h1 className="text-4xl lg:text-5xl font-bold text-[var(--color-text-bright)] mb-4 leading-tight">
          Write Once,<br />
          <span className="bg-gradient-to-r from-[var(--color-accent)] to-[var(--color-cyan)] bg-clip-text text-transparent">Query Everything</span>
        </h1>
        <p className="text-lg text-[var(--color-text-dim)] max-w-2xl mx-auto mb-8">
          Single syntax for metrics, logs, and traces. Transpiles to PromQL, LogQL, and LogsQL.
          No vendor lock-in. No data migration.
        </p>
        <div className="flex items-center justify-center gap-3 mb-12">
          <button
            onClick={() => setTab('transpile')}
            className="px-5 py-2.5 rounded-lg bg-[var(--color-accent)] text-white text-sm font-semibold hover:opacity-90 transition-opacity cursor-pointer"
          >
            Try Playground
          </button>
          <button
            onClick={() => setTab('live')}
            className="px-5 py-2.5 rounded-lg border border-[var(--color-border)] text-[var(--color-text)] text-sm font-semibold hover:bg-[var(--color-surface-2)] transition-all cursor-pointer"
          >
            Live Demo
          </button>
          <a
            href="https://github.com/zheimr/uniQL"
            target="_blank"
            rel="noopener noreferrer"
            className="px-5 py-2.5 rounded-lg border border-[var(--color-border)] text-[var(--color-text-dim)] text-sm font-semibold hover:text-[var(--color-text)] hover:bg-[var(--color-surface-2)] transition-all"
          >
            GitHub
          </a>
        </div>

        {/* Live stats */}
        <div className="flex items-center justify-center gap-8 text-center">
          <Stat value={String(stats.snmpDevices)} label="SNMP Devices" />
          <Stat value={String(stats.vms)} label="Virtual Machines" />
          <Stat value={stats.services} label="Services" />
          <Stat value={`${stats.engineMs}ms`} label="Engine Latency" />
          <Stat value="467" label="Tests" />
          <Stat value="83%" label="Coverage" />
        </div>
      </section>

      {/* ─── Transpiler Demo ──────────────────────────────────────────── */}
      <section className="mb-16">
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden shadow-2xl shadow-black/20">
          {/* Terminal header */}
          <div className="flex items-center gap-2 px-4 py-3 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
            <span className="w-3 h-3 rounded-full bg-[#ff5f57]" />
            <span className="w-3 h-3 rounded-full bg-[#febc2e]" />
            <span className="w-3 h-3 rounded-full bg-[#28c840]" />
            <span className="ml-3 text-[11px] text-[var(--color-text-dim)] font-mono">uniql-transpiler</span>
            {!!wasm && <span className="ml-auto text-[9px] px-2 py-0.5 rounded-full bg-[var(--color-green)]/10 text-[var(--color-green)]">WASM Ready</span>}
          </div>
          {/* Query */}
          <div className="p-5 border-b border-[var(--color-border)]">
            <div className="text-[10px] text-[var(--color-text-dim)] font-mono mb-2 uppercase tracking-wider">UniQL Input</div>
            <pre className="font-mono text-sm leading-relaxed">
              {typedQuery.split('\n').map((line, i) => (
                <span key={i}>
                  {line.split(/(\b(?:SHOW|FROM|WHERE|AND|WITHIN|COMPUTE|GROUP BY|HAVING|CORRELATE|PARSE|DEFINE)\b|"[^"]*")/g).map((part, j) => {
                    if (/^(SHOW|FROM|WHERE|AND|WITHIN|COMPUTE|GROUP BY|HAVING|CORRELATE|PARSE|DEFINE)$/.test(part))
                      return <span key={j} className="text-[var(--color-accent)]">{part}</span>;
                    if (part.startsWith('"'))
                      return <span key={j} className="text-[var(--color-green)]">{part}</span>;
                    return <span key={j} className="text-[var(--color-text)]">{part}</span>;
                  })}
                  {'\n'}
                </span>
              ))}
              <span className="inline-block w-[2px] h-[1em] bg-[var(--color-accent)] ml-0.5 align-middle" style={{ animation: 'typing-cursor 1s infinite' }} />
            </pre>
          </div>
          {/* 3 outputs */}
          {typingDone && !!wasm && (
            <div className="grid grid-cols-3 divide-x divide-[var(--color-border)]">
              {targets.map((t, i) => {
                const result = transpile(heroQuery, t.backend);
                const isActive = i === activeTarget;
                return (
                  <div key={t.backend} className={`p-4 transition-all duration-500 ${isActive ? 'bg-[var(--color-accent)]/5' : ''}`}>
                    <div className="flex items-center gap-2 mb-2">
                      <span className={`text-[11px] font-bold tracking-wider ${isActive ? 'text-[var(--color-accent)]' : 'text-[var(--color-text-dim)]'}`}>{t.label}</span>
                      <span className="text-[9px] text-[var(--color-text-dim)]">{t.sub}</span>
                      {isActive && <span className="w-1.5 h-1.5 rounded-full bg-[var(--color-accent)] animate-pulse ml-auto" />}
                    </div>
                    <code className={`text-[11px] font-mono break-all leading-relaxed ${isActive ? 'text-[var(--color-cyan)]' : 'text-[var(--color-text-dim)]'}`}>{result}</code>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </section>

      {/* ─── Features ─────────────────────────────────────────────────── */}
      <section className="mb-16">
        <h2 className="text-2xl font-bold text-[var(--color-text-bright)] text-center mb-2">How It Works</h2>
        <p className="text-[var(--color-text-dim)] text-center mb-8">12-layer compiler pipeline, sub-millisecond parse time</p>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
          <FeatureCard icon="1" title="Parse" desc="Handwritten lexer + Pratt parser. 64-level depth limit, 64KB query limit." color="var(--color-accent)" />
          <FeatureCard icon="2" title="Bind + Normalize" desc="Identifier resolution, stream label classification, duration extraction, OR flattening." color="var(--color-cyan)" />
          <FeatureCard icon="3" title="Transpile" desc="AST → native query. PromQL for metrics, LogsQL for VictoriaLogs, LogQL for Loki." color="var(--color-green)" />
          <FeatureCard icon="4" title="Execute + Correlate" desc="Parallel backend execution, hash-partitioned time-window join, 10K cardinality limit." color="var(--color-amber)" />
        </div>
      </section>

      {/* ─── Language Features ─────────────────────────────────────────── */}
      <section className="mb-16">
        <h2 className="text-2xl font-bold text-[var(--color-text-bright)] text-center mb-8">Language Features</h2>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
          <LangFeature kw="FROM" example='FROM metrics, logs CORRELATE ON host' desc="Multi-backend, multi-signal, with aliases" />
          <LangFeature kw="WHERE" example='WHERE service =~ "api.*" AND level = "error"' desc="Equality, regex, IN, CONTAINS, STARTS WITH" />
          <LangFeature kw="WITHIN" example='WITHIN last 1h' desc="Relative, absolute, today, this_week" />
          <LangFeature kw="COMPUTE" example='COMPUTE rate(value, 5m) GROUP BY service' desc="rate, avg, sum, min, max, p50-p99, count" />
          <LangFeature kw="PARSE" example='PARSE json' desc="json, logfmt, pattern, regexp" />
          <LangFeature kw="CORRELATE" example='CORRELATE ON host WITHIN 60s' desc="Cross-signal join with time window" />
          <LangFeature kw="SHOW" example='SHOW table' desc="timeseries, table, count, timeline, heatmap" />
          <LangFeature kw="DEFINE" example='DEFINE high_cpu = __name__ = "cpu_usage"' desc="Reusable query macros" />
          <LangFeature kw="NATIVE" example='NATIVE("promql", "rate(up[5m])")' desc="Backend passthrough for power users" />
        </div>
      </section>

      {/* ─── Architecture ─────────────────────────────────────────────── */}
      <section className="mb-16">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
          {/* Pipeline */}
          <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-6">
            <h3 className="text-sm font-semibold text-[var(--color-text-dim)] uppercase tracking-wider mb-4">12-Layer Pipeline</h3>
            <div className="grid grid-cols-4 gap-1.5">
              {['Lexer', 'Parser', 'Expander', 'Binder', 'Validator', 'Normalizer', 'Planner', 'Transpiler', 'Executor', 'Normalizer²', 'Correlator', 'Formatter'].map((name, i) => {
                const colors = ['#7c5cfc','#8b6cf7','#9b7cf2','#ab8ced','#bb9ce8','#cb8ce3','#e09c5e','#e0b640','#90d050','#50c8a0','#40c8d0','#50b8e0'];
                return (
                  <div key={name} className="rounded-md px-2 py-2 text-[10px] font-semibold text-center border" style={{ background: `${colors[i]}15`, borderColor: `${colors[i]}35`, color: colors[i] }}>
                    {name}
                  </div>
                );
              })}
            </div>
            <div className="flex items-center gap-1 mt-3 text-[10px] text-[var(--color-text-dim)]">
              <span>Input</span>
              <div className="flex-1 h-[1px] bg-gradient-to-r from-[var(--color-accent)] via-[var(--color-cyan)] to-[var(--color-green)]" />
              <span>Output</span>
            </div>
          </div>

          {/* Engine + Tech */}
          <div className="space-y-4">
            <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-6">
              <div className="flex items-center justify-between mb-4">
                <h3 className="text-sm font-semibold text-[var(--color-text-dim)] uppercase tracking-wider">Engine Status</h3>
                {engineOk && <span className="text-[10px] px-2 py-0.5 rounded-full bg-[var(--color-green)]/10 text-[var(--color-green)] font-semibold">v{engine?.version}</span>}
              </div>
              {engine?.backends.map((b) => (
                <div key={b.name} className="flex items-center justify-between py-2 border-t border-[var(--color-border)]">
                  <span className="text-xs text-[var(--color-text)] font-medium">{b.name}</span>
                  <div className="flex items-center gap-2">
                    <span className="text-[10px] text-[var(--color-text-dim)] font-mono">{b.type}</span>
                    <span className={`w-2 h-2 rounded-full ${b.status === 'reachable' ? 'bg-[var(--color-green)]' : 'bg-[var(--color-red)]'}`} />
                  </div>
                </div>
              ))}
            </div>
            <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-6">
              <h3 className="text-sm font-semibold text-[var(--color-text-dim)] uppercase tracking-wider mb-3">Built With</h3>
              <div className="grid grid-cols-2 gap-2 text-[11px]">
                {[
                  ['Engine', 'Rust + Axum + Tokio'],
                  ['Browser', 'WASM (7 functions)'],
                  ['Metrics', 'VictoriaMetrics'],
                  ['Logs', 'VictoriaLogs'],
                  ['Tests', '467 tests, 83% coverage'],
                  ['Demo', 'React 18 + Vite'],
                ].map(([k, v]) => (
                  <div key={k} className="flex items-center justify-between py-1">
                    <span className="text-[var(--color-text-dim)]">{k}</span>
                    <span className="text-[var(--color-text)] font-mono">{v}</span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* ─── CTA ──────────────────────────────────────────────────────── */}
      <section className="text-center py-12 mb-8">
        <h2 className="text-2xl font-bold text-[var(--color-text-bright)] mb-3">Ready to try?</h2>
        <p className="text-[var(--color-text-dim)] mb-6">Open source. Zero dependencies. Runs anywhere.</p>
        <div className="inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-[var(--color-surface-3)] border border-[var(--color-border)] font-mono text-sm text-[var(--color-text)]">
          <span className="text-[var(--color-text-dim)]">$</span>
          docker compose up -d
        </div>
      </section>

      {/* ─── Footer ───────────────────────────────────────────────────── */}
      <footer className="border-t border-[var(--color-border)] py-6 text-center text-[11px] text-[var(--color-text-dim)]">
        <div className="flex items-center justify-center gap-4">
          <span>UniQL v0.3.0</span>
          <span>467 Tests</span>
          <span>83% Coverage</span>
          <span>MIT License</span>
          <a href="https://github.com/zheimr/uniQL" target="_blank" rel="noopener noreferrer" className="text-[var(--color-accent)] hover:underline">GitHub</a>
        </div>
      </footer>
    </div>
  );
}

function Stat({ value, label }: { value: string; label: string }) {
  return (
    <div>
      <div className="text-xl font-bold font-mono text-[var(--color-text-bright)]">{value}</div>
      <div className="text-[10px] text-[var(--color-text-dim)] uppercase tracking-wider">{label}</div>
    </div>
  );
}

function FeatureCard({ icon, title, desc, color }: { icon: string; title: string; desc: string; color: string }) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-5">
      <div className="w-8 h-8 rounded-lg flex items-center justify-center text-sm font-bold mb-3" style={{ background: `${color}15`, color, border: `1px solid ${color}35` }}>
        {icon}
      </div>
      <h3 className="text-sm font-semibold text-[var(--color-text-bright)] mb-1">{title}</h3>
      <p className="text-[11px] text-[var(--color-text-dim)] leading-relaxed">{desc}</p>
    </div>
  );
}

function LangFeature({ kw, example, desc }: { kw: string; example: string; desc: string }) {
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-3">
      <span className="text-[10px] font-bold text-[var(--color-accent)] tracking-wider">{kw}</span>
      <div className="font-mono text-[11px] text-[var(--color-cyan)] mt-1 break-all">{example}</div>
      <div className="text-[10px] text-[var(--color-text-dim)] mt-1">{desc}</div>
    </div>
  );
}
