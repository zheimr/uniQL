# UniQL — Write Once, Query Everything

Unified observability query language. Single syntax for metrics, logs, and traces across multiple backends.

```
FROM metrics WHERE __name__ = "vsphere_host_cpu_usage_average"
  AND clustername = "DELLR750_Cluster"
WITHIN last 1h
COMPUTE avg(value) GROUP BY esxhostname
```

One query. Three backends: **PromQL** (Prometheus/VictoriaMetrics), **LogQL** (Loki), **LogsQL** (VictoriaLogs).

---

## Why

Every observability backend has its own query language. PromQL for metrics, LogQL for logs, LogsQL for VictoriaLogs, SPL for Splunk. Teams learn 3+ languages and can't correlate across signals.

UniQL solves this: **one syntax → transpile to any backend → execute in parallel → correlate results**.

No data migration. No vendor lock-in. Your existing backends stay where they are.

## Architecture

```
                    ┌─────────────────────────────────┐
                    │         UniQL Engine             │
                    │                                  │
  UNIQL Query ────► Lexer → Parser → Binder →         │
                    │  Normalizer → Transpiler ──────► PromQL  → VictoriaMetrics
                    │                          ──────► LogsQL  → VictoriaLogs
                    │                          ──────► LogQL   → Loki
                    │                                  │
                    │  Executor → Correlator →         │
                    │  Formatter → Response            │
                    └─────────────────────────────────┘
```

**12-layer compiler pipeline**, sub-millisecond parse time, Rust + Tokio async runtime.

## Quick Start

```bash
# Run engine
docker compose up -d

# Query metrics
curl -X POST http://localhost:9090/v1/query \
  -H "Content-Type: application/json" \
  -d '{"query": "SHOW timeseries FROM victoria WHERE __name__ = \"up\""}'

# Query logs
curl -X POST http://localhost:9090/v1/query \
  -H "Content-Type: application/json" \
  -d '{"query": "SHOW table FROM vlogs WHERE job = \"fortigate\" WITHIN last 5m"}'

# Explain execution plan
curl -X POST http://localhost:9090/v1/explain \
  -H "Content-Type: application/json" \
  -d '{"query": "FROM metrics WHERE __name__ = \"up\" WITHIN last 1h"}'

# Investigate (parallel 3-query pack)
curl -X POST http://localhost:9090/v1/investigate \
  -H "Content-Type: application/json" \
  -d '{"pack": "high_cpu", "params": {"host": "r750g01.kocaeli.bel.tr"}}'
```

## Language Reference

### FROM — Data Source

```sql
FROM metrics                              -- Prometheus/VictoriaMetrics
FROM logs                                 -- VictoriaLogs/Loki
FROM vlogs                                -- VictoriaLogs (explicit)
FROM victoria                             -- VictoriaMetrics (explicit)
FROM metrics, logs CORRELATE ON host      -- Cross-signal join
```

### WHERE — Filtering

```sql
WHERE __name__ = "up"                     -- Metric name
WHERE service = "api" AND env = "prod"    -- Label equality
WHERE service != "debug"                  -- Negation
WHERE service =~ "api.*"                  -- Regex match
WHERE service IN ["api", "web"]           -- IN list
WHERE message CONTAINS "error"            -- Log content search
WHERE message MATCHES "err.*timeout"      -- Regex content search
WHERE message STARTS WITH "FATAL"         -- Prefix match
```

### WITHIN — Time Range

```sql
WITHIN last 5m                            -- Relative duration
WITHIN last 1h                            -- Hours
WITHIN last 7d                            -- Days
WITHIN "2026-03-01" TO "2026-03-15"       -- Absolute range
WITHIN today                              -- Today
WITHIN this_week                          -- This week
```

### COMPUTE — Aggregation

```sql
COMPUTE count()                           -- Count
COMPUTE sum(value)                        -- Sum
COMPUTE avg(value)                        -- Average
COMPUTE min(value) / max(value)           -- Min/Max
COMPUTE rate(value, 5m)                   -- Rate over duration
COMPUTE p50(value) / p95(value) / p99(value)  -- Percentiles
```

### GROUP BY + HAVING

