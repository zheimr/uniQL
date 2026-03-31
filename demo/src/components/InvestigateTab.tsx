import { useState } from 'react';
import type { EngineHealth } from '../App';
import { investigationSteps } from '../data/scenarios';

const ENGINE_URL = `http://${window.location.hostname}:9090`;

const packs = [
  { id: 'high_cpu', label: 'High CPU', icon: '🔥', desc: 'ESXi host CPU trend + VM CPU + memory', defaultHost: 'esxi-node01.example.com', color: '#d29922' },
  { id: 'link_down', label: 'Link Down', icon: '🔌', desc: 'Device status + interface + firewall logs', defaultHost: 'CORE-SW-01', color: '#f85149' },
  { id: 'error_spike', label: 'Error Spike', icon: '📈', desc: 'SOC events + error logs + API errors', defaultHost: '', defaultService: 'admin-api', color: '#7c5cfc' },
  { id: 'latency_degradation', label: 'Latency', icon: '🐌', desc: 'API latency + request rate + slow logs', defaultHost: '', defaultService: 'admin-api', color: '#39d0d8' },
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
    <div className="space-y-6 pt-4 animate-fade-in">
      {/* Header */}
      <div>
        <h2 className="text-xl font-bold text-[var(--color-text-bright)] mb-1">Investigation Packs</h2>
        <p className="text-[13px] text-[var(--color-text-dim)]">Alert fires → UniQL runs 3 parallel queries → root cause in seconds</p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Left: Pack selector + RCA flow */}
        <div className="space-y-4">
          {/* Pack cards */}
          <div className="grid grid-cols-2 gap-2">
            {packs.map(p => (
              <button key={p.id} onClick={() => selectPack(p)}
                className={`rounded-xl border p-3 text-left transition-all cursor-pointer ${
                  activePack.id === p.id ? 'border-[var(--color-accent)]/40 bg-[var(--color-accent)]/5' : 'border-[var(--color-border)] hover:border-[var(--color-border-bright)]'
                }`}>
                <div className="text-lg mb-1">{p.icon}</div>
                <div className="text-xs font-semibold text-[var(--color-text)]">{p.label}</div>
                <div className="text-[9px] text-[var(--color-text-dim)] mt-0.5">{p.desc}</div>
              </button>
            ))}
          </div>

          {/* Run control */}
          <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4 space-y-3">
            <label className="text-[10px] text-[var(--color-text-dim)] uppercase tracking-wider">
              {activePack.defaultHost !== undefined ? 'Target Host' : 'Target Service'}
            </label>
            <input value={param} onChange={e => setParam(e.target.value)}
              className="w-full px-3 py-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-3)] text-sm text-[var(--color-text)] font-mono focus:outline-none focus:border-[var(--color-accent)]" />
            <button onClick={run} disabled={running || !engineOk || !param}
              className="w-full py-2.5 rounded-lg bg-gradient-to-r from-[var(--color-accent)] to-[var(--color-cyan)] text-white text-sm font-bold cursor-pointer disabled:opacity-40 hover:opacity-90 transition-opacity">
              {running ? 'Investigating...' : 'Run Investigation'}
            </button>
          </div>

          {/* RCA Flow Timeline */}
          <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4">
            <div className="text-[10px] font-semibold text-[var(--color-text-dim)] uppercase tracking-wider mb-3">Alert → RCA Flow</div>
            <div className="space-y-0">
              {investigationSteps.map((step, i) => (
                <button key={step.id} onClick={() => setActiveStep(activeStep === step.id ? null : step.id)}
                  className="w-full text-left cursor-pointer group">
                  <div className="flex items-start gap-3 py-2">
                    {/* Timeline line + dot */}
                    <div className="flex flex-col items-center shrink-0">
                      <div className={`w-6 h-6 rounded-full flex items-center justify-center text-[10px] ${
                        activeStep === step.id ? 'bg-[var(--color-accent)] text-white' : 'bg-[var(--color-surface-3)] text-[var(--color-text-dim)] border border-[var(--color-border)]'
                      }`}>{step.icon}</div>
                      {i < investigationSteps.length - 1 && <div className="w-[1px] h-4 bg-[var(--color-border)]" />}
                    </div>
                    <div className="min-w-0 flex-1 -mt-0.5">
                      <div className="text-[11px] font-semibold text-[var(--color-text)] group-hover:text-[var(--color-accent)] transition-colors">{step.title}</div>
                      <div className="text-[10px] text-[var(--color-text-dim)]">{step.description}</div>
                      {activeStep === step.id && (
                        <div className="mt-2 text-[10px] text-[var(--color-text-dim)] bg-[var(--color-surface-3)] rounded-lg p-2 whitespace-pre-line animate-fade-in">{step.detail}</div>
                      )}
                    </div>
                  </div>
                </button>
              ))}
            </div>
          </div>
        </div>

        {/* Right: Results (2 col) */}
        <div className="lg:col-span-2 space-y-3">
          {results.length === 0 ? (
            <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-12 text-center">
              <div className="text-3xl mb-3">{activePack.icon}</div>
              <div className="text-sm text-[var(--color-text)] font-semibold mb-1">Select a pack and run investigation</div>
              <div className="text-[11px] text-[var(--color-text-dim)]">3 parallel UniQL queries execute against live AETHERIS backends</div>
            </div>
          ) : (
            <>
              {/* Summary bar */}
              <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4">
                <div className="flex items-center justify-between mb-3">
                  <div className="flex items-center gap-3">
                    <span className="text-sm font-bold text-[var(--color-text-bright)]">{activePack.label} Investigation</span>
                    <span className="text-[10px] px-2 py-0.5 rounded-full bg-[var(--color-green)]/10 text-[var(--color-green)] font-semibold">
                      {results.filter(r => r.status === 'success').length}/{results.length} success
                    </span>
                    <span className="text-[10px] text-[var(--color-text-dim)] font-mono">{totalMs}ms total</span>
                  </div>
                </div>
                {/* Timeline bar */}
                <div className="flex items-center gap-1 h-8">
                  {results.map((r, i) => {
                    const maxMs = Math.max(...results.map(x => x.execute_time_ms), 1);
                    const width = Math.max((r.execute_time_ms / maxMs) * 100, 15);
                    const colors = ['var(--color-accent)', 'var(--color-cyan)', 'var(--color-green)'];
                    return (
                      <div key={r.name} className="flex-1 relative group">
                        <div className="h-6 rounded-md flex items-center px-2" style={{ width: `${width}%`, background: `${colors[i % 3]}20`, border: `1px solid ${colors[i % 3]}40` }}>
                          <span className="text-[8px] font-mono truncate" style={{ color: colors[i % 3] }}>{r.name} ({r.execute_time_ms}ms)</span>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>

              {/* Analysis Report */}
              <AnalysisReport results={results} packId={activePack.id} param={param} />

              {/* Raw query details (collapsible) */}
              <Details label="Query Details">
                {results.map(r => (
                  <div key={r.name} className="border-b border-[var(--color-border)]/20 py-3 last:border-0">
                    <div className="flex items-center gap-2 mb-1">
                      <span className={`w-1.5 h-1.5 rounded-full ${r.status === 'success' ? 'bg-[var(--color-green)]' : 'bg-[var(--color-red)]'}`} />
                      <span className="text-[10px] font-semibold text-[var(--color-text)]">{r.name}</span>
                      <span className="text-[9px] text-[var(--color-text-dim)] font-mono ml-auto">{r.execute_time_ms}ms</span>
                    </div>
                    <div className="text-[10px] text-[var(--color-accent)] font-mono mb-0.5">{r.query}</div>
                    {r.native_query && <div className="text-[10px] text-[var(--color-cyan)] font-mono">{r.native_query}</div>}
                    {r.error && <div className="text-[10px] text-[var(--color-red)] mt-1">{r.error}</div>}
                  </div>
                ))}
              </Details>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function Details({ label, children }: { label: string; children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
      <button onClick={() => setOpen(!open)} className="w-full flex items-center justify-between px-4 py-2.5 text-[11px] font-semibold text-[var(--color-text-dim)] hover:text-[var(--color-text)] cursor-pointer bg-[var(--color-surface-3)]">
        {label} <span>{open ? '▼' : '▶'}</span>
      </button>
      {open && <div className="px-4 py-2">{children}</div>}
    </div>
  );
}

function AnalysisReport({ results, packId, param }: { results: PackResult[]; packId: string; param: string }) {
  // Extract structured data from results
  const extractMetrics = (r: PackResult) => {
    if (r.status !== 'success') return [];
    const d = r.data as Record<string, unknown>;
    const promResult = (d?.data as Record<string, unknown>)?.result as Record<string, unknown>[] | undefined;
    if (!Array.isArray(promResult)) return [];
    return promResult.map((item: Record<string, unknown>) => ({
      metric: item.metric as Record<string, string>,
      value: ((item.value as [number, string])?.[1]) || '0',
    }));
  };

  const extractLogs = (r: PackResult) => {
    if (r.status !== 'success') return [];
    const d = r.data as Record<string, unknown>;
    const logs = d?.result as Record<string, unknown>[] | undefined;
    if (!Array.isArray(logs)) {
      const rows = (d?.rows as unknown[][]) || [];
      const cols = (d?.columns as string[]) || [];
      return rows.slice(0, 10).map(row => {
        const obj: Record<string, string> = {};
        cols.forEach((c, i) => { obj[c] = String(row[i] || ''); });
        return obj;
      });
    }
    return logs.slice(0, 10);
  };

  if (packId === 'high_cpu') {
    const cpuData = results[0] ? extractMetrics(results[0]) : [];
    const vmData = results[1] ? extractMetrics(results[1]) : [];
    const memData = results[2] ? extractMetrics(results[2]) : [];

    // Calculate host CPU average
    const cpuValues = cpuData.map(d => parseFloat(d.value)).filter(v => !isNaN(v));
    const avgCpu = cpuValues.length > 0 ? cpuValues.reduce((a, b) => a + b, 0) / cpuValues.length : 0;

    // Top VMs by CPU
    const vms = vmData
      .filter(d => d.metric?.cpu === 'instance-total')
      .map(d => ({ name: d.metric?.vmname || '?', cpu: parseFloat(d.value) }))
      .sort((a, b) => b.cpu - a.cpu);

    // Memory
    const memValue = memData.length > 0 ? parseFloat(memData[0].value) : 0;

    const cpuSeverity = avgCpu > 80 ? 'CRITICAL' : avgCpu > 50 ? 'HIGH' : avgCpu > 20 ? 'MODERATE' : 'LOW';
    const cpuColor = avgCpu > 80 ? 'var(--color-red)' : avgCpu > 50 ? 'var(--color-amber)' : 'var(--color-green)';

    return (
      <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
        <div className="px-4 py-3 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
          <div className="text-xs font-bold text-[var(--color-text-bright)]">Analysis: {param}</div>
          <div className="text-[10px] text-[var(--color-text-dim)]">Production_Cluster — {cpuData.length} CPU cores, {vms.length} VMs</div>
        </div>
        <div className="p-4 space-y-4">
          {/* CPU Overview */}
          <div className="flex items-center gap-4">
            <div className="w-20 h-20 rounded-full border-4 flex items-center justify-center" style={{ borderColor: cpuColor }}>
              <div className="text-center">
                <div className="text-lg font-bold font-mono" style={{ color: cpuColor }}>{avgCpu.toFixed(1)}%</div>
                <div className="text-[8px] text-[var(--color-text-dim)]">AVG CPU</div>
              </div>
            </div>
            <div className="flex-1">
              <div className="flex items-center gap-2 mb-1">
                <span className="text-[10px] font-bold px-2 py-0.5 rounded" style={{ background: `${cpuColor}20`, color: cpuColor }}>{cpuSeverity}</span>
                <span className="text-[11px] text-[var(--color-text)]">Host CPU utilization is {cpuSeverity.toLowerCase()}</span>
              </div>
              <div className="text-[10px] text-[var(--color-text-dim)]">
                Memory: {memValue.toFixed(1)}% — {memValue > 80 ? 'Memory pressure detected' : 'Memory normal'}
              </div>
            </div>
          </div>

          {/* Top VMs */}
          {vms.length > 0 && (
            <div>
              <div className="text-[10px] font-semibold text-[var(--color-text-dim)] uppercase tracking-wider mb-2">Top VMs by CPU on this host</div>
              <div className="space-y-1">
                {vms.slice(0, 5).map((vm, i) => {
                  const barColor = vm.cpu > 50 ? 'var(--color-red)' : vm.cpu > 20 ? 'var(--color-amber)' : 'var(--color-green)';
                  return (
                    <div key={i} className="flex items-center gap-2">
                      <span className="text-[10px] font-mono text-[var(--color-text)] w-44 truncate">{vm.name}</span>
                      <div className="flex-1 h-2 rounded-full bg-[var(--color-surface)] overflow-hidden">
                        <div className="h-full rounded-full transition-all" style={{ width: `${Math.min(100, vm.cpu)}%`, background: barColor }} />
                      </div>
                      <span className="text-[10px] font-bold font-mono w-12 text-right" style={{ color: barColor }}>{vm.cpu.toFixed(1)}%</span>
                    </div>
                  );
                })}
              </div>
            </div>
          )}

          {/* Recommendation */}
          <div className="rounded-lg bg-[var(--color-surface-3)] border border-[var(--color-border)] p-3">
            <div className="text-[10px] font-semibold text-[var(--color-accent)] mb-1">Recommendation</div>
            <div className="text-[11px] text-[var(--color-text)]">
              {avgCpu > 50
                ? `High CPU on ${param}. Top consumer: ${vms[0]?.name || '?'} at ${vms[0]?.cpu.toFixed(1)}%. Consider VM rightsizing or migration.`
                : avgCpu > 20
                ? `Moderate CPU usage. ${vms.filter(v => v.cpu > 20).length} VMs above 20%. Monitor for trend.`
                : `CPU usage is healthy. ${vms.length} VMs running normally on this host.`
              }
            </div>
          </div>
        </div>
      </div>
    );
  }

  // Generic report for other packs
  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4">
      <div className="text-xs font-bold text-[var(--color-text-bright)] mb-3">Analysis: {packId} — {param}</div>
      {results.map(r => {
        const metrics = extractMetrics(r);
        const logs = extractLogs(r);
        const count = metrics.length || logs.length;
        return (
          <div key={r.name} className="mb-3 last:mb-0">
            <div className="flex items-center gap-2 mb-1">
              <span className={`w-1.5 h-1.5 rounded-full ${r.status === 'success' ? 'bg-[var(--color-green)]' : 'bg-[var(--color-red)]'}`} />
              <span className="text-[11px] font-semibold text-[var(--color-text)]">{r.name}</span>
              <span className="text-[9px] text-[var(--color-text-dim)]">{count} results, {r.execute_time_ms}ms</span>
            </div>
            {metrics.length > 0 && (
              <div className="ml-3.5 space-y-0.5">
                {metrics.slice(0, 5).map((m, i) => {
                  const label = Object.entries(m.metric).filter(([k]) => k !== '__name__' && k !== 'job' && k !== 'instance' && k !== 'host').slice(0, 2).map(([k, v]) => `${k}=${v}`).join(' ');
                  return <div key={i} className="text-[10px] font-mono"><span className="text-[var(--color-text-dim)]">{label}</span> <span className="text-[var(--color-green)] font-bold">{parseFloat(m.value).toFixed(2)}</span></div>;
                })}
              </div>
            )}
            {logs.length > 0 && (
              <div className="ml-3.5 text-[10px] text-[var(--color-text-dim)]">{logs.length} log entries found</div>
            )}
          </div>
        );
      })}
    </div>
  );
}

function ResultPreview({ data, status }: { data: unknown; status: string }) {
  if (status !== 'success' || !data) return null;
  const d = data as Record<string, unknown>;

  // Prometheus format
  const promResult = (d?.data as Record<string, unknown>)?.result;
  if (Array.isArray(promResult)) {
    if (promResult.length === 0) return <div className="text-[10px] text-[var(--color-text-dim)]">No data in time range</div>;
    return (
      <div className="rounded-lg bg-[var(--color-surface)] border border-[var(--color-border)] p-2 max-h-36 overflow-auto">
        {promResult.slice(0, 8).map((item: Record<string, unknown>, i: number) => {
          const metric = item.metric as Record<string, string> | undefined;
          const value = (item.value as [number, string])?.[1];
          const name = metric?.__name__ || '';
          const labels = metric ? Object.entries(metric).filter(([k]) => k !== '__name__').slice(0, 3).map(([k, v]) => `${k}=${v}`).join(' ') : '';
          return (
            <div key={i} className="flex items-center gap-2 py-0.5 text-[10px] font-mono">
              {name && <span className="text-[var(--color-accent)] shrink-0">{name}</span>}
              <span className="text-[var(--color-text-dim)] truncate flex-1">{labels}</span>
              {value && <span className="text-[var(--color-green)] font-bold shrink-0">{parseFloat(value).toFixed(2)}</span>}
            </div>
          );
        })}
        {promResult.length > 8 && <div className="text-[9px] text-[var(--color-text-dim)] pt-1">+{promResult.length - 8} more</div>}
      </div>
    );
  }

  // VLogs
  const vlogsResult = d?.result;
  if (Array.isArray(vlogsResult)) {
    if (vlogsResult.length === 0) return <div className="text-[10px] text-[var(--color-text-dim)]">No logs in time range</div>;
    return (
      <div className="rounded-lg bg-[var(--color-surface)] border border-[var(--color-border)] p-2 max-h-36 overflow-auto">
        <div className="text-[9px] text-[var(--color-text-dim)] mb-1">{vlogsResult.length} log entries</div>
        {vlogsResult.slice(0, 5).map((entry: Record<string, unknown>, i: number) => (
          <div key={i} className="text-[10px] font-mono text-[var(--color-text-dim)] truncate py-0.5">
            {(entry._msg as string || '').slice(0, 100) || JSON.stringify(entry).slice(0, 100)}
          </div>
        ))}
      </div>
    );
  }

  // Table format
  if (d?.format === 'table') {
    const rows = (d.rows as unknown[][]) || [];
    return <div className="text-[10px] text-[var(--color-text-dim)]">{rows.length} rows (table format)</div>;
  }

  return null;
}
