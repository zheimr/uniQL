export interface Scenario {
  id: string;
  title: string;
  icon: string;
  device: string;
  query: string;
  description: string;
  backend: 'promql' | 'logql' | 'logsql';
  category: 'basics' | 'filtering' | 'aggregation' | 'logs' | 'advanced';
}

export const scenarios: Scenario[] = [
  // ─── Basics ────────────────────────────────────────────────────
  {
    id: 'service-health',
    title: 'Service Health',
    icon: '💚',
    device: 'platform',
    category: 'basics',
    query: `SHOW timeseries FROM victoria
WHERE __name__ = "up"`,
    description: 'Simple metric query — all platform services',
    backend: 'promql',
  },
  {
    id: 'within-range',
    title: 'Time Range',
    icon: '🕐',
    device: 'platform',
    category: 'basics',
    query: `SHOW timeseries FROM victoria
WHERE __name__ = "up"
WITHIN last 1h`,
    description: 'WITHIN clause → query_range with auto step',
    backend: 'promql',
  },
  {
    id: 'show-table',
    title: 'Table Format',
    icon: '📋',
    device: 'platform',
    category: 'basics',
    query: `SHOW table FROM victoria
WHERE __name__ = "up"`,
    description: 'SHOW table → structured columns + rows',
    backend: 'promql',
  },

  // ─── Filtering ─────────────────────────────────────────────────
  {
    id: 'regex-match',
    title: 'Regex Match',
    icon: '🔍',
    device: 'snmp',
    category: 'filtering',
    query: `FROM metrics
WHERE __name__ = "snmpv2_device_up"
  AND hostname =~ "CORE.*"`,
    description: 'Regex label matching with =~',
    backend: 'promql',
  },
  {
    id: 'in-list',
    title: 'IN List',
    icon: '📝',
    device: 'platform',
    category: 'filtering',
    query: `FROM metrics
WHERE __name__ = "up"
  AND job IN ["admin-api", "traefik", "postgresql"]`,
    description: 'IN operator → regex union',
    backend: 'promql',
  },
  {
    id: 'cross-field-or',
    title: 'Cross-Field OR',
    icon: '🔀',
    device: 'platform',
    category: 'filtering',
    query: `FROM metrics
WHERE __name__ = "up" AND job = "admin-api"
   OR __name__ = "up" AND job = "traefik"`,
    description: 'Cross-field OR → PromQL binary "or" operator',
    backend: 'promql',
  },

  // ─── Aggregation ───────────────────────────────────────────────
  {
    id: 'compute-count',
    title: 'Count + Group By',
    icon: '📊',
    device: 'platform',
    category: 'aggregation',
    query: `FROM metrics
WHERE __name__ = "up"
COMPUTE count()
GROUP BY job`,
    description: 'count() aggregation grouped by job',
    backend: 'promql',
  },
  {
    id: 'compute-rate',
    title: 'Rate + Group By',
    icon: '📈',
    device: 'vcenter',
    category: 'aggregation',
    query: `FROM metrics
WHERE __name__ = "vsphere_host_cpu_usage_average"
COMPUTE avg(value)
GROUP BY esxhostname
WITHIN last 1h`,
    description: 'Average CPU per ESXi host over 1 hour',
    backend: 'promql',
  },
  {
    id: 'percentile',
    title: 'Percentile (p99)',
    icon: '🎯',
    device: 'platform',
    category: 'aggregation',
    query: `FROM metrics
WHERE __name__ = "http_request_duration_seconds_bucket"
COMPUTE p99(value)
WITHIN last 30m`,
    description: 'p99 latency → histogram_quantile(0.99, rate(...))',
    backend: 'promql',
  },

  // ─── Logs ──────────────────────────────────────────────────────
  {
    id: 'fortigate-logs',
    title: 'FortiGate Logs',
    icon: '🔥',
    device: 'fortigate',
    category: 'logs',
    query: `SHOW table FROM vlogs
WHERE job = "fortigate"
WITHIN last 5m`,
    description: 'Firewall syslog via VictoriaLogs',
    backend: 'logsql',
  },
  {
    id: 'log-contains',
    title: 'Log Search',
    icon: '🔎',
    device: 'fortigate',
    category: 'logs',
    query: `FROM logs
WHERE job = "fortigate"
  AND message CONTAINS "deny"
WITHIN last 15m`,
    description: 'CONTAINS → content filter in logs',
    backend: 'logsql',
  },
  {
    id: 'parse-json',
    title: 'Parse JSON',
    icon: '🔧',
    device: 'fortigate',
    category: 'logs',
    query: `FROM logs
WHERE job = "fortigate"
PARSE json
WITHIN last 5m`,
    description: 'PARSE json → unpack_json pipe in LogsQL',
    backend: 'logsql',
  },
  {
    id: 'log-count',
    title: 'Log Aggregation',
    icon: '🧮',
    device: 'fortigate',
    category: 'logs',
    query: `FROM logs
WHERE job = "fortigate"
COMPUTE count()
GROUP BY level
WITHIN last 1h`,
    description: 'Count logs by level → stats pipe in LogsQL',
    backend: 'logsql',
  },

  // ─── Advanced ──────────────────────────────────────────────────
  {
    id: 'correlate',
    title: 'CORRELATE',
    icon: '🔗',
    device: 'uniql',
    category: 'advanced',
    query: `FROM metrics, logs
CORRELATE ON host
WITHIN 60s`,
    description: 'Cross-signal join — metrics + logs on shared field',
    backend: 'promql',
  },
  {
    id: 'define-macro',
    title: 'DEFINE Macro',
    icon: '🔁',
    device: 'vcenter',
    category: 'advanced',
    query: `DEFINE high_cpu = __name__ = "vsphere_host_cpu_usage_average"
FROM metrics WHERE high_cpu
  AND clustername = "Production_Cluster"`,
    description: 'Reusable macro — DEFINE/USE pattern',
    backend: 'promql',
  },
  {
    id: 'native-passthrough',
    title: 'NATIVE Passthrough',
    icon: '⚡',
    device: 'platform',
    category: 'advanced',
    query: `NATIVE("promql", "rate(http_requests_total[5m])")`,
    description: 'Direct backend query — bypass UniQL transpiler',
    backend: 'promql',
  },
  {
    id: 'complex-query',
    title: 'Full Pipeline',
    icon: '🚀',
    device: 'vcenter',
    category: 'advanced',
    query: `SHOW timeseries FROM victoria
WHERE __name__ = "vsphere_host_cpu_usage_average"
  AND clustername = "Production_Cluster"
WITHIN last 1h`,
    description: 'SHOW + FROM + WHERE + WITHIN — full clause chain',
    backend: 'promql',
  },
];

