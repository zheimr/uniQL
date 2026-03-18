# UNIQL Roadmap
**Last updated:** 2026-03-18 (v0.2.0)

---

## Phase 1 — "Fix & Foundation" [DONE]

```
[x] Build-breaking bug fix (pub mod logql)
[x] PromQL BUG-1: OR conditions silently dropped → regex matcher
[x] PromQL BUG-2: HAVING invalid output → comparison only
[x] config.rs — centralized constants (DEFAULT_RANGE_DURATION, QUANTILE_MAP)
[x] Transpiler trait interface (BackendType, TranspileOutput, get_transpiler())
[x] PromQL transpiler → trait migration (PromQLTranspiler struct)
[x] LogQL transpiler (Loki) — stream selector, line filter, parser stages, metric queries
[x] LogsQL transpiler (VictoriaLogs) — _stream:{}, _msg, unpack_json, stats pipe
[x] DEFINE/USE macro expansion pass — lexical scope, no recursion, expand-before-transpile
[x] Signal-type semantic validation — PARSE on logs only, CORRELATE required for multi-signal
[x] |> pipe syntax parser support — same AST as SQL-style
[x] Unused code cleanup
```

**Output:** 3 working transpilers (PromQL, LogQL, LogsQL), DEFINE/USE macros, type validation, pipe syntax.

---

## Phase 2 — "Engine" [DONE]

HTTP execution engine. Single-signal queries execute against real backends.

```
[x] Cargo workspace restructure (uniql-core, uniql-engine, uniql-wasm)
[x] uniql-engine scaffold (axum + tokio + reqwest/rustls)
[x] Backend config (env JSON — backend name, type, URL)
[x] POST /v1/query — parse → validate → transpile → execute → return
[x] PrometheusExecutor — VictoriaMetrics HTTP client (/api/v1/query)
[x] VictoriaLogsExecutor — VictoriaLogs HTTP client (/select/logsql/query)
[x] POST /v1/validate — parse + semantic check, return AST summary
[x] POST /v1/explain — show execution plan with native queries
[x] GET /health — backend reachability check
[x] Docker image (multi-stage build)
[x] Docker-compose with AETHERIS network (readonly)
[x] Real data testing: VictoriaMetrics + VictoriaLogs — metrics, logs, stats, pipe syntax
```

**Exit criteria met:** `curl POST /v1/query` returns real FortiGate logs from VictoriaLogs and real metrics from VictoriaMetrics.

---

## Phase 3 — "Correlate" [DONE]

Multi-signal queries. CORRELATE working. Real RCA scenarios.

```
[x] Query planner / decomposer — split multi-signal query into sub-queries
[x] Parallel executor (futures::join_all) — concurrent backend calls
[x] Result merger — field normalization
[x] CORRELATE ON field WITHIN window — TimeFieldJoin strategy
[x] Correlation metadata (metrics_count, logs_count, correlated_count, strategy)
[x] Multi-signal end-to-end test — metrics (14 series) + logs (3 entries), 18ms
[ ] CORRELATE ON trace_id — TraceIdJoin strategy [deferred: no Tempo]
[ ] CARDINALITY_LIMIT protection [deferred: needs benchmarking]
[ ] TraceQL transpiler [deferred: no Tempo in stack]
```

**Exit criteria met:** Multi-signal CORRELATE query returns parallel results from VictoriaMetrics + VictoriaLogs with correlation engine.

---

## Phase 3.5 — "12-Layer Architecture" [DONE]

Pipeline 7 → 12 layer refactoring. Eliminates critical duplication, fixes 6 bugs, adds 5 missing layers identified from research of 14 production systems (Trino, Calcite, DataFusion, DuckDB, Mimir, VictoriaMetrics, rustc, sqlglot).

