import { useState } from 'react';

type Section = 'syntax' | 'api' | 'examples' | 'backends';

export default function DocsTab() {
  const [section, setSection] = useState<Section>('syntax');

  return (
    <div className="pt-4 animate-fade-in">
      <div className="flex gap-6 max-w-[1200px] mx-auto">
        {/* Sidebar */}
        <nav className="w-48 shrink-0 sticky top-16 self-start space-y-1">
          <div className="text-[10px] text-[var(--color-text-dim)] uppercase tracking-wider mb-2 px-3">Reference</div>
          {([
            { id: 'syntax', label: 'Query Syntax' },
            { id: 'api', label: 'API Reference' },
            { id: 'examples', label: 'Examples' },
            { id: 'backends', label: 'Backends' },
          ] as { id: Section; label: string }[]).map(s => (
            <button
              key={s.id}
              onClick={() => setSection(s.id)}
              className={`w-full text-left px-3 py-1.5 rounded text-[13px] transition-all cursor-pointer ${
                section === s.id
                  ? 'text-[var(--color-text-bright)] bg-[var(--color-surface-3)]'
                  : 'text-[var(--color-text-dim)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-2)]'
              }`}
            >
              {s.label}
            </button>
          ))}
        </nav>

        {/* Content */}
        <div className="flex-1 min-w-0 pb-16">
          {section === 'syntax' && <SyntaxRef />}
          {section === 'api' && <ApiRef />}
          {section === 'examples' && <ExamplesRef />}
          {section === 'backends' && <BackendsRef />}
        </div>
      </div>
    </div>
  );
}

// ─── Shared components ───────────────────────────────────────────

function H2({ children }: { children: React.ReactNode }) {
  return <h2 className="text-xl font-bold text-[var(--color-text-bright)] mb-4 mt-8 first:mt-0">{children}</h2>;
}
function H3({ children }: { children: React.ReactNode }) {
  return <h3 className="text-sm font-bold text-[var(--color-text-bright)] mb-2 mt-6">{children}</h3>;
}
function P({ children }: { children: React.ReactNode }) {
  return <p className="text-[13px] text-[var(--color-text-dim)] mb-3 leading-relaxed">{children}</p>;
}
function Code({ children }: { children: string }) {
  return (
    <pre className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] p-4 mb-4 overflow-x-auto text-[12px] font-mono leading-relaxed text-[var(--color-text)]">
      {children.split('\n').map((line, i) => {
        const colored = line
          .replace(/\b(FROM|WHERE|AND|OR|WITHIN|COMPUTE|GROUP BY|HAVING|CORRELATE|ON|SHOW|PARSE|DEFINE|AS|IN|NOT|CONTAINS|STARTS WITH|MATCHES|NATIVE|last|today|this_week)\b/g, '\x01$1\x02')
          .replace(/"([^"]*)"/g, '\x03"$1"\x04');
        return (
          <span key={i}>
            {colored.split(/(\x01[^\x02]*\x02|\x03[^\x04]*\x04)/).map((part, j) => {
              if (part.startsWith('\x01')) return <span key={j} className="text-[var(--color-accent)]">{part.slice(1, -1)}</span>;
              if (part.startsWith('\x03')) return <span key={j} className="text-[var(--color-green)]">{part.slice(1, -1)}</span>;
              return <span key={j}>{part}</span>;
            })}
            {'\n'}
          </span>
        );
      })}
    </pre>
  );
}