```sql
COMPUTE rate(value, 5m) GROUP BY service
COMPUTE count() GROUP BY level HAVING count > 100
```

### SHOW — Output Format

```sql
SHOW timeseries     -- Time series data (default)
SHOW table          -- Tabular format
SHOW count          -- Single count value
SHOW timeline       -- Timeline view
SHOW heatmap        -- Heatmap view
```

### PARSE — Log Parsing

```sql
PARSE json                                -- JSON field extraction
PARSE logfmt                              -- Logfmt parsing
PARSE pattern "<ip> - <method> <path>"    -- Pattern template
PARSE regexp "(?P<status>\d{3})"          -- Regex extraction
```

### CORRELATE — Cross-Signal Join

```sql
FROM metrics, logs
CORRELATE ON host WITHIN 60s
```

### DEFINE — Reusable Macros

```sql
DEFINE high_cpu = __name__ = "vsphere_host_cpu_usage_average"
FROM metrics WHERE high_cpu AND clustername = "DELLR750_Cluster"
```

### NATIVE — Backend Passthrough

```sql
NATIVE("promql", "rate(http_requests_total[5m])")
FROM logs WHERE service = "api" AND NATIVE("status_code:>=500")
```

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/query` | POST | Execute a UniQL query |
| `/v1/explain` | POST | Show execution plan |
| `/v1/validate` | POST | Validate query syntax |
| `/v1/investigate` | POST | Run investigation pack |
| `/health` | GET | Engine + backend health |

### Query Request

```json
{
  "query": "FROM metrics WHERE __name__ = \"up\" WITHIN last 1h",
  "format": "json",
  "limit": 100
}
```

### Investigation Packs

| Pack | Target | Queries |
|------|--------|---------|
| `high_cpu` | ESXi hosts | Host CPU trend + VM CPU + Host memory |
| `link_down` | Network | Device status + Interface status + Firewall logs |
| `error_spike` | Services | Event rate + Error logs + API errors |
| `latency_degradation` | APIs | Latency + Request rate + Slow logs |

## Project Structure

```
uniql-core/           Rust library — parser, AST, 3 transpilers
uniql-engine/         HTTP execution engine — Axum + Tokio
uniql-wasm/           WebAssembly module — browser transpilation
demo/                 React demo UI — 4 tabs (Overview, Live, Transpile, Investigate)
```

## WASM Module

7 browser functions, zero server dependency:

```javascript
import { parse, to_promql, to_logql, to_logsql, validate, explain, autocomplete } from 'uniql-wasm';

const promql = to_promql('FROM metrics WHERE __name__ = "up"');
// → "up"

const logsql = to_logsql('FROM logs WHERE service = "api" AND level = "error"');
// → '_stream:{service="api"} level:error'

const plan = JSON.parse(explain('FROM metrics WHERE __name__ = "up" WITHIN last 1h'));
// → { steps: [{ action: "parse", ... }, { action: "transpile_promql", native_query: "up[1h]", ... }] }
```

## Configuration

Environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `UNIQL_LISTEN` | `0.0.0.0:9090` | Listen address |
| `UNIQL_BACKENDS` | VictoriaMetrics + VictoriaLogs | Backend config (JSON array) |
| `UNIQL_API_KEYS` | *(disabled)* | Comma-separated API keys |
| `UNIQL_CORS_ORIGINS` | *(permissive)* | Allowed CORS origins |

## Testing

```bash
cargo test --workspace        # 465 tests
cargo llvm-cov --workspace    # Coverage report (~83%)
```

## Security

- API key authentication with constant-time comparison
- Parameter injection prevention (allowlist sanitization)
- Panic recovery middleware (500 instead of crash)
- Request timeout (60s global, 30s per backend)
- Body size limit (256KB)
- Correlator cardinality limit (10K max events)
- Duration overflow protection (max 365 days)
- Expression depth limit (max 64 nesting levels)

## Built With

- **Rust** — Engine + WASM
- **Axum** — HTTP framework
- **Tokio** — Async runtime
- **wasm-bindgen** — Browser bindings
- **React 18 + Vite** — Demo UI

## License

MIT

---

*UniQL v0.3.0 — 465 tests, 83% coverage, 12-layer pipeline, sub-ms parse time.*