```
[x] Binder (uniql-core) — unified ident resolution, condition classification, stream label whitelist, OR flattening
[x] Normalizer (uniql-core) — duration parsing, aggregation extraction, percentile resolution, HAVING pre-computation
[x] transpile_normalized() trait method — new path with fallback to legacy
[x] PromQL transpile_normalized — reads BoundConditions directly (~100 lines dedup)
[x] LogQL transpile_normalized — reads BoundConditions directly, OR handling fixed
[x] LogsQL transpile_normalized — reads BoundConditions directly, HAVING bug fixed
[x] Mirror tests — old path == new path assertion for all 3 transpilers
[x] ResultNormalizer (uniql-engine) — backend-specific JSON → uniform NormalizedResult
[x] Proper RFC3339 timestamp parsing (replaces stub that returned None)
[x] correlate_normalized() — accepts NormalizedResult, same algorithm
[x] Response Formatter (uniql-engine) — SHOW clause + format parameter + limit
[x] Middleware (uniql-engine) — x-request-id header, 60s request timeout
[x] Planner uses normalized transpile path with fallback
[x] SHOW clause passthrough in QueryPlan
[x] Code review + fixes — middleware order, HAVING op mapping, test gaps
```

**Pipeline (12 layers):**
```
 1. Lexer            (existing)
 2. Parser           (existing)
 3. Expander         (existing)
 4. Binder           ← NEW  uniql-core/src/bind/mod.rs
 5. Validator        (existing)
 6. Normalizer       ← NEW  uniql-core/src/normalize/mod.rs
 7. Transpiler       (existing, +transpile_normalized path)
 8. Middleware       ← NEW  uniql-engine/src/middleware_layers.rs
 9. Executor         (existing)
10. ResultNormalizer ← NEW  uniql-engine/src/normalize_result/mod.rs
11. Correlator       (existing, +correlate_normalized)
12. Formatter        ← NEW  uniql-engine/src/format/mod.rs
```

**Bugs fixed:**
```
[x] BUG-3: LogsQL HAVING hardcodes "count(*)" → normalizer uses actual aggregate
[x] BUG-4: LogQL OR falls through silently → binder flattens to BoundOrGroup
[x] BUG-5: LogQL/LogsQL stream label whitelists inconsistent → unified in binder
[x] BUG-6: Correlator timestamp parsing returns None → proper RFC3339 parser
[x] BUG-7: SHOW clause ignored → formatter applies it
[x] BUG-8: QueryRequest.format ignored → formatter uses it
```

**Tests:** 93 original + 68 new = 161 total, zero clippy warnings.

**Exit criteria met:** All original tests pass, mirror tests confirm old == new output, zero clippy warnings.

---

## Phase 3.75 — "Hardening" [DONE]

Self-assessment: 6.5/10 → 8/10 after hardening.

### Step 1: Parser Safety + Public API Error Types [DONE]
```
[x] MAX_EXPR_DEPTH enforcement — parser depth counter, ParseError::MaxDepthExceeded at 64
[x] Public API typed error enum — Result<T, String> → Result<T, UniqlError>
    - UniqlError { Lex, Parse, Expand, Semantic, Bind, Normalize, Transpile }
    - Legacy API (parse_str, prepare_str) kept for backward compat
[x] Input size limit — MAX_QUERY_SIZE = 64KB, ParseError::QueryTooLarge
```

### Step 2: Engine Security [DONE]
```
[x] Request body size limit — axum DefaultBodyLimit::max(256KB)
[x] API key auth middleware — x-api-key header, UNIQL_API_KEYS env (comma-separated)
    - Health endpoint exempt, empty keys = auth disabled
[x] CORS origin whitelist — UNIQL_CORS_ORIGINS env, empty = permissive (backward compat)
[x] Graceful shutdown — tokio signal handler (SIGTERM + Ctrl+C), in-flight request draining
```

### Step 3: Config & Ops [DONE]
```
[x] toml crate — hand-rolled parser replaced with toml = "0.8"
[x] Query audit log — structured tracing middleware (method, path, status, duration_ms)
[ ] /metrics endpoint — Prometheus format [deferred to Phase 4]
[ ] /health backend probe — per-backend HTTP check [deferred to Phase 4]
```

