import { useState, useEffect, useCallback, useRef } from 'react';

const ENGINE_URL = `http://${window.location.hostname}:9090`;
const REFRESH_INTERVAL = 10_000;

// --- Types ---

interface MetricResult {
  metric: Record<string, string>;
  value: [number, string];
}

interface QueryResponse {
  status: string;
  data?: {
    data?: {
      result?: MetricResult[];
    };
    // VLogs raw format
    result?: Array<{ _msg?: string; _time?: string; [key: string]: unknown }>;
    // SHOW table format
    format?: string;
    columns?: string[];
    rows?: unknown[][];
  };
  metadata?: {
    parse_time_us?: number;
    transpile_time_us?: number;
    execute_time_ms?: number;
    total_time_ms?: number;
    native_query?: string;
  };
}

interface RangeResult {
  metric: Record<string, string>;
  values: [number, string][];
}

interface WidgetConfig {
  id: string;
  title: string;
  uniql: string;
  rangeQuery?: string;
  unit: string;
  icon: string;
  color: string;
  extract: (results: MetricResult[]) => { display: string; numeric: number };
  description: string;
}

interface WidgetState {
  display: string;
  numeric: number;
  native?: string;
  totalMs?: number;
  executeMs?: number;
  parseUs?: number;
  transpileUs?: number;
  history: number[];
  rangeData: [number, string][];
  loading: boolean;
  error?: string;
}

interface HealthData {
  status: string;
  version: string;
  backends: { name: string; type: string; url: string; status: string }[];
}

interface LogEntry {
  _msg?: string;
  _time?: string;
  [key: string]: unknown;
}

// --- Widget definitions ---

const WIDGETS: WidgetConfig[] = [
  {
    id: 'snmp',
    title: 'SNMP Devices',
    uniql: 'SHOW timeseries FROM victoria WHERE __name__ = "count(snmpv2_device_up==1)"',
    rangeQuery: 'SHOW timeseries FROM victoria WHERE __name__ = "count(snmpv2_device_up==1)" WITHIN last 1h',
    unit: 'online',
    icon: 'N',
    color: '#39d0d8',
    description: 'Network devices reporting via SNMP v2 — count(snmpv2_device_up==1)',
    extract: (results) => {
      if (!results.length) return { display: '0', numeric: 0 };
      const val = parseInt(results[0]?.value?.[1] || '0');
      return { display: `${val}`, numeric: val };
    },
  },
  {
    id: 'vms',
    title: 'vSphere VMs',
    uniql: 'SHOW timeseries FROM victoria WHERE __name__ = "count(count by (vmname)(vsphere_vm_cpu_usage_average))"',
    rangeQuery: 'SHOW timeseries FROM victoria WHERE __name__ = "count(count by (vmname)(vsphere_vm_cpu_usage_average))" WITHIN last 1h',
    unit: 'active',
    icon: 'V',
    color: '#7c5cfc',
    description: 'Virtual machines with CPU telemetry — count by vmname',
    extract: (results) => {
      if (!results.length) return { display: '0', numeric: 0 };
      const val = parseInt(results[0]?.value?.[1] || '0');
      return { display: `${val}`, numeric: val };
    },
  },
  {
    id: 'esxi',
    title: 'ESXi Host CPU',
    uniql: 'SHOW timeseries FROM victoria WHERE __name__ = "avg(vsphere_host_cpu_usage_average)"',
    rangeQuery: 'SHOW timeseries FROM victoria WHERE __name__ = "avg(vsphere_host_cpu_usage_average)" WITHIN last 1h',
    unit: '%',
    icon: 'H',
    color: '#d29922',
    description: 'Average CPU utilization across all ESXi hosts',
    extract: (results) => {
      if (!results.length) return { display: '--', numeric: 0 };
      const val = parseFloat(results[0]?.value?.[1] || '0');
      return { display: val.toFixed(1), numeric: val };
    },
  },
  {
    id: 'services',
    title: 'Services Up',
    uniql: 'SHOW timeseries FROM victoria WHERE __name__ = "up"',
    rangeQuery: 'SHOW timeseries FROM victoria WHERE __name__ = "count(up==1)" WITHIN last 1h',
    unit: '',
    icon: 'S',
    color: '#3fb950',
    description: 'Platform service health via up metric',
    extract: (results) => {
      if (!results.length) return { display: '0/0', numeric: 0 };
      const up = results.filter((r) => r.value?.[1] === '1').length;
      const total = results.length;
      return { display: `${up}/${total}`, numeric: up };
    },
  },
];

// --- Component ---

const TIME_RANGES = [
  { label: '5m', value: '5m' },
  { label: '15m', value: '15m' },
  { label: '1h', value: '1h' },
  { label: '6h', value: '6h' },
  { label: '24h', value: '24h' },
];

