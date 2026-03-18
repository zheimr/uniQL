import { useState, useEffect, useCallback, useRef } from 'react';

const ENGINE_URL = `http://${window.location.hostname}:9090`;
const REFRESH_INTERVAL = 15_000;

// ─── Types ───────────────────────────────────────────────────────

interface MetricResult {
  metric: Record<string, string>;
  value: [number, string];
  values?: [number, string][];
}

interface PanelConfig {
  id: string;
  title: string;
  uniql: string;
  rangeQuery?: string;
  type: 'stat' | 'table' | 'bar' | 'log';
  color: string;
  source: string;
  extract: (data: any) => PanelData;
}

interface PanelData {
  display: string;
  rows?: { label: string; value: string; color?: string }[];
  rangeValues?: number[];
}

interface LogEntry {
  _msg?: string;
  _time?: string;
  [key: string]: unknown;
}

// ─── Panel Definitions ───────────────────────────────────────────

const PANELS: PanelConfig[] = [
  // Row 1: Key stats
  {
    id: 'snmp', title: 'SNMP Devices', type: 'stat', color: '#39d0d8', source: 'VictoriaMetrics',
    uniql: 'SHOW timeseries FROM victoria WHERE __name__ = "count(snmpv2_device_up==1)"',
    rangeQuery: 'SHOW timeseries FROM victoria WHERE __name__ = "count(snmpv2_device_up==1)" WITHIN last 1h',
    extract: (d: any) => {
      const r: MetricResult[] = d?.data?.data?.result ?? [];
      const val = r[0]?.value?.[1] || '0';
      const range = r[0]?.values?.map(([, v]: [number, string]) => parseFloat(v)) ?? [];
      return { display: parseInt(val).toLocaleString(), rangeValues: range };
    },
  },
  {
    id: 'vms', title: 'vSphere VMs', type: 'stat', color: '#7c5cfc', source: 'VictoriaMetrics',
    uniql: 'SHOW timeseries FROM victoria WHERE __name__ = "count(count by (vmname)(vsphere_vm_cpu_usage_average))"',
    rangeQuery: 'SHOW timeseries FROM victoria WHERE __name__ = "count(count by (vmname)(vsphere_vm_cpu_usage_average))" WITHIN last 1h',
    extract: (d: any) => {
      const r: MetricResult[] = d?.data?.data?.result ?? [];
      const val = r[0]?.value?.[1] || '0';
      const range = r[0]?.values?.map(([, v]: [number, string]) => parseFloat(v)) ?? [];
      return { display: parseInt(val).toLocaleString(), rangeValues: range };
    },
  },
  {
    id: 'services', title: 'Services', type: 'stat', color: '#3fb950', source: 'VictoriaMetrics',
    uniql: 'SHOW timeseries FROM victoria WHERE __name__ = "up"',
    extract: (d: any) => {
      const r: MetricResult[] = d?.data?.data?.result ?? [];
      const up = r.filter(x => x.value?.[1] === '1').length;
      return { display: `${up}/${r.length}` };
    },
  },
  {
    id: 'esxi', title: 'ESXi CPU', type: 'stat', color: '#d29922', source: 'VictoriaMetrics',
    uniql: 'SHOW timeseries FROM victoria WHERE __name__ = "avg(vsphere_host_cpu_usage_average)"',
    rangeQuery: 'SHOW timeseries FROM victoria WHERE __name__ = "avg(vsphere_host_cpu_usage_average)" WITHIN last 1h',
    extract: (d: any) => {
      const r: MetricResult[] = d?.data?.data?.result ?? [];
      const val = parseFloat(r[0]?.value?.[1] || '0').toFixed(1);
      const range = r[0]?.values?.map(([, v]: [number, string]) => parseFloat(v)) ?? [];
      return { display: `${val}%`, rangeValues: range };
    },
  },

  // Row 2: Detailed panels
  {
    id: 'esxi-hosts', title: 'ESXi Host CPU', type: 'bar', color: '#d29922', source: 'VictoriaMetrics',
    uniql: 'SHOW timeseries FROM victoria WHERE __name__ = "vsphere_host_cpu_usage_average"',
    extract: (d: any) => {
      const r: MetricResult[] = d?.data?.data?.result ?? [];
      const seen = new Map<string, number>();
      for (const x of r) {
        const host = x.metric.esxhostname || '?';
        const val = parseFloat(x.value?.[1] || '0');
        seen.set(host, Math.max(seen.get(host) || 0, val));
      }
      const rows = [...seen.entries()]
        .sort((a, b) => b[1] - a[1])
        .slice(0, 6)
        .map(([host, val]) => ({
          label: host.split('.')[0],
          value: `${val.toFixed(1)}%`,
          color: val > 50 ? 'var(--color-red)' : val > 20 ? 'var(--color-amber)' : 'var(--color-green)',
        }));
      return { display: `${seen.size} hosts`, rows };
    },
  },
  {
    id: 'platform-health', title: 'Platform Services', type: 'table', color: '#3fb950', source: 'VictoriaMetrics',
    uniql: 'SHOW timeseries FROM victoria WHERE __name__ = "up"',
    extract: (d: any) => {
      const r: MetricResult[] = d?.data?.data?.result ?? [];
      const rows = r
        .sort((a, b) => (a.metric.job || '').localeCompare(b.metric.job || ''))
        .map(x => ({
          label: x.metric.job || '?',
          value: x.value?.[1] === '1' ? 'UP' : 'DOWN',
          color: x.value?.[1] === '1' ? 'var(--color-green)' : 'var(--color-red)',
        }));
      return { display: `${rows.filter(r => r.value === 'UP').length}/${rows.length}`, rows };
    },
  },
  {
    id: 'pg-connections', title: 'PostgreSQL Connections', type: 'bar', color: '#336791', source: 'VictoriaMetrics',
    uniql: 'SHOW timeseries FROM victoria WHERE __name__ = "pg_stat_activity_count"',
    extract: (d: any) => {
      const r: MetricResult[] = d?.data?.data?.result ?? [];
      const byDb = new Map<string, number>();
      for (const x of r) {
        const db = x.metric.datname || x.metric.state || '?';
        byDb.set(db, (byDb.get(db) || 0) + parseFloat(x.value?.[1] || '0'));
      }
      const rows = [...byDb.entries()]
        .filter(([, v]) => v > 0)
        .sort((a, b) => b[1] - a[1])
        .slice(0, 6)
        .map(([db, val]) => ({ label: db, value: String(Math.round(val)), color: '#336791' }));
      return { display: `${rows.reduce((s, r) => s + parseInt(r.value), 0)} active`, rows };
    },
  },
  {
    id: 'traefik', title: 'Traefik Routes', type: 'bar', color: '#00b4d8', source: 'VictoriaMetrics',
    uniql: 'SHOW timeseries FROM victoria WHERE __name__ = "traefik_service_requests_total"',
    extract: (d: any) => {
      const r: MetricResult[] = d?.data?.data?.result ?? [];
      const byService = new Map<string, number>();
      for (const x of r) {
        const svc = (x.metric.service || '?').replace(/@.*/, '');
        byService.set(svc, (byService.get(svc) || 0) + parseFloat(x.value?.[1] || '0'));
      }
      const rows = [...byService.entries()]
        .sort((a, b) => b[1] - a[1])
        .slice(0, 6)
        .map(([svc, val]) => ({ label: svc, value: val > 1000 ? `${(val/1000).toFixed(0)}K` : String(Math.round(val)), color: '#00b4d8' }));
      return { display: `${byService.size} services`, rows };
    },
  },
];