export const investigationSteps = [
  {
    id: 1,
    title: 'Alert Triggered',
    icon: '🔴',
    description: 'ESXi host CPU > 85% — AETHERIS vmalert rule',
    detail: 'vmalert detected: vsphere_host_cpu_usage_average > 85 for 5 minutes. Host: esxi-node01.example.com, Cluster: Production_Cluster.',
    query: `SHOW timeseries FROM victoria WHERE __name__ = "vsphere_host_cpu_usage_average"`,
  },
  {
    id: 2,
    title: 'Investigation Pack Started',
    icon: '📦',
    description: 'UniQL "high_cpu" pack — 3 parallel queries',
    detail: 'high_cpu investigation pack active. CPU trend + Top VM by CPU + Host memory queries executed in parallel.',
    query: null,
  },
  {
    id: 3,
    title: 'Parallel Queries Complete',
    icon: '⚡',
    description: 'CPU trend | Top VMs | Memory correlation',
    detail: 'Query 1: vsphere_host_cpu → spike at 14:32, 92% peak\nQuery 2: vsphere_vm_cpu top 5 → test-vm-01 highest\nQuery 3: vsphere_host_mem → 67%, normal range',
    query: `SHOW timeseries FROM victoria WHERE __name__ = "vsphere_vm_cpu_usage_average"`,
  },
  {
    id: 4,
    title: 'Correlation Analysis',
    icon: '🧩',
    description: 'Host + VM + time window match found',
    detail: 'CORRELATE ON host WITHIN 60s: Host CPU spike (14:32, 92%) + VM "test-vm-01" CPU (14:31, 100%) on same host, same time window. VM caused the host spike.',
    query: null,
  },
  {
    id: 5,
    title: 'Root Cause Identified',
    icon: '✅',
    description: 'test-vm-01 VM — CPU runaway, started at 14:31',
    detail: 'Root Cause: test-vm-01 VM on esxi-node01.example.com reached 100% CPU at 14:31, causing host-level CPU spike. Recommended action: VM CPU limit or rightsizing.',
    query: null,
  },
];