### Step 4: CI/CD [DONE]
```
[x] GitHub Actions workflow — test + clippy + fmt + build-release + docker
[x] Version bump — 0.1.0 → 0.2.0
```

**Tests:** 170 total (165 core + 5 engine), zero clippy warnings.

**Exit criteria met:** Typed errors, depth limit enforced, API key auth, body limit, CORS configurable, graceful shutdown, toml crate, audit log, CI workflow, v0.2.0.

---

## Phase 3.9 — "Competitive Moat" (eleştirileri göm)

Based on deep research: 5 parallel analysis tracks, 80+ sources, 14 production systems analyzed.
See COMPETITIVE_STRATEGY.md for full analysis.

### Step 1: NATIVE Clause [DONE]
```
[x] AST: Expr::Native { backend: Option<String>, query: String }
[x] Lexer: NATIVE keyword
[x] Parser: NATIVE("query") and NATIVE("backend", "query") syntax
[x] Binder: BoundCondition::Native passthrough
[x] PromQL: native fragments injected into selector, full-query native, backend mismatch error
[x] LogQL: native fragments as label filters, backend mismatch error
[x] LogsQL: native fragments as field filters, backend mismatch error
[x] Tests: 8 new tests (full query, partial, backend match, backend mismatch × 3 transpilers)
```

### Step 2: Hash Join Correlator [DONE]
```
[x] HashMap<CompositeKey, Vec<&FlatEntry>> build phase — O(m)
[x] Sorted buckets by timestamp_epoch
[x] Binary search (partition_point) for time window boundaries — O(log k) per probe
[x] Replace nested loop in correlate_normalized() — O(m×l) → O(m + l·log k)
[x] Strategy renamed: "TimeFieldJoin" → "HashTimeWindowJoin"
[ ] CORRELATE ... MODE closest (ASOF semantics) [deferred: needs syntax change]
[ ] Benchmark: 10K×100K target [needs real data test]
```

### Step 3: Investigation Packs [DONE]
```
[x] POST /v1/investigate endpoint — pack name + params → parallel execution
[x] Built-in packs: high_cpu, error_spike, latency_degradation, link_down
[x] $param substitution in pack templates
[x] Custom pack: pack="custom" + queries array for ad-hoc investigation
[x] Parallel execution via futures::join_all — all pack queries run concurrently
[x] Per-query error isolation — one failing query doesn't block others
[x] Loose coupling — AETHERIS only calls HTTP, never imports UNIQL internals
```

**AETHERIS integration contract (stable HTTP API):**
```
POST /v1/investigate
{
  "pack": "link_down",
  "params": { "host": "router-fw-01" }
}
→ Returns: parallel results from 3 queries (interface_errors, syslog_events, interface_status)
```

**Exit criteria met:** NATIVE clause working, hash join O(m+l·log k), investigation packs with 4 built-in + custom.

---

## Phase 4 — "Ecosystem"

Public release. WASM, npm, playground, Grafana plugin.

```
[ ] WASM build (wasm-pack) — parse(), to_promql(), to_logql(), to_logsql(), validate()
[ ] @uniql/core npm package
[ ] Web playground (React + Monaco + WASM)
    - Monaco syntax definition (uniql-monarch.ts)
    - Three-pane view: UNIQL | Native Query | Result
    - Live transpilation preview (<1ms)
    - Example query picker
[ ] TextMate grammar for VS Code syntax highlighting
[ ] Grafana datasource plugin (alpha)
[ ] PromQL → UNIQL import wizard (best-effort, ~80% coverage)
[ ] /metrics Prometheus endpoint
[ ] /health backend probe
[ ] GitHub public repository
[ ] Documentation site (rustdoc API docs + deployment guide + query reference)
```

**Exit criteria:** Public repo, working playground, Grafana plugin alpha.

**Puan hedefi:** 8/10 → 9/10

---

## v1.x — Post-Launch

