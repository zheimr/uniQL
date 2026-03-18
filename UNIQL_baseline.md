# UNIQL — Unified Observability Query Language
## Product Baseline v1.0

**Author:** Samet Yagci
**Date:** 2026-03-17
**Status:** Approved — Implementation Ready

---

## 1. Problem Statement

### 1.1 The Fragmentation Crisis

A modern SRE/DevOps engineer must learn 6+ query languages to observe a single system:

```
PromQL   → rate(http_requests_total[5m])
LogQL    → {app="nginx"} |= "error" | json | rate([5m])
TraceQL  → {span.http.status=500 && duration>100ms}
SQL      → SELECT * FROM metrics WHERE ...
SPL      → index=main sourcetype=nginx ERROR
KQL      → Perf | where CounterName == "CPU"
```

Same data. Same intent. Six incompatible languages. None of them talk to each other.

### 1.2 The Correlation Gap

Cross-signal investigation is the #1 unmet need in observability:

- **Metrics** tell you something is wrong (CPU spike, error rate increase)
- **Logs** tell you what went wrong (stack trace, error message)
- **Traces** tell you where it went wrong (which service, which span)
- **Events** tell you why it went wrong (deployment, config change)

Today, correlating these requires: opening 4 Grafana panels, manually aligning timestamps, writing Python scripts, or suffering.

**No platform offers declarative, first-class cross-signal correlation at the query language level.**

### 1.3 Quantified Pain

| Pain Point | Severity | Evidence |
|------------|----------|----------|
| PromQL steep learning curve | Critical | Community #1 complaint |
| Vector matching confusion (on/ignoring/group_left) | Critical | PromQL community |
| rate() silent failures | Critical | Data loss, no error |
| 6+ query languages to learn | Critical | Universal |
| Cross-signal correlation impossible | Critical | Universal |
| No variable/macro support in PromQL | High | GitHub Issue #11609 |
| Vendor lock-in (SPL, KQL) | High | SPL/KQL users |
| Poor error messages across all languages | Medium | Universal |

---

## 2. Vision

**"Write once, query everything."**

A single query language + execution engine for:
- **Metrics** (Prometheus, VictoriaMetrics, InfluxDB, ClickHouse)
- **Logs** (Loki, Elasticsearch, Splunk, ClickHouse)
- **Traces** (Tempo, Jaeger, Zipkin)
- **Events** (Kafka, custom webhook, OTel events)

And the ability to **query them together**:

```uniql
FROM metrics:victoria AS m, logs:loki AS l
WHERE m.__name__ = "http_latency_p99" AND m.value > 500
  AND l.level = "error"
  AND m.service = "api-gateway"
WITHIN last 15m
CORRELATE ON service, host WITHIN 10s
SHOW timeline
```

---

## 3. Market Position

### 3.1 Competitive Landscape

| Product | Approach | Unified Language | Cross-Signal | Open Source | Status |
|---------|----------|-----------------|--------------|-------------|--------|
| **Observe (OPAL)** | Pipeline-based unified QL | Yes | Partial | No (Snowflake acquired, $1B, Jan 2026) | Locked to Snowflake |
| **SigNoz** | OTel-native, PromQL + ClickHouse SQL | No (3 languages) | Manual | Yes | Growing |
| **Grafana** | PromQL + LogQL + TraceQL | No (3 languages) | UI-level Correlations Editor | Yes | Dominant |
| **Datadog** | DQL/DDSQL per signal | Partial | Auto (tags) | No | Enterprise |
| **New Relic** | NRQL (SQL-like) | Partial (single vendor) | NRQL JOIN | No | Enterprise |
| **Cribl Search** | Federated SQL | Partial | Federated | No | Emerging |
| **UNIQL** | Unified QL + Execution Engine | **Yes** | **First-class CORRELATE** | **Yes** | **Building** |

### 3.2 Standards Alignment