const LOG_SOURCES = [
  { id: 'all', label: 'ALL', query: '*', color: 'var(--color-accent)' },
  { id: 'fortigate', label: 'FortiGate', query: 'job = "fortigate"', color: 'var(--color-amber)' },
  { id: 'fsso', label: 'FSSO', query: 'job = "fsso"', color: 'var(--color-cyan)' },
];

export default function LiveTab() {
  const [widgetStates, setWidgetStates] = useState<Record<string, WidgetState>>({});
  const [expanded, setExpanded] = useState<string | null>(null);
  const [countdown, setCountdown] = useState(REFRESH_INTERVAL / 1000);
  const [lastRefresh, setLastRefresh] = useState<Date | null>(null);
  const [health, setHealth] = useState<HealthData | null>(null);
  const [engineLatency, setEngineLatency] = useState<number | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [timeRange, setTimeRange] = useState('5m');
  const [logSource, setLogSource] = useState('fortigate');
  const [logState, setLogState] = useState<{
    native?: string;
    ms?: number;
    error?: string;
    fallback: boolean;
  }>({ fallback: false });
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const refreshRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // --- Fetch health ---
  const fetchHealth = useCallback(async () => {
    try {
      const start = performance.now();
      const resp = await fetch(`${ENGINE_URL}/health`);
      const elapsed = performance.now() - start;
      const data: HealthData = await resp.json();
      setHealth(data);
      setEngineLatency(Math.round(elapsed));
    } catch {
      setHealth(null);
      setEngineLatency(null);
    }
  }, []);

  // --- Fetch a single widget ---
  const fetchWidget = useCallback(async (widget: WidgetConfig) => {
    try {
      // Fetch instant value + range data in parallel
      const [instantResp, rangeResp] = await Promise.all([
        fetch(`${ENGINE_URL}/v1/query`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ query: widget.uniql }),
        }),
        widget.rangeQuery
          ? fetch(`${ENGINE_URL}/v1/query`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ query: widget.rangeQuery }),
            })
          : Promise.resolve(null),
      ]);

      const json: QueryResponse = await instantResp.json();
      const results: MetricResult[] = json.data?.data?.result ?? [];
      const { display, numeric } = widget.extract(results);

      // Extract range data (matrix values)
      let rangeData: [number, string][] = [];
      if (rangeResp) {
        const rangeJson = await rangeResp.json();
        const rangeResults: RangeResult[] = rangeJson?.data?.data?.result ?? [];
        if (rangeResults.length > 0 && rangeResults[0].values) {
          rangeData = rangeResults[0].values;
        }
      }

      setWidgetStates((prev) => {
        const prevHistory = prev[widget.id]?.history ?? [];
        const newHistory = [...prevHistory.slice(-29), numeric];
        return {
          ...prev,
          [widget.id]: {
            display,
            numeric,
            native: json.metadata?.native_query,
            totalMs: json.metadata?.total_time_ms,
            executeMs: json.metadata?.execute_time_ms,
            parseUs: json.metadata?.parse_time_us,
            transpileUs: json.metadata?.transpile_time_us,
            history: newHistory,
            rangeData,
            loading: false,
          },
        };
      });
    } catch (err) {
      setWidgetStates((prev) => ({
        ...prev,
        [widget.id]: {
          display: '--',
          numeric: 0,
          history: prev[widget.id]?.history ?? [],
          rangeData: prev[widget.id]?.rangeData ?? [],
          loading: false,
          error: err instanceof Error ? err.message : 'Fetch failed',
        },
      }));
    }
  }, []);

  // --- Fetch logs ---
  const fetchLogs = useCallback(async () => {
    try {
      const source = LOG_SOURCES.find(s => s.id === logSource) || LOG_SOURCES[0];
      const whereClause = source.id === 'all' ? '' : `WHERE ${source.query}`;
      const resp = await fetch(`${ENGINE_URL}/v1/query`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ query: `SHOW table FROM vlogs ${whereClause} WITHIN last ${timeRange}` }),
      });
      const json: QueryResponse = await resp.json();

      // SHOW table format returns { columns, rows, format: "table" }
      // Raw vlogs returns { result: [...], result_type: "logs" }
      let entries: LogEntry[] = [];

      if (json.status === 'success' && json.data) {
        if (json.data.format === 'table' && json.data.columns && json.data.rows) {
          // Table format → reconstruct objects from columns + rows
          const cols = json.data.columns;
          entries = json.data.rows.slice(0, 20).map((row) => {
            const obj: LogEntry = {};
            cols.forEach((col, i) => {
              obj[col] = (row as unknown[])[i] as string;
            });
            return obj;
          });
        } else if (json.data.result && json.data.result.length > 0) {
          entries = json.data.result.slice(0, 20);
        }
      }

      if (entries.length > 0) {
        setLogs(entries);
        setLogState({
          native: json.metadata?.native_query,
          ms: json.metadata?.total_time_ms,
          fallback: false,
        });
      } else {
        setLogs([]);
        setLogState({
          native: json.metadata?.native_query,
          ms: json.metadata?.total_time_ms,
          fallback: true,
        });
      }
    } catch {
      setLogs([]);
      setLogState({ fallback: true, error: 'Engine unreachable' });
    }
  }, [timeRange, logSource]);

  // --- Fetch all ---
  const fetchAll = useCallback(async () => {
    setWidgetStates((prev) => {
      const next = { ...prev };
      for (const w of WIDGETS) {
        next[w.id] = { ...(next[w.id] ?? { display: '--', numeric: 0, history: [] }), loading: true };
      }
      return next;
    });

    await Promise.all([
      fetchHealth(),
      ...WIDGETS.map((w) => fetchWidget(w)),
      fetchLogs(),
    ]);

    setLastRefresh(new Date());
    setCountdown(REFRESH_INTERVAL / 1000);
  }, [fetchHealth, fetchWidget, fetchLogs]);

  // --- Timers ---
  useEffect(() => {
    fetchAll();
    refreshRef.current = setInterval(fetchAll, REFRESH_INTERVAL);
    return () => {
      if (refreshRef.current) clearInterval(refreshRef.current);
    };
  }, [fetchAll]);

  useEffect(() => {
    countdownRef.current = setInterval(() => {
      setCountdown((prev) => (prev <= 1 ? REFRESH_INTERVAL / 1000 : prev - 1));
    }, 1000);
    return () => {
      if (countdownRef.current) clearInterval(countdownRef.current);
    };
  }, []);

  // --- Derived stats for hero banner ---
  const snmpCount = widgetStates.snmp?.display ?? '--';
  const vmCount = widgetStates.vms?.display ?? '--';
  const serviceCount = widgetStates.services?.display ?? '--';
  const avgLatency = (() => {
    const times = WIDGETS.map((w) => widgetStates[w.id]?.totalMs).filter(
      (t): t is number => t != null
    );
    if (!times.length) return null;
    return Math.round(times.reduce((a, b) => a + b, 0) / times.length);
  })();

  return (
    <div className="space-y-4 pt-4 animate-fade-in">
      {/* Title bar */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <span className="text-xs font-semibold text-[var(--color-green)] uppercase tracking-wider">
            AETHERIS Live
          </span>
          <span className="relative flex items-center justify-center w-2.5 h-2.5">
            <span className="absolute w-full h-full rounded-full bg-[var(--color-green)]" style={{ animation: 'pulse-ring 2s ease-out infinite' }} />
            <span className="w-2 h-2 rounded-full bg-[var(--color-green)]" />
          </span>
        </div>
        <div className="flex items-center gap-4 text-[10px] text-[var(--color-text-dim)] font-mono">
          <span className="flex items-center gap-1.5">
            <span>Next refresh:</span>
            <span className="text-[var(--color-accent)] tabular-nums min-w-[1.5ch] text-right">{countdown}s</span>
          </span>
          {lastRefresh && <span>{lastRefresh.toLocaleTimeString('tr-TR')}</span>}
        </div>
      </div>

      {/* Platform Stats Hero Banner */}
      <div className="rounded-lg border border-[var(--color-border)] bg-gradient-to-r from-[var(--color-surface-2)] to-[var(--color-surface-3)] p-4">
        <div className="flex items-center justify-between mb-3">
          <span className="text-[10px] font-semibold text-[var(--color-text-dim)] uppercase tracking-wider">
            Platform Overview
          </span>
          {health && (
            <span className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-[10px] font-semibold bg-[var(--color-green-dim)] text-[var(--color-green)]">
              <span className="w-1.5 h-1.5 rounded-full bg-[var(--color-green)]" />
              Engine v{health.version}
            </span>
          )}
        </div>
        <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
          <HeroStat value={snmpCount} label="SNMP Devices" color="#39d0d8" icon="N" />
          <HeroStat value={vmCount} label="Virtual Machines" color="#7c5cfc" icon="V" />
          <HeroStat value={serviceCount} label="Services" color="#3fb950" icon="S" />
          <HeroStat value={avgLatency != null ? `${avgLatency}` : '--'} label="Avg Engine Latency" color="#d29922" icon="~" unit="ms" />
        </div>
      </div>

      {/* Metric widget cards */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
        {WIDGETS.map((w) => {
          const s = widgetStates[w.id];
          const isExpanded = expanded === w.id;
          return (
            <div
              key={w.id}
              className={`rounded-lg border bg-[var(--color-surface-2)] p-4 cursor-pointer transition-all ${
                isExpanded
                  ? 'border-[var(--color-accent)]/40 ring-1 ring-[var(--color-accent)]/20'
                  : 'border-[var(--color-border)] hover:border-[var(--color-border-bright)]'
              }`}
              onClick={() => setExpanded(isExpanded ? null : w.id)}
            >
              {/* Header row */}
              <div className="flex items-center justify-between mb-2">
                <div className="flex items-center gap-2">
                  <span
                    className="w-5 h-5 rounded flex items-center justify-center text-[9px] font-bold"
                    style={{
                      background: `${w.color}18`,
                      color: w.color,
                      border: `1px solid ${w.color}40`,
                    }}
                  >
                    {w.icon}
                  </span>
                  <span className="text-[10px] text-[var(--color-text-dim)] uppercase tracking-wider">
                    {w.title}
                  </span>
                </div>
                <div className="flex items-center gap-1.5">
                  {s?.loading && (
                    <span className="w-1.5 h-1.5 rounded-full bg-[var(--color-amber)] animate-pulse" />
                  )}
                  {s?.totalMs != null && (
                    <span className="text-[9px] text-[var(--color-text-dim)] font-mono">{s.totalMs}ms</span>
                  )}
                </div>
              </div>

              {/* Value */}
              <div className="flex items-baseline gap-1.5">
                <span
                  className="text-2xl font-bold font-mono"
                  style={{ color: s?.error ? 'var(--color-red)' : 'var(--color-text-bright)' }}
                >
                  {s?.display ?? '--'}
                </span>
                {w.unit && <span className="text-xs text-[var(--color-text-dim)]">{w.unit}</span>}
              </div>

              {/* Trend chart: range data (1h) or fallback to history sparkline */}
              {s?.rangeData && s.rangeData.length > 1 ? (
                <AreaChart data={s.rangeData} color={w.color} height={48} />
              ) : s?.history && s.history.length > 1 ? (
                <Spark data={s.history} color={w.color} />
              ) : null}

              {/* Expanded query details */}
              {isExpanded && s && (
                <div className="mt-3 pt-3 border-t border-[var(--color-border)] space-y-2 text-[10px]">
                  <div className="text-[var(--color-text-dim)] mb-1">{w.description}</div>
                  {s.rangeData.length > 0 && (
                    <div className="text-[var(--color-text-dim)]">
                      Trend: {s.rangeData.length} data points (WITHIN last 1h)
                    </div>
                  )}
                  <div>
                    <span className="text-[var(--color-text-dim)]">UNIQL: </span>
                    <span className="text-[var(--color-accent)] font-mono break-all">{w.uniql}</span>
                  </div>
                  {w.rangeQuery && (
                    <div>
                      <span className="text-[var(--color-text-dim)]">Range: </span>
                      <span className="text-[var(--color-cyan)] font-mono break-all">{w.rangeQuery}</span>
                    </div>
                  )}
                  {s.native && (
                    <div>
                      <span className="text-[var(--color-text-dim)]">Native: </span>
                      <span className="text-[var(--color-cyan)] font-mono break-all">{s.native}</span>
                    </div>
                  )}
                  <div className="flex gap-3 text-[var(--color-text-dim)] font-mono">
                    {s.parseUs != null && <span>parse: {s.parseUs}us</span>}
                    {s.transpileUs != null && <span>transpile: {s.transpileUs}us</span>}
                    {s.executeMs != null && <span>execute: {s.executeMs}ms</span>}
                    {s.totalMs != null && <span className="text-[var(--color-green)]">total: {s.totalMs}ms</span>}
                  </div>
                  {s.error && <div className="text-[var(--color-red)] font-mono">{s.error}</div>}
                </div>
              )}
            </div>
          );
        })}
      </div>

      {/* Unified Feed: Metrics + Logs interleaved — Write Once, Query Everything */}
      <UnifiedFeed />

      {/* Log stream */}
      <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
        <div className="flex items-center justify-between px-4 py-2 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
          <div className="flex items-center gap-3">
            <span className="text-xs font-semibold text-[var(--color-amber)] tracking-wider">LOG STREAM</span>
            <div className="flex items-center gap-0.5">
              {LOG_SOURCES.map((src) => (
                <button
                  key={src.id}
                  onClick={() => setLogSource(src.id)}
                  className={`px-2 py-0.5 rounded text-[9px] font-semibold cursor-pointer transition-all ${
                    logSource === src.id
                      ? 'text-white'
                      : 'text-[var(--color-text-dim)] hover:text-[var(--color-text)]'
                  }`}
                  style={logSource === src.id ? { background: `${src.color}30`, color: src.color, border: `1px solid ${src.color}50` } : { border: '1px solid transparent' }}
                >
                  {src.label}
                </button>
              ))}
            </div>
            {!logState.fallback && (
              <span className="text-[10px] text-[var(--color-green)] font-mono">{logs.length} entries</span>
            )}
          </div>
          <div className="flex items-center gap-3 text-[10px] text-[var(--color-text-dim)] font-mono">
            <div className="flex items-center gap-1">
              {TIME_RANGES.map((t) => (
                <button
                  key={t.value}
                  onClick={() => setTimeRange(t.value)}
                  className={`px-1.5 py-0.5 rounded text-[9px] cursor-pointer transition-all ${
                    timeRange === t.value
                      ? 'bg-[var(--color-amber-dim)] text-[var(--color-amber)] border border-[var(--color-amber)]/30'
                      : 'text-[var(--color-text-dim)] hover:text-[var(--color-text)] border border-transparent'
                  }`}
                >
                  {t.label}
                </button>
              ))}
            </div>
            {logState.ms != null && <span>{logState.ms}ms</span>}
            {logState.native && (
              <span className="text-[var(--color-cyan)] max-w-[300px] truncate">{logState.native}</span>
            )}
          </div>
        </div>
        <div className="max-h-72 overflow-y-auto divide-y divide-[var(--color-border)]/50">
          {logState.fallback ? (
            <div className="p-6 text-center">
              <div className="text-[var(--color-text-dim)] text-xs font-semibold mb-1">No logs in selected range</div>
              <div className="text-[var(--color-text-dim)] text-[11px]">
                No FortiGate logs found in the last {timeRange}. Try a wider time range.
              </div>
              {logState.error && (
                <div className="mt-2 text-[10px] text-[var(--color-red)]">{logState.error}</div>
              )}
            </div>
          ) : logs.length === 0 ? (
            <div className="p-4 text-center text-[var(--color-text-dim)] text-xs">Waiting for log data...</div>
          ) : (
            logs.map((entry, i) => {
              const time = entry._time as string | undefined;
              const action = entry.action as string | undefined;
              const srcIp = entry.source_ip as string | undefined;
              const subtype = entry.subtype as string | undefined;
              const job = entry.job as string | undefined;
              const source = entry.source as string | undefined;
              const msg = entry._msg as string | undefined;
              const msgPreview = msg && msg.length > 180 ? msg.slice(0, 180) + '...' : msg;

              const jobColor = job === 'fortigate' ? 'var(--color-amber)' : job === 'fsso' ? 'var(--color-cyan)' : 'var(--color-text-dim)';

              return (
                <div key={i} className="px-4 py-1.5 hover:bg-[var(--color-surface-3)] transition-colors flex items-start gap-2">
                  {time && (
                    <span className="text-[10px] text-[var(--color-text-dim)] font-mono shrink-0 w-[52px] pt-0.5">
                      {new Date(time).toLocaleTimeString('tr-TR', { hour: '2-digit', minute: '2-digit', second: '2-digit' })}
                    </span>
                  )}
                  {/* Source badge */}
                  {logSource === 'all' && job && (
                    <span className="text-[8px] font-mono px-1 py-0.5 rounded shrink-0" style={{ background: `${jobColor}18`, color: jobColor, border: `1px solid ${jobColor}40` }}>
                      {job}
                    </span>
                  )}
                  {/* FortiGate fields */}
                  {action && (
                    <span className={`text-[9px] font-mono px-1 py-0.5 rounded shrink-0 ${
                      action === 'deny' ? 'bg-[var(--color-red)]/15 text-[var(--color-red)]'
                        : action === 'accept' ? 'bg-[var(--color-green)]/15 text-[var(--color-green)]'
                        : action === 'close' ? 'bg-[var(--color-text-dim)]/10 text-[var(--color-text-dim)]'
                        : 'bg-[var(--color-surface-3)] text-[var(--color-text-dim)]'
                    }`}>
                      {action}
                    </span>
                  )}
                  {subtype && (
                    <span className="text-[9px] text-[var(--color-cyan)] font-mono shrink-0">{subtype}</span>
                  )}
                  {srcIp && (
                    <span className="text-[9px] text-[var(--color-amber)] font-mono shrink-0">{srcIp}</span>
                  )}
                  {/* FSSO fields */}
                  {!action && source && (
                    <span className="text-[9px] text-[var(--color-cyan)] font-mono shrink-0">{source}</span>
                  )}
                  <span className="text-[10px] text-[var(--color-text)] font-mono break-all leading-relaxed min-w-0">
                    {msgPreview ?? JSON.stringify(entry).slice(0, 150)}
                  </span>
                </div>
              );
            })
          )}
        </div>
      </div>

      {/* Engine backends status row */}
      {health && (
        <div className="flex items-center gap-3 text-[10px] text-[var(--color-text-dim)] font-mono">
          <span className="uppercase tracking-wider">Backends:</span>
          {health.backends.map((b) => (
            <span key={b.name} className="inline-flex items-center gap-1">
              <span className={`w-1.5 h-1.5 rounded-full ${b.status === 'reachable' ? 'bg-[var(--color-green)]' : 'bg-[var(--color-red)]'}`} />
              <span>{b.name}</span>
              <span className="text-[var(--color-text-dim)]">({b.type})</span>
            </span>
          ))}
          {engineLatency != null && (
            <span className="ml-auto text-[var(--color-text-dim)]">Health RTT: {engineLatency}ms</span>
          )}
        </div>
      )}
    </div>
  );
}

// --- Hero stat component ---

function HeroStat({ value, label, color, icon, unit }: { value: string; label: string; color: string; icon: string; unit?: string }) {
  return (
    <div className="flex items-center gap-3">
      <div
        className="w-9 h-9 rounded-lg flex items-center justify-center text-sm font-bold shrink-0"
        style={{
          background: `${color}18`,
          color,
          border: `1px solid ${color}40`,
        }}
      >
        {icon}
      </div>
      <div>
        <div className="flex items-baseline gap-1">
          <span className="text-xl font-bold font-mono text-[var(--color-text-bright)]">{value}</span>
          {unit && <span className="text-[10px] text-[var(--color-text-dim)]">{unit}</span>}
        </div>
        <div className="text-[10px] text-[var(--color-text-dim)] uppercase tracking-wider">{label}</div>
      </div>
    </div>
  );
}

// --- Sparkline component ---

function Spark({ data, color = 'var(--color-accent)' }: { data: number[]; color?: string }) {
  const min = Math.min(...data);
  const max = Math.max(...data);
  const range = max - min || 1;
  const w = 90;
  const h = 24;
  const points = data
    .map((v, i) => {
      const x = (i / (data.length - 1)) * w;
      const y = h - ((v - min) / range) * (h - 4) - 2;
      return `${x},${y}`;
    })
    .join(' ');
  const fillPoints = `0,${h} ${points} ${w},${h}`;
  const gradId = `spark-grad-${color.replace(/[^a-z0-9]/gi, '')}`;

  return (
    <svg width={w} height={h} className="mt-2">
      <defs>
        <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity="0.2" />
          <stop offset="100%" stopColor={color} stopOpacity="0" />
        </linearGradient>
      </defs>
      <polygon points={fillPoints} fill={`url(#${gradId})`} />
      <polyline points={points} fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" opacity="0.8" />
      {data.length > 0 && (
        <circle cx={w} cy={h - ((data[data.length - 1] - min) / range) * (h - 4) - 2} r="2" fill={color} />
      )}
    </svg>
  );
}

// --- Unified Stream: real-time feed with append-on-top behavior ---

interface StreamItem {
  id: string;
  type: 'metric' | 'log';
  source: string;
  color: string;
  ts: string;
  content: string;
  value?: string;
  badge?: string;
  badgeColor?: string;
  fresh: boolean;
}

const MAX_STREAM_ITEMS = 80;
const STREAM_POLL_MS = 3000;

function UnifiedFeed() {
  const [items, setItems] = useState<StreamItem[]>([]);
  const [paused, setPaused] = useState(false);
  const [stats, setStats] = useState({ queries: 0, ms: 0, metrics: 0, logs: 0 });
  const seenRef = useRef<Set<string>>(new Set());
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (paused) return;

    const poll = async () => {
      const start = performance.now();

      const [esxiRes, svcRes, fgtRes, fssoRes] = await Promise.allSettled([
        fetch(`${ENGINE_URL}/v1/query`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ query: 'SHOW timeseries FROM victoria WHERE __name__ = "vsphere_host_cpu_usage_average"' }) }).then(r => r.json()),
        fetch(`${ENGINE_URL}/v1/query`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ query: 'SHOW timeseries FROM victoria WHERE __name__ = "up"' }) }).then(r => r.json()),
        fetch(`${ENGINE_URL}/v1/query`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ query: 'SHOW table FROM vlogs WHERE job = "fortigate" WITHIN last 5s', limit: 10 }) }).then(r => r.json()),
        fetch(`${ENGINE_URL}/v1/query`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ query: 'SHOW table FROM vlogs WHERE job = "fsso" WITHIN last 5s', limit: 5 }) }).then(r => r.json()),
      ]);

      const newItems: StreamItem[] = [];
      const now = new Date();
      const ts = now.toLocaleTimeString('tr-TR', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
      let mCount = 0, lCount = 0;

      // ESXi — top 3 by CPU
      if (esxiRes.status === 'fulfilled') {
        const results: MetricResult[] = esxiRes.value?.data?.data?.result ?? [];
        const sorted = [...results].sort((a, b) => parseFloat(b.value?.[1] || '0') - parseFloat(a.value?.[1] || '0'));
        for (const r of sorted.slice(0, 3)) {
          const cpu = parseFloat(r.value?.[1] || '0');
          const host = r.metric.esxhostname || '?';
          const id = `esxi-${host}-${ts}`;
          if (!seenRef.current.has(id)) {
            seenRef.current.add(id);
            newItems.push({
              id, type: 'metric', source: 'vCenter', color: '#7c5cfc', ts,
              content: host, value: cpu.toFixed(1) + '%',
              badge: cpu > 50 ? 'HIGH' : cpu > 20 ? 'MED' : 'OK',
              badgeColor: cpu > 50 ? 'var(--color-red)' : cpu > 20 ? 'var(--color-amber)' : 'var(--color-green)',
              fresh: true,
            });
            mCount++;
          }
        }
      }

      // Services
      if (svcRes.status === 'fulfilled') {
        const results: MetricResult[] = svcRes.value?.data?.data?.result ?? [];
        const up = results.filter(r => r.value?.[1] === '1').length;
        const id = `svc-${ts}`;
        if (!seenRef.current.has(id)) {
          seenRef.current.add(id);
          newItems.push({
            id, type: 'metric', source: 'Platform', color: '#3fb950', ts,
            content: `${up}/${results.length} services`,
            value: up === results.length ? 'ALL UP' : `${results.length - up} DOWN`,
            badge: up === results.length ? 'OK' : 'ALERT',
            badgeColor: up === results.length ? 'var(--color-green)' : 'var(--color-red)',
            fresh: true,
          });
          mCount++;
        }
      }

      // FortiGate logs
      if (fgtRes.status === 'fulfilled') {
        const data = fgtRes.value?.data;
        const cols: string[] = data?.columns ?? [];
        const rows: unknown[][] = data?.rows ?? [];
        const mi = cols.indexOf('_msg'), ai = cols.indexOf('action'), ti = cols.indexOf('_time');
        const si = cols.indexOf('subtype'), ipi = cols.indexOf('source_ip');
        for (const row of rows) {
          const rawTs = ti >= 0 ? (row[ti] as string) : '';
          const logTs = rawTs ? new Date(rawTs).toLocaleTimeString('tr-TR', { hour: '2-digit', minute: '2-digit', second: '2-digit' }) : ts;
          const action = ai >= 0 ? (row[ai] as string) : '';
          const sub = si >= 0 ? (row[si] as string) : '';
          const ip = ipi >= 0 ? (row[ipi] as string) : '';
          const msg = mi >= 0 ? (row[mi] as string || '').slice(0, 90) : '';
          const id = `fgt-${rawTs || Math.random()}`;
          if (!seenRef.current.has(id)) {
            seenRef.current.add(id);
            newItems.push({
              id, type: 'log', source: 'FortiGate', color: '#e09c5e', ts: logTs,
              content: `${sub} ${ip} ${msg}`.trim(),
              badge: action || undefined,
              badgeColor: action === 'deny' ? 'var(--color-red)' : action === 'accept' ? 'var(--color-green)' : 'var(--color-text-dim)',
              fresh: true,
            });
            lCount++;
          }
        }
      }

      // FSSO logs
      if (fssoRes.status === 'fulfilled') {
        const data = fssoRes.value?.data;
        const cols: string[] = data?.columns ?? [];
        const rows: unknown[][] = data?.rows ?? [];
        const mi = cols.indexOf('_msg'), ti = cols.indexOf('_time');
        for (const row of rows) {
          const rawTs = ti >= 0 ? (row[ti] as string) : '';
          const logTs = rawTs ? new Date(rawTs).toLocaleTimeString('tr-TR', { hour: '2-digit', minute: '2-digit', second: '2-digit' }) : ts;
          const msg = mi >= 0 ? (row[mi] as string || '').slice(0, 100) : '';
          const id = `fsso-${rawTs || Math.random()}`;
          if (!seenRef.current.has(id)) {
            seenRef.current.add(id);
            newItems.push({
              id, type: 'log', source: 'FSSO', color: '#40c8d0', ts: logTs,
              content: msg, badge: 'SSO', badgeColor: 'var(--color-cyan)', fresh: true,
            });
            lCount++;
          }
        }
      }

      // Trim seen set
      if (seenRef.current.size > 500) {
        const arr = Array.from(seenRef.current);
        seenRef.current = new Set(arr.slice(-300));
      }

      if (newItems.length > 0) {
        setItems(prev => {
          const unfreshed = prev.map(i => ({ ...i, fresh: false }));
          return [...newItems, ...unfreshed].slice(0, MAX_STREAM_ITEMS);
        });
      }

      setStats({ queries: 4, ms: Math.round(performance.now() - start), metrics: mCount, logs: lCount });
    };

    poll();
    const interval = setInterval(poll, STREAM_POLL_MS);
    return () => clearInterval(interval);
  }, [paused]);

  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
      <div className="flex items-center justify-between px-4 py-2 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
        <div className="flex items-center gap-3">
          <span className="text-xs font-semibold text-[var(--color-accent)] tracking-wider">UNIFIED STREAM</span>
          <span className="relative flex items-center justify-center w-2 h-2">
            <span className="absolute w-full h-full rounded-full bg-[var(--color-green)]" style={{ animation: paused ? 'none' : 'pulse-ring 2s ease-out infinite' }} />
            <span className={`w-1.5 h-1.5 rounded-full ${paused ? 'bg-[var(--color-text-dim)]' : 'bg-[var(--color-green)]'}`} />
          </span>
          <span className="text-[10px] text-[var(--color-text-dim)] font-mono">{items.length} events</span>
        </div>
        <div className="flex items-center gap-3 text-[10px] font-mono">
          <button
            onClick={() => setPaused(p => !p)}
            className="px-2 py-0.5 rounded text-[9px] font-semibold cursor-pointer transition-all border"
            style={{
              color: paused ? 'var(--color-amber)' : 'var(--color-text-dim)',
              borderColor: paused ? 'var(--color-amber)' : 'var(--color-border)',
              background: paused ? 'var(--color-amber-dim)' : 'transparent',
            }}
          >
            {paused ? 'PAUSED' : 'PAUSE'}
          </button>
          <span className="text-[var(--color-text-dim)]">{stats.ms}ms</span>
          <span className="flex items-center gap-1 text-[var(--color-accent)]"><span className="w-1.5 h-1.5 rounded-full bg-[var(--color-accent)]" />PromQL</span>
          <span className="flex items-center gap-1 text-[var(--color-amber)]"><span className="w-1.5 h-1.5 rounded-full bg-[var(--color-amber)]" />LogsQL</span>
        </div>
      </div>
      <div ref={containerRef} className="max-h-80 overflow-y-auto">
        {items.map((item) => (
          <div
            key={item.id}
            className={`px-4 py-1 flex items-center gap-2 border-b border-[var(--color-border)]/15 transition-all duration-500 ${
              item.fresh ? 'bg-[var(--color-accent)]/8' : 'hover:bg-[var(--color-surface-3)]'
            }`}
          >
            <span className={`w-1 h-3.5 rounded-full shrink-0 ${item.type === 'metric' ? 'bg-[var(--color-accent)]' : 'bg-[var(--color-amber)]'}`} />
            <span className="text-[9px] text-[var(--color-text-dim)] font-mono shrink-0 w-[50px]">{item.ts}</span>
            <span className="text-[8px] font-bold font-mono px-1.5 py-0.5 rounded shrink-0 min-w-[54px] text-center" style={{ background: `${item.color}15`, color: item.color, border: `1px solid ${item.color}35` }}>
              {item.source}
            </span>
            {item.badge && (
              <span className="text-[8px] font-bold font-mono px-1 py-0.5 rounded shrink-0" style={{ background: `${item.badgeColor}15`, color: item.badgeColor }}>
                {item.badge}
              </span>
            )}
            <span className="text-[10px] text-[var(--color-text)] font-mono truncate flex-1 min-w-0">{item.content}</span>
            {item.value && (
              <span className="text-[10px] font-bold font-mono shrink-0 tabular-nums" style={{ color: item.color }}>{item.value}</span>
            )}
          </div>
        ))}
        {items.length === 0 && (
          <div className="p-8 text-center text-[var(--color-text-dim)] text-xs">Waiting for data...</div>
        )}
      </div>
    </div>
  );
}