```
[ ] NL→UNIQL API endpoint — LLM generates, UNIQL validates (POST /v1/ask)
    - RAG pipeline: schema context + example pairs + domain docs
    - Self-correction loop: parse error → LLM retry (DIN-SQL pattern)
    - Target: %80+ accuracy on standard observability queries
[ ] Schema introspection from backends (metric names, label names, log fields)
    - Feeds NL pipeline with context for better accuracy
[ ] LSP server (diagnostics → hover → autocomplete → go-to-definition)
[ ] Expression-level type inference (metrics.cpu → numeric, logs.message → string)
[ ] Rate limiting — per-IP/per-API-key throttle (tower middleware)
[ ] LokiExecutor (if Loki added to stack)
[ ] ElasticsearchExecutor
[ ] FILL clause (fill gaps in time series)
[ ] EXTRACT clause (SPL rex equivalent)
[ ] MetricsQL-specific extensions (keep_metric_names, WITH)
[ ] PromQL → UNIQL bidirectional transpile (full)
[ ] Recording rules / saved queries
[ ] Query caching layer
[ ] CORRELATE ON trace_id — TraceIdJoin strategy
[ ] CARDINALITY_LIMIT protection
[ ] SKEW_TOLERANCE with proper chrono parsing
[ ] Remove legacy transpile() codepath (once all tests use normalized path)
[ ] CNCF TAG Observability QLS working group engagement
```

---

## v2.0 — Future

```
[ ] AETHERIS chatbox NL integration — "son 1 saatte CPU %90 üstü sunucuları göster"
[ ] NL→UNIQL training data flywheel (Vanna pattern: successful queries → training data)
[ ] DETECT ANOMALY — anomaly detection on time series
[ ] CLUSTER — log clustering
[ ] DIFF — compare two time ranges
[ ] PREDICT — linear/exponential prediction
[ ] SUBSCRIBE / STREAM — streaming query results (WebSocket)
[ ] TOPOLOGY-aware correlation (service graph + CMDB)
[ ] Statistical correlation (Pearson/Spearman)
[ ] UNIQL ↔ PromQL/LogQL bidirectional with full fidelity
[ ] Plugin system — community-contributed transpilers
[ ] Distributed engine (multiple UNIQL nodes)
[ ] TraceQL transpiler
```

---

## Backend Support Matrix

| Backend | Signal | Phase 2 | Phase 3 | v1.x | v2.0 |
|---------|--------|---------|---------|------|------|
| VictoriaMetrics (PromQL/MetricsQL) | metrics | Impl | - | - | - |
| VictoriaLogs (LogsQL) | logs | Impl | - | - | - |
| Loki (LogQL) | logs | - | If needed | Impl | - |
| Tempo (TraceQL) | traces | - | Spec | Impl | - |
| Elasticsearch (ES DSL) | logs | - | - | Spec | Impl |
| ClickHouse (SQL) | metrics+logs | - | - | - | Impl |

---

## Architecture (as of Phase 3.5)

```
┌──────────────────── uniql-core ────────────────────┐
│  UNIQL input                                        │
│    → 1. Lexer (tokenize)                            │
│    → 2. Parser (AST)                                │
│    → 3. Expander (DEFINE macros)                    │
│    → 4. Binder (ident resolution, condition class.) │
│    → 5. Validator (semantic checks)                 │
│    → 6. Normalizer (durations, agg, HAVING)         │
│    → 7. Transpiler (PromQL / LogQL / LogsQL)        │
│  native query string                                │
└─────────────────────────────────────────────────────┘
                        ↓
┌──────────────────── uniql-engine ──────────────────┐
│    → 8. Middleware (request-id, timeout)            │
│    → 9. Executor (HTTP → backends)                 │
│    → 10. ResultNormalizer (uniform schema)          │
│    → 11. Correlator (multi-signal join)             │
│    → 12. Formatter (SHOW clause, limit)             │
│  JSON response                                      │
└─────────────────────────────────────────────────────┘
```

---

*UNIQL Roadmap — Build it, use it, fix it.*
