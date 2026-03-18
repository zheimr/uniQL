import { useState, useEffect, useRef } from 'react';
import type { EngineHealth, TabId } from '../App';

const ENGINE_URL = `http://${window.location.hostname}:9090`;

interface Props {
  engine: EngineHealth | null;
  wasm: unknown;
  transpile: (q: string, b: string) => string | null;
  setTab: (t: TabId) => void;
}

// ─── Benchmark data ──────────────────────────────────────────────

interface BenchResult { query: string; parseUs: number; transpileUs: number; executeMs: number; results: number }

const DEMO_QUERIES = [
  { query: 'FROM metrics WHERE __name__ = "up"', label: '14 services' },
  { query: 'SHOW timeseries FROM victoria WHERE __name__ = "snmpv2_device_up"', label: '728 SNMP devices' },
  { query: 'SHOW table FROM vlogs WHERE job = "fortigate" WITHIN last 1m', label: 'FortiGate logs' },
  { query: 'FROM metrics WHERE __name__ = "vsphere_host_cpu_usage_average"', label: 'ESXi CPU' },
];

export default function OverviewTab({ engine, wasm, transpile, setTab }: Props) {
  const [typedIndex, setTypedIndex] = useState(0);
  const [typed, setTyped] = useState('');
  const [benchmarks, setBenchmarks] = useState<BenchResult[]>([]);
  const [benchRunning, setBenchRunning] = useState(false);
  const [showComparison, setShowComparison] = useState(false);
  const heroRef = useRef<HTMLDivElement>(null);

  // Typing animation cycling through queries
  useEffect(() => {
    const queries = [
      'FROM metrics WHERE __name__ = "up" WITHIN last 1h',
      'SHOW table FROM vlogs WHERE job = "fortigate"',
      'FROM metrics, logs CORRELATE ON host WITHIN 60s',
      'COMPUTE rate(value, 5m) GROUP BY service',
    ];
    let charIdx = 0;
    let deleting = false;
    const q = queries[typedIndex % queries.length];

    const interval = setInterval(() => {
      if (!deleting) {
        charIdx++;
        setTyped(q.slice(0, charIdx));
        if (charIdx >= q.length) {
          setTimeout(() => { deleting = true; }, 2000);
        }
      } else {
        charIdx--;
        setTyped(q.slice(0, charIdx));
        if (charIdx <= 0) {
          deleting = false;
          setTypedIndex(prev => prev + 1);
        }
      }
    }, deleting ? 25 : 40);
    return () => clearInterval(interval);
  }, [typedIndex]);

  // Live benchmark
  const runBenchmark = async () => {
    setBenchRunning(true);
    setBenchmarks([]);
    const results: BenchResult[] = [];
    for (const dq of DEMO_QUERIES) {
      try {
        const resp = await fetch(`${ENGINE_URL}/v1/query`, {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ query: dq.query, limit: 5 }),
        });
        const json = await resp.json();
        const m = json.metadata || {};
        const count = json.data?.data?.result?.length ?? json.data?.rows?.length ?? json.data?.result?.length ?? 0;
        results.push({
          query: dq.query, parseUs: m.parse_time_us || 0,
          transpileUs: m.transpile_time_us || 0, executeMs: m.total_time_ms || 0,
          results: count,
        });
        setBenchmarks([...results]);
      } catch { results.push({ query: dq.query, parseUs: 0, transpileUs: 0, executeMs: 0, results: 0 }); }
    }
    setBenchRunning(false);
  };

  useEffect(() => { setTimeout(runBenchmark, 500); }, []);

  // Transpile demo
  const demoQuery = 'FROM metrics WHERE __name__ = "vsphere_host_cpu_usage_average" AND clustername = "DELLR750_Cluster" WITHIN last 1h';
  const promql = wasm ? transpile(demoQuery, 'promql') : null;
  const logql = wasm ? transpile(demoQuery, 'logql') : null;
  const logsql = wasm ? transpile(demoQuery, 'logsql') : null;

  const engineOk = engine?.status === 'ok';

  return (
    <div className="animate-fade-in">
      {/* ═══ HERO ═══ */}
      <section ref={heroRef} className="py-20 text-center relative overflow-hidden">
        {/* Background grid */}
        <div className="absolute inset-0 opacity-[0.03]" style={{ backgroundImage: 'radial-gradient(circle, var(--color-accent) 1px, transparent 1px)', backgroundSize: '30px 30px' }} />

        <div className="relative z-10">
          <div className="inline-flex items-center gap-2 px-4 py-1.5 rounded-full border border-[var(--color-accent)]/20 bg-[var(--color-accent)]/5 text-[12px] text-[var(--color-accent)] font-medium mb-8">
            <span className="relative flex h-2 w-2"><span className="animate-ping absolute h-full w-full rounded-full bg-[var(--color-green)] opacity-75" /><span className="relative rounded-full h-2 w-2 bg-[var(--color-green)]" /></span>
            {engineOk ? `Engine v${engine?.version} — ${engine?.backends.length} backends online` : 'Open Source Query Language'}
          </div>

          <h1 className="text-5xl lg:text-6xl font-extrabold text-[var(--color-text-bright)] mb-6 leading-[1.1]">
            One query.<br />
            <span className="bg-gradient-to-r from-[var(--color-accent)] via-[var(--color-cyan)] to-[var(--color-green)] bg-clip-text text-transparent">Every backend.</span>
          </h1>

          <p className="text-lg text-[var(--color-text-dim)] max-w-xl mx-auto mb-4">
            Stop learning PromQL, LogQL, and LogsQL separately. Write UniQL once — it transpiles to all three in sub-millisecond.
          </p>

          {/* Live typing demo */}
          <div className="inline-block rounded-lg bg-[var(--color-surface-2)] border border-[var(--color-border)] px-5 py-3 font-mono text-sm text-left mb-8 min-w-[500px]">
            <span className="text-[var(--color-text-dim)]">uniql&gt; </span>
            <span className="text-[var(--color-accent)]">{typed}</span>
            <span className="inline-block w-[2px] h-[14px] bg-[var(--color-accent)] ml-0.5 align-middle" style={{ animation: 'typing-cursor 1s infinite' }} />
          </div>

          <div className="flex items-center justify-center gap-3 mb-12">
            <button onClick={() => setTab('transpile')} className="px-6 py-3 rounded-lg bg-gradient-to-r from-[var(--color-accent)] to-[var(--color-cyan)] text-white text-sm font-bold hover:opacity-90 transition-opacity cursor-pointer shadow-lg shadow-[var(--color-accent)]/25">
              Open Playground
            </button>
            <button onClick={() => setTab('live')} className="px-6 py-3 rounded-lg border border-[var(--color-border)] text-[var(--color-text)] text-sm font-semibold hover:bg-[var(--color-surface-2)] transition-all cursor-pointer">
              Live Demo
            </button>
            <a href="https://github.com/zheimr/uniQL" target="_blank" rel="noopener noreferrer" className="px-6 py-3 rounded-lg border border-[var(--color-border)] text-[var(--color-text-dim)] text-sm font-semibold hover:text-[var(--color-text)] transition-all flex items-center gap-2">
              <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>
              GitHub
            </a>
          </div>

          {/* ═══ LIVE BENCHMARK ═══ */}
          <div className="max-w-3xl mx-auto">
            <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden text-left">
              <div className="flex items-center justify-between px-4 py-2.5 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
                <span className="text-[11px] font-semibold text-[var(--color-text-dim)] uppercase tracking-wider">Live Benchmark — Real Data</span>
                <button onClick={runBenchmark} disabled={benchRunning} className="text-[10px] px-2 py-0.5 rounded border border-[var(--color-accent)]/30 text-[var(--color-accent)] cursor-pointer hover:bg-[var(--color-accent)]/10 disabled:opacity-40">
                  {benchRunning ? 'Running...' : 'Re-run'}
                </button>
              </div>
              <div className="divide-y divide-[var(--color-border)]/30">
                {DEMO_QUERIES.map((dq, i) => {
                  const b = benchmarks[i];
                  return (
                    <div key={i} className="px-4 py-2.5 flex items-center gap-3">
                      <div className="flex-1 min-w-0">
                        <div className="text-[11px] font-mono text-[var(--color-accent)] truncate">{dq.query}</div>
                        <div className="text-[9px] text-[var(--color-text-dim)]">{dq.label}</div>
                      </div>
                      {b ? (
                        <div className="flex items-center gap-4 shrink-0 text-[10px] font-mono">
                          <span className="text-[var(--color-cyan)]">{b.parseUs}us parse</span>
                          <span className="text-[var(--color-green)]">{b.transpileUs}us transpile</span>
                          <span className="text-[var(--color-amber)]">{b.executeMs}ms total</span>
                          <span className="text-[var(--color-text)]">{b.results} results</span>
                        </div>
                      ) : (
                        <span className="text-[10px] text-[var(--color-text-dim)]">{benchRunning ? '...' : '--'}</span>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* ═══ THE PROBLEM ═══ */}
      <section className="mb-16">
        <h2 className="text-2xl font-bold text-[var(--color-text-bright)] text-center mb-3">The Problem</h2>
        <p className="text-[var(--color-text-dim)] text-center max-w-2xl mx-auto mb-8">Every observability backend speaks a different language. Your team learns 3+ query syntaxes and can't correlate across signals.</p>
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 max-w-4xl mx-auto">
          {/* Before */}
          <div className="rounded-xl border border-[var(--color-red)]/20 bg-[var(--color-red)]/5 p-6">
            <div className="text-sm font-bold text-[var(--color-red)] mb-4">Without UniQL</div>
            <div className="space-y-3 text-[12px] font-mono">
              <div className="rounded bg-[var(--color-surface)] p-2 border border-[var(--color-border)]">
                <span className="text-[var(--color-text-dim)]">Metrics:</span> <span className="text-[var(--color-red)]">rate(http_requests_total{'{'}job="api"{'}'}[5m])</span>
              </div>
              <div className="rounded bg-[var(--color-surface)] p-2 border border-[var(--color-border)]">
                <span className="text-[var(--color-text-dim)]">Logs:</span> <span className="text-[var(--color-red)]">{'{'}job="api"{'}'} |= "error" | json | level="error"</span>
              </div>
              <div className="rounded bg-[var(--color-surface)] p-2 border border-[var(--color-border)]">
                <span className="text-[var(--color-text-dim)]">VLogs:</span> <span className="text-[var(--color-red)]">_stream:{'{'}job="api"{'}'} error | stats count()</span>
              </div>
            </div>
            <div className="mt-3 text-[11px] text-[var(--color-red)]/70">3 languages. 3 syntaxes. No correlation.</div>
          </div>
          {/* After */}
          <div className="rounded-xl border border-[var(--color-green)]/20 bg-[var(--color-green)]/5 p-6">
            <div className="text-sm font-bold text-[var(--color-green)] mb-4">With UniQL</div>
            <div className="space-y-3 text-[12px] font-mono">
              <div className="rounded bg-[var(--color-surface)] p-2 border border-[var(--color-green)]/20">
                <span className="text-[var(--color-accent)]">FROM</span> <span className="text-[var(--color-text)]">metrics</span> <span className="text-[var(--color-accent)]">WHERE</span> <span className="text-[var(--color-text)]">job =</span> <span className="text-[var(--color-green)]">"api"</span> <span className="text-[var(--color-accent)]">COMPUTE</span> <span className="text-[var(--color-cyan)]">rate</span><span className="text-[var(--color-text)]">(value, 5m)</span>
              </div>
              <div className="rounded bg-[var(--color-surface)] p-2 border border-[var(--color-green)]/20">
                <span className="text-[var(--color-accent)]">FROM</span> <span className="text-[var(--color-text)]">logs</span> <span className="text-[var(--color-accent)]">WHERE</span> <span className="text-[var(--color-text)]">job =</span> <span className="text-[var(--color-green)]">"api"</span> <span className="text-[var(--color-accent)]">AND</span> <span className="text-[var(--color-text)]">message</span> <span className="text-[var(--color-accent)]">CONTAINS</span> <span className="text-[var(--color-green)]">"error"</span>
              </div>
              <div className="rounded bg-[var(--color-surface)] p-2 border border-[var(--color-green)]/20">
                <span className="text-[var(--color-accent)]">FROM</span> <span className="text-[var(--color-text)]">metrics, logs</span> <span className="text-[var(--color-accent)]">CORRELATE ON</span> <span className="text-[var(--color-text)]">host</span> <span className="text-[var(--color-accent)]">WITHIN</span> <span className="text-[var(--color-amber)]">60s</span>
              </div>
            </div>
            <div className="mt-3 text-[11px] text-[var(--color-green)]/70">1 language. All backends. Cross-signal correlation.</div>
          </div>
        </div>
      </section>

      {/* ═══ TRANSPILE PROOF ═══ */}
      <section className="mb-16">
        <h2 className="text-2xl font-bold text-[var(--color-text-bright)] text-center mb-3">One Query, Three Outputs</h2>
        <p className="text-[var(--color-text-dim)] text-center mb-8">Same UniQL transpiles to native syntax for each backend — in the browser via WASM</p>
        <div className="max-w-4xl mx-auto rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
          <div className="p-4 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
            <div className="text-[10px] text-[var(--color-text-dim)] uppercase tracking-wider mb-1">UniQL Input</div>
            <div className="font-mono text-[13px]">
              <span className="text-[var(--color-accent)]">FROM</span> <span className="text-[var(--color-text)]">metrics</span> <span className="text-[var(--color-accent)]">WHERE</span> <span className="text-[var(--color-text)]">__name__ =</span> <span className="text-[var(--color-green)]">"vsphere_host_cpu_usage_average"</span> <span className="text-[var(--color-accent)]">AND</span> <span className="text-[var(--color-text)]">clustername =</span> <span className="text-[var(--color-green)]">"DELLR750_Cluster"</span> <span className="text-[var(--color-accent)]">WITHIN</span> <span className="text-[var(--color-amber)]">last 1h</span>
            </div>
          </div>
          <div className="grid grid-cols-3 divide-x divide-[var(--color-border)]">
            {[
              { label: 'PromQL', sub: 'VictoriaMetrics', output: promql, color: 'var(--color-accent)' },
              { label: 'LogQL', sub: 'Grafana Loki', output: logql, color: 'var(--color-green)' },
              { label: 'LogsQL', sub: 'VictoriaLogs', output: logsql, color: 'var(--color-amber)' },
            ].map(t => (
              <div key={t.label} className="p-4">
                <div className="flex items-center gap-2 mb-2">
                  <span className="w-2 h-2 rounded-full" style={{ background: t.color }} />
                  <span className="text-[11px] font-bold" style={{ color: t.color }}>{t.label}</span>
                  <span className="text-[9px] text-[var(--color-text-dim)]">{t.sub}</span>
                </div>
                <code className="text-[11px] font-mono text-[var(--color-text)] break-all leading-relaxed">{t.output || '--'}</code>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* ═══ FEATURES ═══ */}
      <section className="mb-16">
        <h2 className="text-2xl font-bold text-[var(--color-text-bright)] text-center mb-8">Built for Production</h2>
        <div className="grid grid-cols-2 lg:grid-cols-4 gap-4 max-w-4xl mx-auto">
          {[
            { icon: '⚡', title: 'Sub-ms Parse', desc: 'Handwritten Rust lexer + Pratt parser. 12-layer pipeline.', stat: '<100us' },
            { icon: '🔒', title: 'Security', desc: 'Injection prevention, panic recovery, constant-time auth.', stat: '8 guards' },
            { icon: '🧪', title: 'Battle Tested', desc: '467 tests, 83% coverage, wiremock integration tests.', stat: '467 tests' },
            { icon: '🌐', title: 'WASM', desc: '7 browser functions. Zero server for transpilation.', stat: '< 200KB' },
            { icon: '🔗', title: 'CORRELATE', desc: 'Cross-signal join with hash-partitioned time window.', stat: '10K limit' },
            { icon: '📦', title: 'Investigation', desc: '4 built-in packs: high_cpu, link_down, error_spike, latency.', stat: '3 parallel' },
            { icon: '🔄', title: 'Retry + Health', desc: 'Auto retry on transient failures. Startup health probe.', stat: '1 retry' },
            { icon: '📖', title: 'Open Source', desc: 'MIT license. Rust + Axum + Tokio. Docker ready.', stat: 'MIT' },
          ].map(f => (
            <div key={f.title} className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-5 hover:border-[var(--color-border-bright)] transition-all">
              <div className="flex items-center justify-between mb-2">
                <span className="text-xl">{f.icon}</span>
                <span className="text-[10px] font-mono text-[var(--color-accent)]">{f.stat}</span>
              </div>
              <h3 className="text-sm font-semibold text-[var(--color-text-bright)] mb-1">{f.title}</h3>
              <p className="text-[11px] text-[var(--color-text-dim)] leading-relaxed">{f.desc}</p>
            </div>
          ))}
        </div>
      </section>

      {/* ═══ COMPARISON ═══ */}
      <section className="mb-16 max-w-4xl mx-auto">
        <button onClick={() => setShowComparison(!showComparison)} className="w-full text-center cursor-pointer group">
          <h2 className="text-2xl font-bold text-[var(--color-text-bright)] mb-2 group-hover:text-[var(--color-accent)] transition-colors">
            How does UniQL compare? {showComparison ? '▼' : '▶'}
          </h2>
        </button>
        {showComparison && (
          <div className="mt-4 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden animate-fade-in">
            <table className="w-full text-[11px]">
              <thead>
                <tr className="border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
                  <th className="text-left px-4 py-2.5 text-[var(--color-text-dim)] font-semibold">Feature</th>
                  <th className="text-center px-3 py-2.5 text-[var(--color-accent)] font-bold">UniQL</th>
                  <th className="text-center px-3 py-2.5 text-[var(--color-text-dim)]">Grafana</th>
                  <th className="text-center px-3 py-2.5 text-[var(--color-text-dim)]">SigNoz</th>
                  <th className="text-center px-3 py-2.5 text-[var(--color-text-dim)]">Datadog</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--color-border)]/30">
                {[
                  ['Unified Syntax', 'Yes — 1 language', 'No — PromQL + LogQL + TraceQL', 'Partial — SQL wrapper', 'Yes — DQL (proprietary)'],
                  ['Backend Agnostic', 'Yes — transpiles to any', 'No — tied to own stack', 'No — ClickHouse only', 'No — cloud only'],
                  ['Cross-Signal', 'CORRELATE ON field', 'Manual tab switching', 'Correlated views', 'Correlation (paid)'],
                  ['Browser Transpile', 'WASM, zero server', 'No', 'No', 'No'],
                  ['Open Source', 'MIT', 'AGPL / Cloud', 'Apache 2.0', 'Proprietary'],
                  ['Parse Speed', '<100us (Rust)', '~5ms (Go)', '~10ms (Go+SQL)', 'Unknown'],
                  ['Self-Hosted', 'Docker, 2MB RAM', 'Heavy stack', 'ClickHouse required', 'Cloud only'],
                ].map(([feature, uniql, grafana, signoz, datadog]) => (
                  <tr key={feature}>
                    <td className="px-4 py-2 text-[var(--color-text)]">{feature}</td>
                    <td className="px-3 py-2 text-center text-[var(--color-green)] font-semibold">{uniql}</td>
                    <td className="px-3 py-2 text-center text-[var(--color-text-dim)]">{grafana}</td>
                    <td className="px-3 py-2 text-center text-[var(--color-text-dim)]">{signoz}</td>
                    <td className="px-3 py-2 text-center text-[var(--color-text-dim)]">{datadog}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {/* ═══ CTA ═══ */}
      <section className="text-center py-16 mb-8">
        <h2 className="text-3xl font-bold text-[var(--color-text-bright)] mb-4">Start querying in 30 seconds</h2>
        <div className="inline-flex items-center gap-2 px-5 py-3 rounded-lg bg-[var(--color-surface-2)] border border-[var(--color-border)] font-mono text-sm text-[var(--color-text)] mb-6 shadow-xl">
          <span className="text-[var(--color-green)]">$</span>
          docker compose up -d && curl localhost:9090/health
        </div>
        <div className="flex items-center justify-center gap-6 text-[11px] text-[var(--color-text-dim)]">
          <span>467 tests</span>
          <span>83% coverage</span>
          <span>12-layer pipeline</span>
          <span>2MB RAM</span>
          <span>MIT license</span>
        </div>
      </section>

      {/* ═══ FOOTER ═══ */}
      <footer className="border-t border-[var(--color-border)] py-6 text-center text-[11px] text-[var(--color-text-dim)]">
        UniQL v0.3.0 — Unified Observability Query Language — <a href="https://github.com/zheimr/uniQL" target="_blank" rel="noopener noreferrer" className="text-[var(--color-accent)] hover:underline">GitHub</a>
      </footer>
    </div>
  );
}