// --- Area chart for range data (WITHIN last 1h) ---

function AreaChart({ data, color, height = 48 }: { data: [number, string][]; color: string; height?: number }) {
  if (data.length < 2) return null;

  const values = data.map(([, v]) => parseFloat(v));
  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;
  const w = 220;
  const h = height;
  const pad = 2;

  const points = values
    .map((v, i) => {
      const x = (i / (values.length - 1)) * w;
      const y = h - ((v - min) / range) * (h - pad * 2) - pad;
      return `${x},${y}`;
    })
    .join(' ');
  const fillPoints = `0,${h} ${points} ${w},${h}`;
  const gradId = `area-grad-${color.replace(/[^a-z0-9]/gi, '')}-${data.length}`;

  // Time labels
  const firstTs = data[0][0];
  const lastTs = data[data.length - 1][0];
  const firstTime = new Date(firstTs * 1000).toLocaleTimeString('tr-TR', { hour: '2-digit', minute: '2-digit' });
  const lastTime = new Date(lastTs * 1000).toLocaleTimeString('tr-TR', { hour: '2-digit', minute: '2-digit' });
  const lastVal = values[values.length - 1];

  return (
    <div className="mt-2 relative">
      <svg width="100%" height={h} viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none">
        <defs>
          <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={color} stopOpacity="0.25" />
            <stop offset="100%" stopColor={color} stopOpacity="0.02" />
          </linearGradient>
        </defs>
        <polygon points={fillPoints} fill={`url(#${gradId})`} />
        <polyline points={points} fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" opacity="0.9" />
        <circle
          cx={w}
          cy={h - ((lastVal - min) / range) * (h - pad * 2) - pad}
          r="2.5"
          fill={color}
        />
      </svg>
      <div className="flex items-center justify-between mt-0.5">
        <span className="text-[8px] text-[var(--color-text-dim)] font-mono">{firstTime}</span>
        <span className="text-[8px] font-mono" style={{ color }}>{data.length} pts</span>
        <span className="text-[8px] text-[var(--color-text-dim)] font-mono">{lastTime}</span>
      </div>
    </div>
  );
}
