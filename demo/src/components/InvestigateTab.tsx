import { useState } from 'react';
import type { EngineHealth } from '../App';
import { investigationSteps } from '../data/scenarios';

const ENGINE_URL = `http://${window.location.hostname}:9090`;

const packs = [
  { id: 'high_cpu', label: 'High CPU', icon: 'C', defaultHost: 'r750g01.kocaeli.bel.tr' },
  { id: 'link_down', label: 'Link Down', icon: '!', defaultHost: 'CORE-SW-01' },
  { id: 'error_spike', label: 'Error Spike', icon: 'E', defaultHost: '', defaultService: 'admin-api' },
  { id: 'latency_degradation', label: 'Latency', icon: 'L', defaultHost: '', defaultService: 'admin-api' },
];

interface PackResult {
  name: string;
  query: string;
  native_query: string | null;
  status: string;
  data: unknown;
  execute_time_ms: number;
  error: string | null;
}

interface Props { engine: EngineHealth | null; }

export default function InvestigateTab({ engine }: Props) {
  const [activePack, setActivePack] = useState(packs[0]);
  const [param, setParam] = useState(packs[0].defaultHost);
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<PackResult[]>([]);
  const [totalMs, setTotalMs] = useState(0);
  const [activeStep, setActiveStep] = useState<number | null>(null);

  const engineOk = engine?.status === 'ok';

  const run = async () => {
    if (!engineOk) return;
    setRunning(true); setResults([]);
    try {
      const params: Record<string, string> = {};
      if (activePack.defaultHost !== undefined) params.host = param;
      if ((activePack as any).defaultService !== undefined) params.service = param;
      const resp = await fetch(`${ENGINE_URL}/v1/investigate`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ pack: activePack.id, params }),
      });
      const json = await resp.json();
      setResults(json.results || []);
      setTotalMs(json.total_time_ms || 0);
    } catch { /* ignore */ }
    setRunning(false);
  };

  const selectPack = (p: typeof packs[0]) => {
    setActivePack(p);
    setParam(p.defaultHost || (p as any).defaultService || '');
    setResults([]);
  };

  return (
    <div className="space-y-4 pt-4 animate-fade-in">
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        {/* Left: pack selector + run */}
        <div className="space-y-4">
          <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4">
            <div className="text-xs font-semibold text-[var(--color-text-dim)] uppercase tracking-wider mb-3">Investigation Pack</div>
            <div className="grid grid-cols-2 gap-2 mb-4">
              {packs.map((p) => (
                <button
                  key={p.id}
                  onClick={() => selectPack(p)}
                  className={`rounded-lg border p-3 text-left transition-all cursor-pointer ${
                    activePack.id === p.id
                      ? 'border-[var(--color-amber)]/40 bg-[var(--color-amber-dim)]'
                      : 'border-[var(--color-border)] hover:border-[var(--color-border-bright)]'
                  }`}
                >
                  <div className="w-6 h-6 rounded flex items-center justify-center text-[10px] font-bold mb-1"
                    style={{ background: 'var(--color-amber-dim)', color: 'var(--color-amber)', border: '1px solid var(--color-amber)' }}>
                    {p.icon}
                  </div>
                  <div className="text-xs font-medium text-[var(--color-text)]">{p.label}</div>
                </button>
              ))}
            </div>
            <div className="space-y-2">
              <label className="text-[10px] text-[var(--color-text-dim)] uppercase tracking-wider">
                {activePack.defaultHost !== undefined ? 'Host' : 'Service'}
              </label>
              <input
                value={param}
                onChange={(e) => setParam(e.target.value)}
                className="w-full px-3 py-2 rounded border border-[var(--color-border)] bg-[var(--color-surface-3)] text-sm text-[var(--color-text)] font-mono focus:outline-none focus:border-[var(--color-amber)]"
              />
              <button
                onClick={run}
                disabled={running || !engineOk || !param}
                className="w-full py-2 rounded bg-[var(--color-amber-dim)] border border-[var(--color-amber)]/30 text-[var(--color-amber)] text-xs font-semibold hover:bg-[var(--color-amber)]/20 transition-all cursor-pointer disabled:opacity-40"
              >
                {running ? 'Running...' : 'Run Investigation'}
              </button>
            </div>
          </div>

          {/* Alert flow steps */}
          <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4">
            <div className="text-xs font-semibold text-[var(--color-text-dim)] uppercase tracking-wider mb-3">Alert → RCA Flow</div>
            <div className="space-y-1">
              {investigationSteps.map((step) => (
                <button
                  key={step.id}
                  onClick={() => setActiveStep(activeStep === step.id ? null : step.id)}
                  className={`w-full text-left px-3 py-2 rounded text-xs transition-all cursor-pointer flex items-center gap-2 ${
                    activeStep === step.id
                      ? 'bg-[var(--color-accent-dim)] border border-[var(--color-accent)]/30'
                      : 'hover:bg-[var(--color-surface-3)] border border-transparent'
                  }`}
                >
                  <span className="text-sm">{step.icon}</span>
                  <div className="flex-1 min-w-0">
                    <div className="text-[var(--color-text)] font-medium truncate">{step.title}</div>
                    {activeStep === step.id && (
                      <div className="mt-1 text-[10px] text-[var(--color-text-dim)] whitespace-pre-line">{step.detail}</div>
                    )}
                  </div>
                </button>
              ))}
            </div>
          </div>
        </div>

        {/* Right: results (2 col) */}
        <div className="lg:col-span-2 space-y-3">
          {results.length === 0 ? (
            <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-8 text-center">
              <div className="text-[var(--color-text-dim)] text-sm">Select a pack and run to see results</div>
              <div className="text-[10px] text-[var(--color-text-dim)] mt-1">Investigation pack runs 3 parallel queries against AETHERIS backends</div>
            </div>
          ) : (
            <>
              <div className="flex items-center gap-3">
                <span className="text-xs font-semibold text-[var(--color-green)] uppercase tracking-wider">
                  {results.length} queries completed
                </span>
                <span className="text-[10px] text-[var(--color-text-dim)] font-mono">{totalMs}ms total</span>
              </div>
              {/* Timeline bar */}
              <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-3">
                <div className="text-[10px] font-semibold text-[var(--color-text-dim)] uppercase tracking-wider mb-2">Parallel Execution Timeline</div>
                <div className="flex items-center gap-1 h-6">
                  {results.map((r, i) => {
                    const maxMs = Math.max(...results.map(x => x.execute_time_ms), 1);
                    const width = Math.max((r.execute_time_ms / maxMs) * 100, 10);
                    const colors = ['var(--color-accent)', 'var(--color-cyan)', 'var(--color-green)'];
                    return (
                      <div key={r.name} className="flex-1 relative group">
                        <div
                          className="h-5 rounded"
                          style={{ width: `${width}%`, background: `${colors[i % colors.length]}30`, border: `1px solid ${colors[i % colors.length]}60` }}
                        />
                        <div className="absolute inset-0 flex items-center px-2">
                          <span className="text-[8px] font-mono text-[var(--color-text)] truncate">{r.name} ({r.execute_time_ms}ms)</span>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>

              {results.map((r) => (
                <div key={r.name} className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
                  <div className="flex items-center justify-between px-4 py-2 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
                    <div className="flex items-center gap-2">
                      <span className={`w-1.5 h-1.5 rounded-full ${r.status === 'success' ? 'bg-[var(--color-green)]' : 'bg-[var(--color-red)]'}`} />
                      <span className="text-xs font-semibold text-[var(--color-text)]">{r.name}</span>
                    </div>
                    <span className="text-[10px] text-[var(--color-text-dim)] font-mono">{r.execute_time_ms}ms</span>
                  </div>
                  <div className="p-3 space-y-2">
                    <div className="text-[10px]">
                      <span className="text-[var(--color-text-dim)]">UNIQL: </span>
                      <span className="text-[var(--color-accent)] font-mono">{r.query}</span>
                    </div>
                    {r.native_query && (
                      <div className="text-[10px]">
                        <span className="text-[var(--color-text-dim)]">Native: </span>
                        <span className="text-[var(--color-cyan)] font-mono">{r.native_query}</span>
                      </div>
                    )}
                    {r.error && <div className="text-[10px] text-[var(--color-red)]">{r.error}</div>}
                    <DataPreview data={r.data} />
                  </div>
                </div>
              ))}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function DataPreview({ data }: { data: unknown }) {
  if (!data || data === null) return <div className="text-[10px] text-[var(--color-text-dim)]">No data</div>;

  const d = data as Record<string, unknown>;

  // Prometheus format: data.result[]
  const promResult = (d?.data as Record<string, unknown>)?.result;
  if (Array.isArray(promResult) && promResult.length > 0) {
    return (
      <div className="space-y-1">
        <div className="text-[9px] text-[var(--color-text-dim)]">{promResult.length} series</div>
        <div className="max-h-28 overflow-auto space-y-0.5">
          {promResult.slice(0, 8).map((item: Record<string, unknown>, i: number) => {
            const metric = item.metric as Record<string, string> | undefined;
            const value = (item.value as [number, string])?.[1];
            const labels = metric ? Object.entries(metric).filter(([k]) => k !== '__name__').map(([k, v]) => `${k}=${v}`).join(', ') : '';
            const name = (metric as Record<string, string>)?.__name__ || '';
            return (
              <div key={i} className="flex items-center gap-2 text-[10px] font-mono">
                {name && <span className="text-[var(--color-accent)] shrink-0">{name}</span>}
                {labels && <span className="text-[var(--color-text-dim)] truncate">{'{' + labels + '}'}</span>}
                {value && <span className="text-[var(--color-green)] ml-auto shrink-0">{parseFloat(value).toFixed(2)}</span>}
              </div>
            );
          })}
          {promResult.length > 8 && <div className="text-[9px] text-[var(--color-text-dim)]">+{promResult.length - 8} more...</div>}
        </div>
      </div>
    );
  }

  // VLogs format: result[]
  const vlogsResult = d?.result;
  if (Array.isArray(vlogsResult) && vlogsResult.length > 0) {
    return (
      <div className="space-y-1">
        <div className="text-[9px] text-[var(--color-text-dim)]">{vlogsResult.length} log entries</div>
        <div className="max-h-28 overflow-auto space-y-0.5">
          {vlogsResult.slice(0, 5).map((entry: Record<string, unknown>, i: number) => {
            const msg = (entry._msg as string) || '';
            const preview = msg.length > 120 ? msg.slice(0, 120) + '...' : msg;
            return (
              <div key={i} className="text-[10px] font-mono text-[var(--color-text-dim)] truncate">{preview || JSON.stringify(entry).slice(0, 120)}</div>
            );
          })}
          {vlogsResult.length > 5 && <div className="text-[9px] text-[var(--color-text-dim)]">+{vlogsResult.length - 5} more...</div>}
        </div>
      </div>
    );
  }

  // Table format
  if (d?.format === 'table' && Array.isArray(d?.rows)) {
    const rows = d.rows as unknown[][];
    return (
      <div className="text-[9px] text-[var(--color-text-dim)]">{rows.length} rows (table format)</div>
    );
  }

  // Fallback: raw JSON
  const json = JSON.stringify(data, null, 2);
  return (
    <pre className="text-[10px] text-[var(--color-text-dim)] font-mono max-h-24 overflow-auto bg-[var(--color-surface)] rounded p-2">
      {json.length > 500 ? json.slice(0, 500) + '\n...' : json}
    </pre>
  );
}