function Table({ headers, rows }: { headers: string[]; rows: string[][] }) {
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden mb-4">
      <table className="w-full text-[12px]">
        <thead>
          <tr className="border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
            {headers.map(h => (
              <th key={h} className="text-left px-4 py-2 text-[var(--color-text-dim)] font-semibold">{h}</th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-[var(--color-border)]/30">
          {rows.map((row, i) => (
            <tr key={i}>
              {row.map((cell, j) => (
                <td key={j} className={`px-4 py-1.5 ${j === 0 ? 'font-mono text-[var(--color-accent)]' : 'text-[var(--color-text-dim)]'}`}>
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ─── SYNTAX REFERENCE ────────────────────────────────────────────

function SyntaxRef() {
  return (
    <>
      <H2>Query Syntax</H2>
      <P>UniQL queries follow a SQL-like structure. Every clause is optional except FROM.</P>

      <Code>{`[SHOW format] FROM signal[:backend] [AS alias]
[WHERE condition [AND|OR condition ...]]
[WITHIN time_range]
[PARSE mode]
[COMPUTE function(args) [AS alias]]
[GROUP BY field [, field ...]]
[HAVING condition]
[CORRELATE ON field [, field ...] [WITHIN duration]]`}</Code>

      <H3>FROM</H3>
      <P>Specifies the signal type and optional backend hint.</P>
      <Table
        headers={['Syntax', 'Description']}
        rows={[
          ['FROM metrics', 'Query metric signal (routes to Prometheus/VictoriaMetrics)'],
          ['FROM logs', 'Query log signal (routes to VictoriaLogs/Loki)'],
          ['FROM metrics:victoria', 'Explicit backend hint'],
          ['FROM vlogs', 'Direct VictoriaLogs backend'],
          ['FROM metrics, logs', 'Multi-signal query (requires CORRELATE)'],
          ['FROM metrics AS m', 'Aliased source for qualified references'],
        ]}
      />

      <H3>WHERE</H3>
      <P>Filter conditions. Supports comparison, logical, regex, string match, and IN list operators.</P>
      <Table
        headers={['Operator', 'Example', 'Description']}
        rows={[
          ['=', '__name__ = "up"', 'Equality'],
          ['!=', 'env != "dev"', 'Not equal'],
          ['>', 'value > 100', 'Greater than'],
          ['<, >=, <=', 'latency >= 500', 'Comparison'],
          ['=~', 'service =~ "api-.*"', 'Regex match'],
          ['!~', 'host !~ "test-.*"', 'Regex not match'],
          ['AND', 'a = "x" AND b = "y"', 'Logical AND'],
          ['OR', 'a = "x" OR a = "y"', 'Logical OR'],
          ['IN', 'service IN ["a", "b"]', 'List membership'],
          ['NOT IN', 'env NOT IN ["dev", "test"]', 'List exclusion'],
          ['CONTAINS', 'message CONTAINS "error"', 'Substring match'],
          ['STARTS WITH', 'host STARTS WITH "prod-"', 'Prefix match'],
          ['MATCHES', 'path MATCHES "^/api/v[0-9]"', 'Regex match (string)'],
        ]}
      />

      <H3>WITHIN</H3>
      <P>Time range for the query.</P>
      <Table
        headers={['Syntax', 'Example']}
        rows={[
          ['WITHIN last <duration>', 'WITHIN last 5m, WITHIN last 24h'],
          ['WITHIN "<from>" TO "<to>"', 'WITHIN "2025-03-01" TO "2025-03-10"'],
          ['WITHIN today', 'Current day'],
          ['WITHIN this_week', 'Current week'],
        ]}
      />
      <P>Duration units: ms, s, m, h, d, w</P>

      <H3>COMPUTE</H3>
      <P>Aggregation and computation functions.</P>
      <Table
        headers={['Function', 'Example', 'Target']}
        rows={[
          ['rate(field, duration)', 'COMPUTE rate(value, 5m)', 'PromQL: rate()'],
          ['avg(field)', 'COMPUTE avg(value)', 'PromQL: avg()'],
          ['sum(field)', 'COMPUTE sum(value)', 'PromQL: sum()'],
          ['count()', 'COMPUTE count()', 'PromQL: count()'],
          ['min(field), max(field)', 'COMPUTE max(value)', 'PromQL: max()'],
          ['p50, p90, p95, p99', 'COMPUTE p99(value)', 'PromQL: histogram_quantile()'],
          ['stddev(field)', 'COMPUTE stddev(value)', 'PromQL: stddev()'],
          ['topk(n, field)', 'COMPUTE topk(10, value)', 'PromQL: topk()'],
        ]}
      />

      <H3>GROUP BY</H3>
      <P>Group results by label fields.</P>
      <Code>{`FROM metrics WHERE __name__ = "http_requests_total"
COMPUTE rate(value, 5m) GROUP BY service, env`}</Code>

      <H3>HAVING</H3>
      <P>Post-aggregation filter.</P>
      <Code>{`COMPUTE rate(value, 5m) GROUP BY service
HAVING rate > 0.01`}</Code>

      <H3>PARSE</H3>
      <P>Log parsing modes (logs signal only).</P>
      <Table
        headers={['Mode', 'Example']}
        rows={[
          ['json', 'PARSE json'],
          ['logfmt', 'PARSE logfmt'],
          ['pattern', 'PARSE pattern "<ip> - <method> <path>"'],
          ['regexp', 'PARSE regexp "(?P<status>\\d{3})"'],
        ]}
      />

      <H3>CORRELATE</H3>
      <P>Cross-signal correlation. Joins metric and log results by shared fields within a time window.</P>
      <Code>{`FROM metrics, logs
WHERE metrics.__name__ = "up" AND logs.level = "error"
CORRELATE ON host WITHIN 60s`}</Code>

      <H3>SHOW</H3>
      <P>Output format hint.</P>
      <Table
        headers={['Format', 'Description']}
        rows={[
          ['SHOW timeseries', 'Time series chart format'],
          ['SHOW table', 'Tabular format'],
          ['SHOW timeline', 'Timeline view'],
          ['SHOW count', 'Count only'],
          ['SHOW heatmap', 'Heatmap visualization'],
          ['SHOW alert', 'Alert-style output'],
          ['SHOW topology', 'Network topology view'],
        ]}
      />

      <H3>DEFINE</H3>
      <P>Macro definitions for reusable expressions.</P>
      <Code>{`DEFINE error_rate = rate(value, 5m)
FROM metrics WHERE __name__ = "http_errors_total"
COMPUTE error_rate GROUP BY service`}</Code>

      <H3>NATIVE</H3>
      <P>Passthrough to native backend query language.</P>
      <Code>{`FROM metrics WHERE NATIVE("promql", "rate(up[5m])")`}</Code>

      <H3>Pipe Syntax</H3>
      <P>Alternative pipe-based syntax.</P>
      <Code>{`FROM metrics:victoria
|> WHERE __name__ = "cpu_usage" AND env = "prod"
|> WITHIN last 1h
|> COMPUTE avg(value) GROUP BY host`}</Code>
    </>
  );
}

// ─── API REFERENCE ───────────────────────────────────────────────

function ApiRef() {
  return (
    <>
      <H2>API Reference</H2>
      <P>The UniQL Engine exposes a REST API on port 9090 (configurable).</P>

      <H3>POST /v1/query</H3>
      <P>Execute a UniQL query against configured backends.</P>
      <Table
        headers={['Field', 'Type', 'Required', 'Description']}
        rows={[
          ['query', 'string', 'Yes', 'UniQL query string'],
          ['format', 'string', 'No', 'Output format override'],
          ['limit', 'number', 'No', 'Max results (default: 100)'],
        ]}
      />
      <Code>{`curl -X POST http://localhost:9090/v1/query \\
  -H "Content-Type: application/json" \\
  -d '{"query": "FROM metrics WHERE __name__ = \\"up\\" WITHIN last 5m", "limit": 10}'`}</Code>
      <P>Response includes metadata with timing breakdown:</P>
      <Code>{`{
  "status": "success",
  "data": { ... },
  "metadata": {
    "query_id": "uuid",
    "parse_time_us": 14,
    "transpile_time_us": 10,
    "execute_time_ms": 8,
    "total_time_ms": 8,
    "backend": "victoria",
    "backend_type": "prometheus",
    "native_query": "up[5m]",
    "signal_type": "metrics"
  }
}`}</Code>

      <H3>POST /v1/validate</H3>
      <P>Validate query syntax without executing.</P>
      <Code>{`curl -X POST http://localhost:9090/v1/validate \\
  -H "Content-Type: application/json" \\
  -d '{"query": "FROM metrics WHERE __name__ = \\"up\\""}'

# Response:
{ "valid": true, "signals": ["Metrics"], "clauses": "FROM -> WHERE", "warnings": [] }`}</Code>

      <H3>POST /v1/explain</H3>
      <P>Show execution plan without running the query.</P>
      <Code>{`curl -X POST http://localhost:9090/v1/explain \\
  -H "Content-Type: application/json" \\
  -d '{"query": "FROM metrics WHERE __name__ = \\"up\\" WITHIN last 5m"}'

# Response:
{ "plan": { "steps": [
  { "step": 1, "action": "parse", "detail": "UNIQL -> AST" },
  { "step": 2, "action": "transpile_metrics", "native_query": "up[5m]" },
  { "step": 3, "action": "execute", "detail": "Execute against backend" }
]}}`}</Code>

      <H3>POST /v1/investigate</H3>
      <P>Run investigation packs — multiple parallel queries triggered by an alert.</P>
      <Table
        headers={['Field', 'Type', 'Required', 'Description']}
        rows={[
          ['pack', 'string', 'Yes', 'Pack name: high_cpu, link_down, error_spike, latency_degradation, custom'],
          ['params', 'object', 'No', 'Parameter substitution ($host, $service)'],
          ['queries', 'string[]', 'No', 'Custom queries (when pack = "custom")'],
        ]}
      />
      <Code>{`curl -X POST http://localhost:9090/v1/investigate \\
  -H "Content-Type: application/json" \\
  -d '{"pack": "high_cpu", "params": {"host": "esxi-node01.example.com"}}'

# Response:
{ "status": "success", "pack": "high_cpu", "total_time_ms": 7,
  "results": [
    { "name": "host_cpu_trend", "status": "success", "execute_time_ms": 5, ... },
    { "name": "vm_cpu_on_host", "status": "success", "execute_time_ms": 4, ... },
    { "name": "host_memory",    "status": "success", "execute_time_ms": 6, ... }
  ]
}`}</Code>

      <H3>GET /health</H3>
      <P>Engine health check with backend connectivity status.</P>
      <Code>{`curl http://localhost:9090/health

{ "status": "ok", "version": "0.3.0",
  "backends": [
    { "name": "victoria", "type": "prometheus", "url": "...", "status": "reachable" },
    { "name": "vlogs", "type": "victorialogs", "url": "...", "status": "reachable" }
  ]
}`}</Code>

      <H3>GET /metrics</H3>
      <P>Prometheus-compatible metrics endpoint for self-monitoring.</P>
      <Table
        headers={['Metric', 'Type', 'Description']}
        rows={[
          ['uniql_queries_total', 'counter', 'Total queries executed'],
          ['uniql_queries_cached', 'counter', 'Cache hits'],
          ['uniql_queries_errors', 'counter', 'Failed queries'],
          ['uniql_investigate_total', 'counter', 'Investigation packs run'],
          ['uniql_cache_entries', 'gauge', 'Active cache entries'],
          ['uniql_info', 'info', 'Version and backend count'],
        ]}
      />

      <H3>GET /v1/schema</H3>
      <P>Discover available metrics, labels, and backends.</P>
      <Code>{`curl http://localhost:9090/v1/schema

{ "metrics": ["up", "node_cpu_seconds_total", ...],
  "labels": ["job", "instance", "env", ...],
  "label_values": { "job": ["node-exporter", "vmagent", ...] },
  "backends": ["victoria", "vlogs"],
  "total_time_ms": 12
}`}</Code>

      <H3>Authentication</H3>
      <P>Set UNIQL_API_KEYS environment variable (comma-separated). Send key via X-Api-Key header.</P>
      <Code>{`# Enable auth:
UNIQL_API_KEYS=key1,key2 uniql-engine

# Use:
curl -H "X-Api-Key: key1" http://localhost:9090/v1/query ...`}</Code>

      <H3>Configuration</H3>
      <Table
        headers={['Env Variable', 'Default', 'Description']}
        rows={[
          ['UNIQL_CONFIG', '—', 'Path to TOML config file'],
          ['UNIQL_LISTEN', '0.0.0.0:9090', 'Listen address'],
          ['UNIQL_BACKENDS', '(defaults)', 'JSON array of backends'],
          ['UNIQL_API_KEYS', '(empty)', 'Comma-separated API keys'],
          ['UNIQL_CORS_ORIGINS', '(permissive)', 'Comma-separated CORS origins'],
          ['RUST_LOG', 'info', 'Log level'],
        ]}
      />
    </>
  );
}

// ─── EXAMPLES ────────────────────────────────────────────────────

function ExamplesRef() {
  return (
    <>
      <H2>Query Examples</H2>

      <H3>Basic Metrics</H3>
      <Code>{`-- Service health
FROM metrics WHERE __name__ = "up" WITHIN last 5m

-- CPU usage rate by instance
FROM metrics WHERE __name__ = "node_cpu_seconds_total" AND mode = "idle"
WITHIN last 1h COMPUTE rate(value, 5m) GROUP BY instance

-- HTTP request rate by service
FROM metrics WHERE __name__ = "http_requests_total" AND env = "prod"
WITHIN last 5m COMPUTE rate(value, 1m) GROUP BY service`}</Code>

      <H3>Filtering</H3>
      <Code>{`-- Regex match
FROM metrics WHERE __name__ = "http_requests_total" AND service =~ "api-.*"

-- IN list
FROM metrics WHERE __name__ = "up" AND job IN ["nginx", "envoy", "haproxy"]

-- Multiple labels
FROM metrics WHERE __name__ = "http_requests_total"
  AND job = "api" AND env = "prod" AND region = "us-east"

-- HAVING threshold
FROM metrics WHERE __name__ = "http_requests_total"
COMPUTE rate(value, 5m) GROUP BY service HAVING rate > 0.01`}</Code>

      <H3>Log Queries</H3>
      <Code>{`-- Simple log search
FROM logs WHERE service = "api" AND level = "error" WITHIN last 1h

-- Contains filter
FROM logs WHERE message CONTAINS "connection refused" WITHIN last 15m

-- FortiGate syslog
SHOW table FROM vlogs WHERE job = "fortigate" WITHIN last 15m

-- JSON parsing
FROM logs WHERE service = "api" WITHIN last 1h PARSE json`}</Code>

      <H3>AETHERIS Scenarios</H3>
      <Code>{`-- SNMP device status
SHOW timeseries FROM victoria WHERE __name__ = "snmpv2_device_up"

-- ESXi host CPU by cluster
FROM metrics WHERE __name__ = "vsphere_host_cpu_usage_average"
  AND clustername = "Production_Cluster" WITHIN last 1h

-- VM memory usage
SHOW timeseries FROM victoria WHERE __name__ = "vsphere_vm_mem_usage_average"`}</Code>

      <H3>Investigation Packs</H3>
      <Code>{`# High CPU investigation (3 parallel queries)
curl -X POST http://localhost:9090/v1/investigate \\
  -H "Content-Type: application/json" \\
  -d '{"pack": "high_cpu", "params": {"host": "esxi-node01.example.com"}}'

# Link down investigation
curl -X POST http://localhost:9090/v1/investigate \\
  -H "Content-Type: application/json" \\
  -d '{"pack": "link_down", "params": {"host": "CORE-SW-01"}}'

# Custom investigation
curl -X POST http://localhost:9090/v1/investigate \\
  -H "Content-Type: application/json" \\
  -d '{"pack": "custom", "queries": [
    "FROM metrics WHERE __name__ = \\"up\\" WITHIN last 5m",
    "FROM logs WHERE level = \\"error\\" WITHIN last 15m"
  ]}'`}</Code>

      <H3>Cross-Signal Correlation</H3>
      <Code>{`-- Correlate metrics spike with log events
FROM metrics, logs
WHERE metrics.__name__ = "ifInErrors" AND logs.message CONTAINS "link down"
CORRELATE ON host WITHIN 60s`}</Code>
    </>
  );
}

// ─── BACKENDS REFERENCE ──────────────────────────────────────────

function BackendsRef() {
  return (
    <>
      <H2>Backend Reference</H2>
      <P>UniQL transpiles to native query languages. Here's how each clause maps.</P>

      <H3>PromQL (Prometheus / VictoriaMetrics)</H3>
      <Table
        headers={['UniQL', 'PromQL', 'Notes']}
        rows={[
          ['FROM metrics WHERE __name__ = "up"', 'up', 'Metric selector'],
          ['AND job = "api"', 'up{job="api"}', 'Label matcher'],
          ['AND service =~ "api-.*"', 'up{service=~"api-.*"}', 'Regex matcher'],
          ['AND env IN ["a","b"]', 'up{env=~"a|b"}', 'IN → regex union'],
          ['WITHIN last 5m', '[5m]', 'Range vector'],
          ['COMPUTE rate(value, 5m)', 'rate(...[5m])', 'Range function'],
          ['COMPUTE avg(value)', 'avg(...)', 'Aggregation'],
          ['COMPUTE p99(value)', 'histogram_quantile(0.99, ...)', 'Percentile'],
          ['GROUP BY service', '... by (service)', 'Label grouping'],
          ['HAVING rate > 0.01', '... > 0.01', 'Post-filter'],
        ]}
      />

      <H3>LogsQL (VictoriaLogs)</H3>
      <Table
        headers={['UniQL', 'LogsQL', 'Notes']}
        rows={[
          ['FROM logs WHERE service = "api"', '_stream:{service="api"}', 'Stream selector'],
          ['AND level = "error"', '_stream:{service="api",level="error"}', 'Multi-label'],
          ['AND message CONTAINS "err"', '... "err"', 'Content filter'],
          ['WITHIN last 15m', '... _time:15m', 'Time range'],
          ['PARSE json', '| unpack_json', 'JSON parsing'],
        ]}
      />

      <H3>LogQL (Grafana Loki)</H3>
      <Table
        headers={['UniQL', 'LogQL', 'Notes']}
        rows={[
          ['FROM logs WHERE service = "api"', '{service="api"}', 'Stream selector'],
          ['AND level = "error"', '{service="api",level="error"}', 'Multi-label'],
          ['AND message CONTAINS "err"', '... |= "err"', 'Line filter'],
          ['PARSE json', '| json', 'JSON parsing'],
          ['COMPUTE rate(count, 5m)', 'rate({...}[5m])', 'Log rate'],
          ['GROUP BY level', '... by (level)', 'Label grouping'],
        ]}
      />

      <H3>Backend Configuration</H3>
      <Code>{`# uniql-engine.toml
listen = "0.0.0.0:9090"

[[backends]]
name = "victoria"
type = "prometheus"
url = "http://victoria-metrics:8428"

[[backends]]
name = "vlogs"
type = "victorialogs"
url = "http://victoria-logs:9428"

# Or via environment:
UNIQL_BACKENDS='[{"name":"vm","type":"prometheus","url":"http://vm:8428"}]'`}</Code>

      <H3>Signal Routing</H3>
      <P>The engine automatically routes queries to the correct backend based on signal type:</P>
      <Table
        headers={['Signal', 'Backend Type', 'Transpiler']}
        rows={[
          ['metrics', 'prometheus / victoriametrics', 'PromQL'],
          ['logs', 'victorialogs', 'LogsQL'],
          ['logs', 'loki', 'LogQL'],
          ['vlogs (alias)', 'victorialogs', 'LogsQL'],
        ]}
      />
    </>
  );
}