const LOG_SOURCES = [
  { id: 'all', label: 'ALL', query: '', color: 'var(--color-accent)' },
  { id: 'fortigate', label: 'FortiGate', query: 'WHERE job = "fortigate"', color: 'var(--color-amber)' },
  { id: 'fsso', label: 'FSSO', query: 'WHERE job = "fsso"', color: 'var(--color-cyan)' },
];

const TIME_RANGES = ['5m', '15m', '1h', '6h', '24h'];

// ─── Component ───────────────────────────────────────────────────

export default function LiveTab() {
  const [panelData, setPanelData] = useState<Record<string, PanelData>>({});
  const [loading, setLoading] = useState(true);
  const [countdown, setCountdown] = useState(REFRESH_INTERVAL / 1000);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [logSource, setLogSource] = useState('fortigate');
  const [timeRange, setTimeRange] = useState('5m');
  const [logMs, setLogMs] = useState(0);
  const [logNative, setLogNative] = useState('');
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchAll = useCallback(async () => {
    setLoading(true);
    const results = await Promise.allSettled(
      PANELS.map(async (panel) => {
        // Fetch instant + range in parallel
        const [instantResp, rangeResp] = await Promise.all([
          fetch(`${ENGINE_URL}/v1/query`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ query: panel.uniql }) }),
          panel.rangeQuery ? fetch(`${ENGINE_URL}/v1/query`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ query: panel.rangeQuery }) }) : Promise.resolve(null),
        ]);
        const data = await instantResp.json();
        let panelResult = panel.extract(data);
        // Merge range data if available
        if (rangeResp) {
          const rangeData = await rangeResp.json();
          const rangeResults: MetricResult[] = rangeData?.data?.data?.result ?? [];
          if (rangeResults[0]?.values) {
            panelResult.rangeValues = rangeResults[0].values.map(([, v]: [number, string]) => parseFloat(v));
          }
        }
        return { id: panel.id, data: panelResult };
      })
    );

    const newData: Record<string, PanelData> = {};
    for (const r of results) {
      if (r.status === 'fulfilled') newData[r.value.id] = r.value.data;
    }
    setPanelData(newData);
    setLoading(false);
    setCountdown(REFRESH_INTERVAL / 1000);
  }, []);

  const fetchLogs = useCallback(async () => {
    const source = LOG_SOURCES.find(s => s.id === logSource) || LOG_SOURCES[0];
    try {
      const resp = await fetch(`${ENGINE_URL}/v1/query`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ query: `SHOW table FROM vlogs ${source.query} WITHIN last ${timeRange}` }),
      });
      const json = await resp.json();
      setLogNative(json?.metadata?.native_query || '');
      setLogMs(json?.metadata?.total_time_ms || 0);
      const data = json?.data;
      if (data?.format === 'table' && data?.columns && data?.rows) {
        const cols: string[] = data.columns;
        setLogs(data.rows.slice(0, 25).map((row: unknown[]) => {
          const obj: LogEntry = {};
          cols.forEach((col: string, i: number) => { obj[col] = row[i] as string; });
          return obj;
        }));
      } else { setLogs([]); }
    } catch { setLogs([]); }
  }, [logSource, timeRange]);

  useEffect(() => { fetchAll(); const i = setInterval(fetchAll, REFRESH_INTERVAL); return () => clearInterval(i); }, [fetchAll]);
  useEffect(() => { fetchLogs(); }, [fetchLogs]);
  useEffect(() => {
    countdownRef.current = setInterval(() => setCountdown(p => p <= 1 ? REFRESH_INTERVAL / 1000 : p - 1), 1000);
    return () => { if (countdownRef.current) clearInterval(countdownRef.current); };
  }, []);

  const statPanels = PANELS.filter(p => p.type === 'stat');
  const detailPanels = PANELS.filter(p => p.type !== 'stat');

  return (
    <div className="space-y-4 pt-4 animate-fade-in">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h2 className="text-sm font-semibold text-[var(--color-text-bright)]">Live Dashboard</h2>
          <span className="relative flex items-center justify-center w-2 h-2">
            <span className="absolute w-full h-full rounded-full bg-[var(--color-green)]" style={{ animation: 'pulse-ring 2s ease-out infinite' }} />
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--color-green)]" />
          </span>
          <span className="text-[10px] text-[var(--color-text-dim)] font-mono">All queries via UniQL engine</span>
        </div>
        <span className="text-[10px] text-[var(--color-text-dim)] font-mono">
          {loading ? 'Refreshing...' : `Next: ${countdown}s`}
        </span>
      </div>

      {/* Stat cards */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
        {statPanels.map(panel => {
          const d = panelData[panel.id];
          return (
            <div key={panel.id} className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4">
              <div className="flex items-center gap-2 mb-2">
                <span className="w-2 h-2 rounded-full" style={{ background: panel.color }} />
                <span className="text-[10px] text-[var(--color-text-dim)] uppercase tracking-wider">{panel.title}</span>
                <span className="text-[8px] text-[var(--color-text-dim)] font-mono ml-auto">{panel.source}</span>
              </div>
              <div className="text-2xl font-bold font-mono" style={{ color: panel.color }}>{d?.display ?? '--'}</div>
              {d?.rangeValues && d.rangeValues.length > 1 && <MiniChart data={d.rangeValues} color={panel.color} />}
            </div>
          );
        })}
      </div>

      {/* Detail panels */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-3">
        {detailPanels.map(panel => {
          const d = panelData[panel.id];
          return (
            <div key={panel.id} className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
              <div className="flex items-center justify-between px-3 py-2 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
                <div className="flex items-center gap-2">
                  <span className="w-1.5 h-1.5 rounded-full" style={{ background: panel.color }} />
                  <span className="text-[10px] font-semibold text-[var(--color-text)]">{panel.title}</span>
                </div>
                <span className="text-[9px] text-[var(--color-text-dim)] font-mono">{d?.display ?? '--'}</span>
              </div>
              <div className="max-h-44 overflow-auto">
                {d?.rows?.map((row, i) => (
                  <div key={i} className="flex items-center justify-between px-3 py-1.5 border-b border-[var(--color-border)]/20 hover:bg-[var(--color-surface-3)] transition-colors">
                    <span className="text-[10px] text-[var(--color-text)] font-mono truncate flex-1">{row.label}</span>
                    {panel.type === 'bar' && (
                      <div className="w-16 h-1.5 rounded-full bg-[var(--color-surface)] mx-2 overflow-hidden">
                        <div className="h-full rounded-full" style={{ width: `${Math.min(100, parseFloat(row.value) / (panel.id === 'esxi-hosts' ? 100 : Math.max(...(d.rows?.map(r => parseFloat(r.value)) || [1]))) * 100)}%`, background: row.color || panel.color }} />
                      </div>
                    )}
                    <span className="text-[10px] font-bold font-mono shrink-0" style={{ color: row.color || panel.color }}>{row.value}</span>
                  </div>
                )) || (
                  <div className="p-4 text-center text-[10px] text-[var(--color-text-dim)]">Loading...</div>
                )}
              </div>
            </div>
          );
        })}
      </div>

      {/* Log stream */}
      <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
        <div className="flex items-center justify-between px-4 py-2.5 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
          <div className="flex items-center gap-3">
            <span className="text-xs font-semibold text-[var(--color-amber)]">Log Stream</span>
            {LOG_SOURCES.map(src => (
              <button key={src.id} onClick={() => setLogSource(src.id)}
                className={`px-2 py-0.5 rounded text-[9px] font-semibold cursor-pointer transition-all ${
                  logSource === src.id ? 'text-white' : 'text-[var(--color-text-dim)] hover:text-[var(--color-text)]'
                }`}
                style={logSource === src.id ? { background: `${src.color}25`, color: src.color, border: `1px solid ${src.color}40` } : { border: '1px solid transparent' }}
              >{src.label}</button>
            ))}
            {logs.length > 0 && <span className="text-[10px] text-[var(--color-green)] font-mono">{logs.length} entries</span>}
          </div>
          <div className="flex items-center gap-2 text-[10px] font-mono text-[var(--color-text-dim)]">
            {TIME_RANGES.map(t => (
              <button key={t} onClick={() => setTimeRange(t)}
                className={`px-1.5 py-0.5 rounded text-[9px] cursor-pointer ${timeRange === t ? 'bg-[var(--color-amber)]/15 text-[var(--color-amber)] border border-[var(--color-amber)]/30' : 'text-[var(--color-text-dim)] border border-transparent'}`}
              >{t}</button>
            ))}
            {logMs > 0 && <span>{logMs}ms</span>}
            {logNative && <span className="text-[var(--color-cyan)] max-w-[200px] truncate">{logNative}</span>}
          </div>
        </div>
        <div className="max-h-64 overflow-auto divide-y divide-[var(--color-border)]/15">
          {logs.length === 0 ? (
            <div className="p-6 text-center text-[var(--color-text-dim)] text-[11px]">No logs in selected range</div>
          ) : logs.map((entry, i) => {
            const time = entry._time as string | undefined;
            const action = entry.action as string | undefined;
            const srcIp = entry.source_ip as string | undefined;
            const subtype = entry.subtype as string | undefined;
            const job = entry.job as string | undefined;
            const msg = (entry._msg as string || '').slice(0, 160);
            return (
              <div key={i} className="px-4 py-1 hover:bg-[var(--color-surface-3)] transition-colors flex items-center gap-2">
                {time && <span className="text-[9px] text-[var(--color-text-dim)] font-mono shrink-0 w-[50px]">{new Date(time).toLocaleTimeString('tr-TR', { hour: '2-digit', minute: '2-digit', second: '2-digit' })}</span>}
                {logSource === 'all' && job && <span className="text-[8px] font-mono px-1 py-0.5 rounded shrink-0" style={{ background: job === 'fortigate' ? 'var(--color-amber)' : 'var(--color-cyan)', color: '#000', opacity: 0.8 }}>{job}</span>}
                {action && <span className={`text-[8px] font-mono px-1 py-0.5 rounded shrink-0 ${action === 'deny' ? 'bg-[var(--color-red)]/15 text-[var(--color-red)]' : action === 'accept' ? 'bg-[var(--color-green)]/15 text-[var(--color-green)]' : 'text-[var(--color-text-dim)]'}`}>{action}</span>}
                {subtype && <span className="text-[8px] text-[var(--color-cyan)] font-mono shrink-0">{subtype}</span>}
                {srcIp && <span className="text-[8px] text-[var(--color-amber)] font-mono shrink-0">{srcIp}</span>}
                <span className="text-[10px] text-[var(--color-text)] font-mono truncate min-w-0">{msg || JSON.stringify(entry).slice(0, 120)}</span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

// ─── Mini chart for stat cards ───────────────────────────────────

function MiniChart({ data, color }: { data: number[]; color: string }) {
  if (data.length < 2) return null;
  const min = Math.min(...data);
  const max = Math.max(...data);
  const range = max - min || 1;
  const w = 180; const h = 32;
  const points = data.map((v, i) => `${(i / (data.length - 1)) * w},${h - ((v - min) / range) * (h - 4) - 2}`).join(' ');
  const fill = `0,${h} ${points} ${w},${h}`;
  const gid = `mc-${color.replace(/[^a-z0-9]/gi, '')}-${data.length}`;
  return (
    <svg width="100%" height={h} viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" className="mt-2">
      <defs><linearGradient id={gid} x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor={color} stopOpacity="0.2" /><stop offset="100%" stopColor={color} stopOpacity="0" /></linearGradient></defs>
      <polygon points={fill} fill={`url(#${gid})`} />
      <polyline points={points} fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" opacity="0.8" />
    </svg>
  );
}