**CNCF TAG Observability** — "Query Language Standardization" working group (TOC #1770) is active, targeting H2 2026 spec. The group recommends "SQL as a basis with further experimentation on syntaxes." UNIQL's SQL-familiar approach aligns directly.

### 3.3 Market Validation

OPAL's $1B acquisition by Snowflake validates the unified query language market. But OPAL is now closed-source and Snowflake-locked. **The open-source position is vacant.**

---

## 4. Design Philosophy

### 4.1 SQL-Familiar, Not SQL-Compatible

UNIQL uses SQL-familiar syntax because every engineer knows SQL. But it is NOT SQL — it is a domain-specific language for observability. This avoids the false expectation trap that `SELECT` would create.

**Rationale (research-backed):**
- LLMs generate SQL 2.5x better than custom DSLs zero-shot (Faros AI study)
- But the gap closes with RAG + examples (custom DSL reaches 83% accuracy)
- Using `SELECT` creates false expectations of JOIN, subqueries, CTEs
- Every successful observability-native language (KQL, SPL, ES|QL) chose NOT to use `SELECT`

### 4.2 Signal-Type Aware

The language knows the nature of metrics, logs, traces, and events:
- `COMPUTE rate()` is valid on metrics, not on raw logs
- `PARSE json` is valid on logs, not on metrics
- `WHERE duration > 500ms` is valid on traces
- Cross-signal requires explicit `CORRELATE`

Signal-type validation catches errors at parse time, not runtime.

### 4.3 Correlation as First-Class Citizen

`CORRELATE ON` is not a bolt-on — it is a core clause of the language, designed from day one. This is UNIQL's primary differentiator. No other query language has this.

### 4.4 AI-Native

Deterministic, unambiguous AST. No implicit behavior. Every query has exactly one parse tree. LLMs can reliably generate and validate UNIQL.

### 4.5 Execution Engine Architecture

UNIQL is not just a transpiler. It is an execution engine (Trino/Presto model):

```
Client → UNIQL Engine → parse → plan → transpile → execute (parallel) → correlate → merge
                                                      ↓            ↓             ↓
                                                VictoriaMetrics   Loki         Tempo
Client ← unified result ←──────────────────────────────────────────────────────────┘
```

### 4.6 Dual Syntax: SQL-Style Primary, Pipe Secondary

UNIQL supports two syntaxes that parse to the same AST:

```uniql
-- SQL-style (primary, canonical)
FROM metrics
WHERE service = "nginx"
WITHIN last 5m
COMPUTE rate(value, 1m) GROUP BY host
SHOW timeseries

-- Pipe-style (secondary, same AST)
FROM metrics
|> WHERE service = "nginx"
|> WITHIN last 5m
|> COMPUTE rate(value, 1m) GROUP BY host
|> SHOW timeseries
```

**Decision:** `|>` (not `|`) — industry standard (Google SQL Pipe, Spark 4.0, Databricks, TC39, PHP 8.5). `|` conflicts with LogQL pipeline stages and bitwise OR.

SQL-style is the canonical form used in documentation, examples, and transpilation output. Pipe-style is syntactic sugar for users who prefer KQL/SPL-style exploration.

### 4.7 Error Messages: Clear, Located, Helpful

Elm/Rust-inspired error messages are a design priority. Not a bolt-on — baked into the parser from v1.0.

```
Error: PARSE is only valid for log sources

  FROM metrics:victoria
  |> PARSE json
     ^^^^^^^^^
  Signal type 'metrics' does not support PARSE.

  Hint: PARSE is used with FROM logs. Did you mean:
    FROM logs:loki |> PARSE json
```

v1.0 targets: accurate source locations, contextual code snippets, "did you mean?" suggestions.

---

## 5. Language Specification (v1.0)

### 5.1 Query Structure

A UNIQL query consists of an ordered sequence of clauses:

```
[DEFINE clause]    — reusable query fragments (macros)
[FROM clause]      — data source + signal type + backend + alias
[WHERE clause]     — filtering (labels, string match, regex, IN, comparison)
[WITHIN clause]    — time window
[PARSE clause]     — log parsing (json, logfmt, pattern, regexp)  [logs only]
[COMPUTE clause]   — aggregation & functions
[GROUP BY clause]  — grouping
[HAVING clause]    — post-aggregation filter
[CORRELATE clause] — cross-signal join
[SHOW clause]      — output format hint (frontend)
```

### 5.2 DEFINE / USE — Reusable Query Fragments

Macros for query reuse. Expand-before-transpile: the transpiler layer never sees DEFINE/USE.

**Design constraints:**
- Lexical scoping only (visible from declaration to end of query)
- No recursion (a DEFINE cannot reference itself)
- No conditional logic (not Turing complete)
- Both simple aliases and parameterized forms

```uniql
-- Simple alias
DEFINE prod_services = service IN ["api", "web", "worker"] AND env = "production"

-- Parameterized macro
DEFINE error_rate(metric_name, window) = (
  FROM metrics
  WHERE __name__ = metric_name
  COMPUTE rate(value, window)
)

-- Usage
FROM metrics
WHERE __name__ = "http_requests_total" AND prod_services
WITHIN last 5m
COMPUTE rate(value, 1m) GROUP BY service
SHOW timeseries

-- Parameterized usage
USE error_rate("http_5xx_total", 5m)
WHERE service = "api"
WITHIN last 1h
HAVING rate > 0.01
SHOW alert
```

**Implementation:** ~300 lines of Rust (based on MetricsQL WITH precedent). AST expansion pass between parser and semantic validation. Transpiler receives fully expanded AST.

### 5.3 FROM — Data Source Declaration

```uniql
FROM metrics                          -- all metric sources
FROM logs                             -- all log sources
FROM traces                           -- all trace sources
FROM events                           -- all event sources
FROM metrics, logs                    -- multi-source (requires CORRELATE)
FROM metrics:victoria                 -- explicit backend hint
FROM logs:loki, metrics:victoria      -- multiple backends
FROM metrics AS m, logs AS l          -- aliases for qualified references
FROM metrics:victoria AS m, logs:loki AS l, traces:tempo AS t  -- full form
```

**Signal types:** `metrics`, `logs`, `traces`, `events`
**Backend hints:** `victoria`, `vlogs`, `prometheus`, `loki`, `tempo`, `elastic`, `clickhouse`, `splunk`, `jaeger`, `zipkin`, `kafka`

### 5.4 WHERE — Filtering

Standard comparison, logical, string matching, and set operators:

```uniql
-- Comparison
WHERE service = "nginx"
WHERE status != 200
WHERE cpu > 80
WHERE latency >= 500
WHERE traces.duration > 500ms

-- Logical
WHERE service = "api" AND env = "prod"
WHERE level = "error" OR level = "fatal"
WHERE NOT status = 200

-- Qualified identifiers (signal-scoped)
WHERE metrics.__name__ = "http_requests_total"
WHERE metrics.cpu > 80
WHERE logs.level = "error"
WHERE logs.message CONTAINS "timeout"
WHERE traces.duration > 500ms
WHERE labels.env = "production"

-- String matching
WHERE message CONTAINS "timeout"        -- substring match
WHERE host STARTS WITH "prod-"          -- prefix match
WHERE message MATCHES "error.*5[0-9]{2}"  -- regex match

-- Set operations
WHERE service IN ["nginx", "envoy", "haproxy"]
WHERE status NOT IN [200, 201, 204]

-- Regex operators
WHERE service =~ "api-.*"
WHERE path !~ "/health.*"
```

### 5.5 WITHIN — Time Window

```uniql
WITHIN last 5m                        -- relative: last N duration
WITHIN last 1h
WITHIN last 24h
WITHIN last 7d
WITHIN "2026-03-01" TO "2026-03-17"   -- absolute range
WITHIN today                          -- named range
WITHIN this_week
```

Duration units: `ms`, `s`, `m`, `h`, `d`, `w`, `y`

### 5.6 PARSE — Log Parsing [Logs Only]

Signal-type constraint: PARSE is only valid when `FROM logs` is declared.

```uniql
PARSE json                                           -- JSON field extraction
PARSE logfmt                                         -- key=value parsing
PARSE pattern "<ip> - <method> <path> <status>"      -- template extraction
PARSE regexp "(?P<status>\\d{3})"                    -- regex extraction
```

Post-PARSE, extracted fields become available in subsequent WHERE, COMPUTE, GROUP BY clauses.

### 5.7 COMPUTE — Aggregation & Functions

UNIQL uses `COMPUTE` (not `SELECT` or `summarize`) as the aggregation keyword.

**Rationale (research-backed):**
- `SELECT` creates false SQL expectations (JOIN, subqueries, CTEs)
- `COMPUTE` clearly signals "calculation happening, not column selection"
- Every observability-native language avoids `SELECT` (KQL: `summarize`, SPL: `stats`, ES|QL: `STATS`)
- LLM gap vs SQL is closable with few-shot examples and RAG

#### 5.7.1 Time-Series Functions

```uniql
COMPUTE rate(value, 5m)               -- per-second rate over window
COMPUTE irate(value, 5m)              -- instant rate (last 2 samples)
COMPUTE increase(value, 1h)           -- total increase over window
COMPUTE rate(count, 5m)               -- log count rate (LogQL metric query)
```

#### 5.7.2 Aggregation Functions

```uniql
COMPUTE avg(cpu)                      -- average
COMPUTE sum(bytes)                    -- sum
COMPUTE min(latency)                  -- minimum
COMPUTE max(latency)                  -- maximum
COMPUTE count()                       -- count of records
COMPUTE count(logs) WHERE level = "error"  -- conditional count
```

#### 5.7.3 Percentile Functions

```uniql
COMPUTE p50(latency)                  -- 50th percentile
COMPUTE p90(latency)                  -- 90th percentile
COMPUTE p95(latency)                  -- 95th percentile
COMPUTE p99(latency)                  -- 99th percentile
COMPUTE histogram_quantile(0.99, rate(duration_bucket, 5m))  -- explicit quantile
```

#### 5.7.4 Statistical Functions

```uniql
COMPUTE stddev(latency)               -- standard deviation
COMPUTE stdvar(latency)               -- variance
COMPUTE topk(10, requests)            -- top K
COMPUTE bottomk(5, cpu)               -- bottom K
COMPUTE predict_linear(disk_usage, 4h) -- linear prediction [v1.0 ML]
```

#### 5.7.5 Multiple Computations

```uniql
COMPUTE rate(value, 5m) AS error_rate,
        avg(cpu) AS avg_cpu,
        p99(latency) AS tail_latency
```

### 5.8 GROUP BY — Grouping

```uniql
GROUP BY service
GROUP BY service, host, region
GROUP BY bin(timestamp, 5m)           -- time bucketing
GROUP BY service, bin(timestamp, 1m)  -- multi-dimensional
```

### 5.9 HAVING — Post-Aggregation Filter

```uniql
HAVING rate > 0.01
HAVING count > 100
HAVING avg_cpu > 80
HAVING error_rate * 100 > 5           -- arithmetic expressions
```

### 5.10 CORRELATE — Cross-Signal Join

The core differentiator. Declarative cross-signal correlation.

```uniql
-- Basic: label matching + time window
CORRELATE ON service, host WITHIN 30s

-- With clock skew tolerance
CORRELATE ON service, host WITHIN 30s SKEW_TOLERANCE 5s

-- Trace ID based (exact match, no time window needed)
CORRELATE ON trace_id

-- With cardinality protection
CORRELATE ON service WITHIN 30s CARDINALITY_LIMIT 100000
```

**Correlation strategies** (selected automatically based on context):
- **TimeFieldJoin:** Timestamp + field match (default for metrics + logs)
- **FieldJoin:** Exact field match, ignore time (for trace_id correlation)
- **TraceIdJoin:** OTel trace_id based correlation

### 5.11 SHOW — Output Format Hint

Frontend visualization hint. Not part of the query result, but guides client rendering.

```uniql
SHOW timeseries                       -- time-series chart
SHOW table                            -- tabular data
SHOW timeline                         -- event timeline (logs + events)
SHOW heatmap                          -- heat map
SHOW flamegraph                       -- trace flamegraph
SHOW topology                         -- service topology graph
SHOW count                            -- single number
SHOW alert                            -- alerting threshold view
```

### 5.12 Pipe Syntax (Alternative)

Same semantics, pipe-style syntax using `|>`:

```uniql
FROM metrics:victoria
|> WHERE __name__ = "http_requests_total" AND env = "prod"
|> WITHIN last 5m
|> COMPUTE rate(value, 1m) GROUP BY service
|> HAVING rate > 0.01
|> SHOW timeseries
```

The parser produces the same AST for both SQL-style and pipe-style queries.

---

## 6. Type System

### 6.1 Signal-Level Type Validation (v1.0)

UNIQL enforces signal-type constraints at parse time:

| Clause/Function | metrics | logs | traces | events |
|----------------|---------|------|--------|--------|
| `PARSE json/logfmt/pattern/regexp` | Error | Valid | Error | Error |
| `COMPUTE rate(value, N)` | Valid | Error* | Error | Error |
| `COMPUTE rate(count, N)` | Valid | Valid | Error | Error |
| `WHERE duration > Nms` | Error | Error | Valid | Error |
| `SHOW flamegraph` | Error | Error | Valid | Error |
| `SHOW topology` | Error | Error | Valid | Error |
| `CORRELATE ON` | Required when FROM has multiple signal types | | | |

*`rate()` on logs requires explicit `count` argument: `COMPUTE rate(count, 5m)`

### 6.2 Type Error Messages

```
Error: rate() requires a metric source

  FROM logs:loki
  WHERE service = "api"
  COMPUTE rate(value, 5m)
          ^^^^^^^^^^^^^^^^

  Signal type 'logs' does not support rate(value, ...).
  Did you mean: COMPUTE rate(count, 5m)  -- counts log entries per second
  Or use: FROM metrics WHERE __name__ = "..." COMPUTE rate(value, 5m)
```

### 6.3 Expression-Level Typing (v1.x — Specified, Deferred)

Future versions will add:
- Type inference: `metrics.cpu` resolves to numeric, `logs.message` resolves to string
- Function signatures: `avg()` requires numeric, `CONTAINS` requires string
- Schema introspection from backends (metric/label names, log field types)

The `Unknown` signal type serves as an escape hatch for flexibility.

---

## 7. Transpiler Targets

### 7.1 Transpiler Trait Interface

```rust
pub trait Transpiler: Send + Sync {
    fn name(&self) -> &str;
    fn supported_signals(&self) -> &[SignalType];
    fn transpile(&self, query: &Query) -> Result<TranspileOutput, TranspileError>;
    fn supports_correlation(&self) -> bool { false }
}

pub struct TranspileOutput {
    pub native_query: String,
    pub target_signal: SignalType,
    pub backend_type: BackendType,
    pub metadata: TranspileMetadata,
}

pub enum BackendType {
    Prometheus,       // Prometheus + VictoriaMetrics
    Loki,
    Tempo,
    Elasticsearch,
    ClickHouse,
    Custom(String),
}
```

### 7.2 Target Matrix

| Backend | Signal | v1.0 | v1.x | v2.0 |
|---------|--------|------|------|------|
| **PromQL** | metrics | Impl | - | - |
| **MetricsQL** | metrics | Impl | - | - |
| **LogsQL (VictoriaLogs)** | logs | Impl | - | - |
| **LogQL (Loki)** | logs | Impl | - | - |
| **TraceQL** | traces | Spec | Impl | - |
| **ES DSL** | logs | Spec | Spec | Impl |
| **ClickHouse SQL** | metrics+logs | - | Spec | Impl |
| **SPL** | logs | - | - | Spec |

### 7.3 Transpilation Examples

```
UNIQL:    FROM metrics WHERE __name__ = "http_requests_total" AND env = "prod"
          COMPUTE rate(value, 5m) GROUP BY service
PromQL:   sum by (service) (rate(http_requests_total{env="prod"}[5m]))

UNIQL:    FROM logs:vlogs WHERE service = "api" AND message CONTAINS "error"
          WITHIN last 1h
LogsQL:   service:api AND _msg:error | fields service, _msg

UNIQL:    FROM logs:loki WHERE service = "api" AND message CONTAINS "error"
          PARSE json WHERE level = "error" WITHIN last 1h
LogQL:    {service="api"} |= "error" | json | level = "error"

UNIQL:    FROM traces WHERE service = "frontend"
          AND duration > 500ms AND status = "error"
TraceQL:  {resource.service.name = "frontend" && duration > 500ms && status = error}
```

### 7.4 Reverse Transpilation (v1.1 — Import Wizard)

One-way best-effort conversion: PromQL → UNIQL.

- Covers ~80% of real-world PromQL queries automatically
- Flags unconvertible constructs (vector matching modifiers, subqueries) for manual review
- Uses `promql-parser` Rust crate as the PromQL parser
- Primary use case: migration from existing Grafana dashboards

Full bidirectional transpilation deferred to v2.0 (if demand materializes).

---

## 8. Architecture

### 8.1 System Overview

```
┌──────────────────────────────────────────────────────────┐
│                    Client Layer                            │
│  AETHERIS (React)  │  CLI  │  Grafana Plugin  │  REST API │
└────────────────────────┬─────────────────────────────────┘
                         │ HTTP POST /v1/query
┌────────────────────────▼─────────────────────────────────┐
│                  UNIQL Engine (Rust + axum)                │
│                                                           │
│  ┌──────────┐  ┌───────────┐  ┌────────────────────────┐ │
│  │  Lexer   │  │ Semantic  │  │    Query Planner       │ │
│  │  Parser  │→│ Validator  │→│  Decompose + Optimize  │ │
│  │  AST     │  │ Type Check│  │  Route to backends     │ │
│  └──────────┘  └───────────┘  └──────────┬─────────────┘ │
│                                           │               │
│  ┌──────────┐  ┌─────────────────────────▼─────────────┐ │
│  │  DEFINE  │  │       Transpiler Layer (trait-based)   │ │
│  │  Expand  │  │  PromQL │ MetricsQL │ LogQL │ TraceQL │ │
│  └──────────┘  └─────────────────────────┬─────────────┘ │
│                                           │               │
│  ┌────────────────────────────────────────▼─────────────┐ │
│  │         Async Executor (tokio + reqwest)              │ │
│  │    parallel backend HTTP calls with timeout + retry   │ │
│  └────────────────────────────────────────┬─────────────┘ │
│                                           │               │
│  ┌────────────────────────────────────────▼─────────────┐ │
│  │         Result Merger / Correlator                    │ │
│  │    timestamp align + field match + format + stream    │ │
│  └────────────────────────────────────────┬─────────────┘ │
└───────────────────────────────────────────┼───────────────┘
                                            │
              ┌───────────┬─────────────────┼──────────────┐
              ▼           ▼                 ▼              ▼
        VictoriaM.      Loki             Tempo        (future)
        :8428          :3100             :3200
```

### 8.2 Processing Pipeline

```
1. Input       "FROM metrics WHERE ..."
2. Lexer       → Token stream with spans
3. Parser      → Untyped AST
4. DEFINE Expand → Expanded AST (macros resolved)
5. Semantic    → Signal-type validation, error checking
6. Planner     → QueryPlan (sub-queries per backend)
7. Transpile   → Native queries (PromQL, LogQL, ...)
8. Execute     → Parallel HTTP calls to backends
9. Merge       → Unified result set
10. Correlate  → Cross-signal join (if CORRELATE present)
11. Format     → JSON/Table/CSV response
```

### 8.3 Component Responsibilities

| Component | Responsibility | Technology |
|-----------|---------------|------------|
| Lexer | UNIQL string → token stream with spans | Rust (handwritten) |
| Parser | Token stream → typed AST (recursive descent + Pratt) | Rust |
| DEFINE Expander | Macro resolution, expand-before-transpile | Rust |
| Semantic Validator | Signal-type checking, clause compatibility | Rust |
| Query Planner | Multi-signal decompose, backend routing, optimization | Rust |
| Transpiler Layer | AST → native query (trait-based, per backend) | Rust |
| Executor | Async parallel HTTP calls with timeout/retry | Rust (tokio + reqwest) |
| Result Merger | Timestamp alignment, field matching, formatting | Rust |
| Correlator | Cross-signal join (TimeFieldJoin, TraceIdJoin) | Rust |
| HTTP API | REST endpoints (/v1/query, /validate, /explain, /health) | Rust (axum) |
| CLI | Terminal tool for dev/debug | Rust (clap) |
| WASM | Browser/Node binding for playground + npm | Rust → wasm-pack |

---

## 9. API Design

### 9.1 POST /v1/query — Execute Query

```json
// Request
{
  "query": "FROM metrics WHERE __name__ = \"http_requests_total\" WITHIN last 5m COMPUTE rate(value, 5m) GROUP BY service",
  "format": "json"
}

// Response
{
  "status": "success",
  "data": {
    "result_type": "matrix",
    "result": [ ... ],
    "metadata": {
      "query_id": "550e8400-e29b-41d4-a716-446655440000",
      "parse_time_us": 42,
      "transpile_time_us": 15,
      "execute_time_ms": 230,
      "total_time_ms": 245,
      "backend": "victoria",
      "native_query": "sum by (service) (rate(http_requests_total[5m]))",
      "signal_type": "metrics"
    }
  }
}
```

### 9.2 POST /v1/validate — Validate Query

```json
// Request
{ "query": "FROM metrics WHERE ..." }

// Response
{
  "valid": true,
  "ast_summary": {
    "signals": ["metrics"],
    "clauses": ["FROM", "WHERE", "WITHIN", "COMPUTE", "GROUP BY"],
    "backends": ["victoria"]
  },
  "warnings": [],
  "errors": []
}
```

### 9.3 POST /v1/explain — Execution Plan

```json
// Request
{ "query": "FROM metrics:victoria, logs:loki WHERE ..." }

// Response
{
  "plan": {
    "steps": [
      { "step": 1, "action": "parse", "detail": "UNIQL → AST" },
      { "step": 2, "action": "validate", "detail": "Signal-type check passed" },
      { "step": 3, "action": "decompose", "detail": "Split into 2 sub-queries" },
      { "step": 4, "action": "transpile_metrics", "native": "rate(ifInErrors{...}[5m])" },
      { "step": 5, "action": "transpile_logs", "native": "{host=\"...\"} |= \"link\"" },
      { "step": 6, "action": "execute_parallel", "detail": "Query VictoriaMetrics + Loki" },
      { "step": 7, "action": "correlate", "detail": "Merge on host WITHIN 60s" }
    ]
  }
}
```

### 9.4 GET /health — Health Check

```json
{
  "status": "ok",
  "version": "1.0.0",
  "backends": [
    { "name": "victoria", "type": "prometheus", "url": "http://...", "status": "reachable", "latency_ms": 12 },
    { "name": "loki", "type": "loki", "url": "http://...", "status": "reachable", "latency_ms": 8 }
  ]
}
```

---

## 10. Project Structure

```
uniql/
├── Cargo.toml                        # workspace root
├── README.md
├── LICENSE                           # Apache 2.0
├── Dockerfile                        # multi-stage build
├── docker-compose.yml                # dev environment
│
├── uniql-core/                       # Parser + Transpiler library
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                    # Public API: parse(), to_promql(), to_logql()
│       ├── config.rs                 # Defaults, constants (DEFAULT_RANGE_DURATION, QUANTILE_MAP)
│       ├── lexer/
│       │   └── mod.rs                # Handwritten tokenizer with spans
│       ├── ast/
│       │   └── mod.rs                # Typed AST nodes (Query, Expr, all clauses)
│       ├── parser/
│       │   └── mod.rs                # Recursive descent + Pratt parsing
│       ├── expand/
│       │   └── mod.rs                # DEFINE/USE macro expansion
│       ├── semantic/
│       │   └── mod.rs                # Signal-type validation, error checking
│       └── transpiler/
│           ├── mod.rs                # Transpiler trait interface
│           ├── promql.rs             # UNIQL → PromQL/MetricsQL
│           └── logql.rs              # UNIQL → LogQL
│
├── uniql-engine/                     # HTTP execution engine
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs                   # axum HTTP server
│       ├── config.rs                 # Backend config (TOML/YAML)
│       ├── api/
│       │   ├── mod.rs
│       │   ├── query.rs              # POST /v1/query
│       │   ├── validate.rs           # POST /v1/validate
│       │   ├── explain.rs            # POST /v1/explain
│       │   └── health.rs             # GET /health
│       ├── executor/
│       │   ├── mod.rs
│       │   ├── traits.rs             # BackendExecutor trait
│       │   ├── prometheus.rs          # VictoriaMetrics/Prometheus HTTP client
│       │   └── loki.rs               # Loki HTTP client
│       ├── planner/
│       │   ├── mod.rs
│       │   ├── decompose.rs          # Multi-signal query decomposition
│       │   └── optimize.rs           # Predicate pushdown, parallel planning
│       └── result/
│           ├── mod.rs
│           ├── types.rs              # UnifiedResult, TimeSeries, LogStream
│           ├── merge.rs              # Result merging
│           └── correlate.rs          # CORRELATE implementation
│
├── uniql-cli/                        # CLI tool
│   ├── Cargo.toml
│   └── src/main.rs
│
├── uniql-wasm/                       # WASM bindings
│   ├── Cargo.toml
│   └── src/lib.rs                    # parse(), to_promql(), to_logql(), validate()
│
└── packages/                         # TypeScript ecosystem
    ├── uniql-core/                   # npm: @uniql/core (WASM wrapper)
    ├── uniql-playground/             # Web playground (React + Monaco)
    │   └── src/
    │       └── uniql-monarch.ts      # Monaco syntax definition
    ├── uniql-grafana/                # Grafana datasource plugin
    └── uniql-vscode/                 # VS Code extension (TextMate grammar)
```

---

## 11. Real-World Examples

### 11.1 Basic Metric Query

```uniql
-- "Show me request rate by service in production"
FROM metrics
WHERE __name__ = "http_requests_total" AND env = "prod"
WITHIN last 5m
COMPUTE rate(value, 1m) GROUP BY service
SHOW timeseries
```
→ PromQL: `sum by (service) (rate(http_requests_total{env="prod"}[1m]))`

### 11.2 Log Search with Parsing

```uniql
-- "Find error logs from the API, parse JSON, show last hour"
FROM logs
WHERE service = "api"
WITHIN last 1h
PARSE json
WHERE level = "error"
SHOW timeline
```
→ LogQL: `{service="api"} | json | level = "error"`

### 11.3 Cross-Signal Correlation (The Power)

```uniql
-- "When latency spiked, what errors appeared?"
FROM metrics:victoria AS m, logs:vlogs AS l
WHERE m.__name__ = "http_latency_p99" AND m.value > 500
  AND l.level = "error"
  AND m.service = "payment-service"
WITHIN last 30m
CORRELATE ON service WITHIN 10s
SHOW timeline
```

### 11.4 AETHERIS — SNMP + Syslog Correlation

```uniql
-- "Router interface errors with syslog link-down messages"
FROM metrics:victoria AS snmp, logs:vlogs AS syslog
WHERE snmp.__name__ = "ifInErrors" AND snmp.device_type = "router"
  AND syslog.message CONTAINS "link down"
WITHIN last 5m
CORRELATE ON host WITHIN 60s SKEW_TOLERANCE 5s
SHOW timeline
```

### 11.5 Reusable Definitions

```uniql
-- "Define once, use everywhere"
DEFINE prod_filter = env = "production" AND region IN ["us-east", "eu-west"]
DEFINE high_error_rate(svc, threshold) = (
  FROM metrics
  WHERE __name__ = "http_5xx_total" AND service = svc AND prod_filter
  COMPUTE rate(value, 5m)
  HAVING rate > threshold
)

USE high_error_rate("api-gateway", 0.05)
WITHIN last 1h
SHOW alert
```

### 11.6 Pipe-Style Exploration

```uniql
-- Same query, pipe-style for exploratory workflow
FROM metrics:victoria
|> WHERE __name__ = "cpu_usage" AND env = "prod"
|> WITHIN last 1h
|> COMPUTE avg(value) GROUP BY host, bin(timestamp, 5m)
|> HAVING avg > 80
|> SHOW heatmap
```

### 11.7 Trace Query (v1.x)

```uniql
-- "Slow backend calls from frontend service"
FROM traces
WHERE parent.service = "frontend" AND child.service = "backend"
  AND child.duration > 500ms
  AND child.status = "error"
WITHIN last 15m
COMPUTE count() GROUP BY child.service, child.operation
HAVING count > 5
SHOW topology
```

---

## 12. AETHERIS Integration (Dog-Food)

### 12.1 Current Flow (Without UNIQL)

```
React → FastAPI → VictoriaMetrics HTTP API (PromQL/MetricsQL)
React → FastAPI → VictoriaLogs HTTP API (LogsQL)
React → FastAPI → (manual correlation in Python)
```

### 12.2 New Flow (With UNIQL)

```
React → FastAPI → UNIQL Engine :9090 → VictoriaMetrics :8428 (metrics)
                                      → VictoriaLogs :9428   (logs)
                                      → Tempo :3200          (future)
```

### 12.3 Docker Integration

```yaml
# docker-compose.yml
services:
  uniql-engine:
    image: uniql/engine:latest
    ports:
      - "9090:9090"
    environment:
      UNIQL_BACKENDS: |
        [
          {"name": "victoria", "type": "prometheus", "url": "http://victoriametrics:8428"},
          {"name": "vlogs", "type": "victorialogs", "url": "http://victorialogs:9428"}
        ]
    depends_on:
      - victoriametrics
      - victorialogs
```

### 12.4 FastAPI Endpoint

```python
@router.post("/api/v1/query")
async def execute_query(request: QueryRequest):
    response = await httpx.post(
        "http://uniql-engine:9090/v1/query",
        json={
            "query": request.uniql_query,
            "format": "json"
        }
    )
    return response.json()
```

---

## 13. Implementation Phases

### Phase 1 — "Fix & Foundation" (Sprint 1-2)

**Goal:** Working transpiler with trait interface, PromQL + LogQL, all tests pass.

```
[x] Mevcut build-breaking bug fix (pub mod logql kaldır)
[ ] PromQL BUG-1: OR condition handling
[ ] PromQL BUG-2: HAVING output fix
[ ] config.rs — hardcoded values centralized
[ ] Transpiler trait interface
[ ] PromQL transpiler → trait migration
[ ] LogQL transpiler (stream selector + line filter + parse + metric query)
[ ] DEFINE/USE expansion pass (AST-level macro system)
[ ] Signal-type semantic validation pass
[ ] Error message system (location + context + suggestions)
[ ] Pipe syntax (|>) parser support
[ ] Unused code cleanup
[ ] cargo clippy --all-targets -- -D warnings
[ ] cargo test — all pass (existing + new)
```

**Exit criteria:** `cargo test` green. PromQL + LogQL transpile works. DEFINE/USE expands. Type errors caught at parse time.

### Phase 2 — "Engine" (Sprint 3-4)

**Goal:** HTTP server running, single-signal queries execute against real backends.

```
[ ] Cargo workspace (uniql-core, uniql-engine, uniql-cli)
[ ] uniql-engine scaffold (axum + tokio + reqwest)
[ ] Backend config (TOML/YAML)
[ ] POST /v1/query — single signal end-to-end
[ ] PrometheusExecutor — VictoriaMetrics HTTP client
[ ] LokiExecutor — Loki HTTP client
[ ] POST /v1/validate
[ ] POST /v1/explain
[ ] GET /health
[ ] Docker image (multi-stage build)
[ ] AETHERIS docker-compose integration
[ ] Real data testing with AETHERIS VictoriaMetrics + Loki
```

**Exit criteria:** `curl POST /v1/query` returns real data from VictoriaMetrics and Loki.

### Phase 3 — "Correlate" (Sprint 5-7)

**Goal:** Multi-signal queries, CORRELATE working, real RCA scenarios in AETHERIS.

```
[ ] Query planner / decomposer
[ ] Parallel executor (tokio::try_join_all)
[ ] Result merger (timestamp alignment)
[ ] CORRELATE ON field WITHIN window implementation
[ ] CORRELATE strategies: TimeFieldJoin, FieldJoin, TraceIdJoin
[ ] SKEW_TOLERANCE implementation
[ ] CARDINALITY_LIMIT protection
[ ] Multi-signal end-to-end tests
[ ] AETHERIS SNMP + Syslog correlation scenario
[ ] TraceQL transpiler (basic span selection)
```

**Exit criteria:** Multi-signal correlation query returns correlated results from real backends.

### Phase 4 — "Ecosystem" (Sprint 8-9, parallel with Phase 3)

**Goal:** WASM, npm, web playground, Grafana plugin, public release.

```
[ ] WASM build (wasm-pack)
[ ] @uniql/core npm package
[ ] Web playground (React + Monaco + WASM)
[ ] Monaco syntax definition (uniql-monarch.ts)
[ ] TextMate grammar for VS Code
[ ] Grafana datasource plugin (alpha)
[ ] PromQL → UNIQL import wizard (v1.1)
[ ] GitHub public repository
[ ] Documentation site
```

**Exit criteria:** Public GitHub repo, working playground, Grafana plugin alpha.

---

## 14. Technical Decisions Log

| # | Decision | Choice | Rationale | Alternatives Considered | Research Evidence |
|---|----------|--------|-----------|------------------------|-------------------|
| D-01 | Core Language | Rust | Parser/compiler ideal. WASM target. Memory safety. Performance. | Go, C++ | - |
| D-02 | Architecture | Full Execution Engine | Transpiler-only = syntax sugar. CORRELATE needs runtime. | Transpiler-only | Trino/Presto model |
| D-03 | Syntax Style | SQL-familiar primary, `\|>` pipe secondary | SQL universally known. Pipe for exploration. Both → same AST. | Pipe-only (KQL), SQL-only | Google SQL Pipe, no successful dual-primary language exists |
| D-04 | Pipe Operator | `\|>` not `\|` | Industry standard. `\|` conflicts with LogQL and bitwise OR. | `\|` (KQL-style) | Google SQL Pipe VLDB 2024, Spark 4.0, TC39, PHP 8.5 |
| D-05 | Aggregation Keyword | `COMPUTE` | Domain-specific identity. Avoids SQL `SELECT` expectations. | `SELECT`, `summarize`, `aggregate`, `stats` | KQL/SPL/ES\|QL all avoid SELECT. CNCF TAG notes SQL basis but with extensions |
| D-06 | Macro System | DEFINE/USE in v1.0 | PromQL #1 complaint is no variables. MetricsQL solved it in ~300 LOC. Deferral risk > inclusion risk. | Defer to v2.0 | MetricsQL WITH success, PromQL GitHub #11609 pain |
| D-07 | Macro Semantics | Expand-before-transpile, lexical scope, no recursion | Simple, safe, transpiler-agnostic. MetricsQL proven model. | Runtime expansion, dynamic scope | dbt Jinja cautionary tale, MetricsQL model |
| D-08 | Type System | Signal-level validation v1.0, expression-level v1.x | Retrofitting types is 3-5x more expensive. But full gradual typing is premature. | No types, full gradual typing | Java generics (20yr pain), Python mypy (3yr retrofit), TypeScript (gradual success) |
| D-09 | Error Messages | Good-not-perfect v1.0 (location + context + suggestions) | Highest ROI. 4-8 weeks. Differentiator. | Minimal errors, full Elm/Rust quality | Elm: 8yr investment, Rust: 10yr+. Phases 1-3 achievable in weeks |
| D-10 | LSP | Not in v1.0. TextMate grammar instead. LSP v1.1+ | No major language shipped LSP at v1.0. Query languages live in web UI/CLI. | LSP in v1.0 | Rust LSP 3yr after 1.0, Gleam built incrementally |
| D-11 | ML Features | Only `predict_linear` in v1.0. Rest v2.0+ | No query language shipped ML at v1.0. Bad abstraction risk. | Full ML spec | KQL/SPL/NRQL all added ML years later |
| D-12 | Reverse Transpile | v1.1 import wizard (PromQL→UNIQL, 80% coverage) | Full bidirectional too expensive, round-trip lossy. One-way import sufficient. | Full bidirectional v1.0 | sqlglot model, Chronosphere/Logz.io import tools |
| D-13 | Dog-Food | AETHERIS | Real VictoriaMetrics + Loki data. Validates SNMP + Syslog correlation. | Synthetic data only | - |

---

## 15. Success Metrics

### 15.1 Phase 1 (Foundation)
- `cargo build` succeeds
- `cargo test` — all pass
- `cargo clippy` — zero warnings
- PromQL transpilation: 15+ test cases pass
- LogQL transpilation: 10+ test cases pass
- DEFINE/USE: 5+ test cases pass
- Signal-type validation: catches invalid combinations

### 15.2 Phase 2 (Engine)
- Single-signal query end-to-end latency < 500ms (parse + transpile + backend)
- VictoriaMetrics query returns real metric data
- Loki query returns real log data
- /health reports all backends reachable
- Docker image size < 50MB

### 15.3 Phase 3 (Correlate)
- Multi-signal query returns correlated results
- CORRELATE accuracy: timestamps within window match correctly
- AETHERIS SNMP + Syslog scenario produces valid RCA timeline
- No data loss in correlation (all matching records returned)

### 15.4 Phase 4 (Ecosystem)
- WASM bundle size < 2MB
- Playground loads in < 3s
- Grafana plugin: basic query execution works
- npm package published

---

## 16. Risk Register

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| CORRELATE performance at scale (O(n^2) join) | High | Medium | CARDINALITY_LIMIT, streaming merge, index-based join |
| Backend API breaking changes (Loki v3, VM v2) | Medium | Low | Versioned executor trait, adapter pattern |
| CNCF standard diverges from UNIQL syntax | Medium | Medium | Monitor TOC #1770, align where possible, transpiler can adapt |
| DEFINE/USE misuse (overly complex macros) | Low | Medium | No recursion, no conditionals, depth limit |
| Clock skew in correlation | High | High | SKEW_TOLERANCE parameter, NTP recommendations |
| Single developer bus factor | High | High | Comprehensive tests, clear docs, open source community |

---

## 17. Reserved Keywords

All reserved keywords for current and future use:

**v1.0 (implemented):**
`FROM`, `WHERE`, `WITHIN`, `COMPUTE`, `SHOW`, `CORRELATE`, `ON`, `GROUP`, `BY`, `HAVING`, `AS`, `AND`, `OR`, `NOT`, `IN`, `LAST`, `TO`, `PARSE`, `FILTER`, `DEFINE`, `USE`, `CONTAINS`, `STARTS`, `WITH`, `MATCHES`, `TODAY`, `THIS_WEEK`

**v1.x (reserved, not yet implemented):**
`FILL`, `EXPLAIN`, `VALIDATE`, `EXTRACT`, `PROJECT`, `EXTEND`, `ALIGN`

**v2.0 (reserved, future):**
`DETECT`, `CLUSTER`, `DIFF`, `PREDICT`, `TOPOLOGY`, `SUBSCRIBE`, `STREAM`, `WINDOW`, `EMIT`

---

## 18. Appendix: Language Grammar (EBNF)

```ebnf
query          = { define_clause } , [ from_clause ] ,
                 { pipe_stage | clause } ;

pipe_stage     = "|>" , clause ;

clause         = where_clause | within_clause | parse_clause |
                 compute_clause | group_by_clause | having_clause |
                 correlate_clause | show_clause ;

define_clause  = "DEFINE" , identifier , [ "(" , param_list , ")" ] ,
                 "=" , ( expr | "(" , query , ")" ) ;

from_clause    = "FROM" , source_list ;
source_list    = data_source , { "," , data_source } ;
data_source    = signal_type , [ ":" , backend_hint ] , [ "AS" , alias ] ;
signal_type    = "metrics" | "logs" | "traces" | "events" | identifier ;

where_clause   = ( "WHERE" | "FILTER" ) , expr ;
within_clause  = "WITHIN" , time_spec ;
time_spec      = "last" , duration
               | string_lit , "TO" , string_lit
               | "today" | "this_week" ;

parse_clause   = "PARSE" , parse_mode ;
parse_mode     = "json" | "logfmt"
               | "pattern" , string_lit
               | ( "regexp" | "regex" ) , string_lit ;

compute_clause = "COMPUTE" , compute_func , { "," , compute_func } ;
compute_func   = identifier , "(" , [ arg_list ] , ")" , [ "AS" , alias ] ;

group_by_clause = "GROUP" , "BY" , expr_list ;
having_clause  = "HAVING" , expr ;

correlate_clause = "CORRELATE" , "ON" , ident_list ,
                   [ "WITHIN" , duration ] ,
                   [ "SKEW_TOLERANCE" , duration ] ,
                   [ "CARDINALITY_LIMIT" , number ] ;

show_clause    = "SHOW" , show_format ;
show_format    = "timeseries" | "table" | "timeline" | "heatmap" |
                 "flamegraph" | "topology" | "count" | "alert" ;

expr           = prefix_expr , { infix_op , prefix_expr }
               | expr , "CONTAINS" , string_lit
               | expr , "STARTS" , "WITH" , string_lit
               | expr , "MATCHES" , string_lit
               | expr , "IN" , "[" , expr_list , "]"
               | expr , "NOT" , "IN" , "[" , expr_list , "]" ;

prefix_expr    = "NOT" , expr
               | "(" , expr , ")"
               | function_call
               | qualified_ident
               | identifier
               | string_lit
               | number_lit
               | duration_lit
               | "*" ;

function_call  = identifier , "(" , [ arg_list ] , ")" ;
qualified_ident = identifier , { "." , identifier } ;
infix_op       = "=" | "!=" | ">" | "<" | ">=" | "<=" | "=~" | "!~"
               | "AND" | "OR"
               | "+" | "-" | "*" | "/" | "%" ;

ident_list     = identifier , { "," , identifier } ;
expr_list      = expr , { "," , expr } ;
arg_list       = expr , { "," , expr } ;
param_list     = identifier , { "," , identifier } ;

duration       = number , duration_unit ;
duration_unit  = "ms" | "s" | "m" | "h" | "d" | "w" | "y" ;
```

---

*UNIQL Baseline v1.0 — Samet Yagci / AETHERIS*
*Research-backed decisions. NASA-grade specification. Build it, use it, fix it.*
