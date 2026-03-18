# UNIQL Layer Architecture Research

## Deep analysis of 14 production systems to determine the optimal pipeline for a unified observability query language.

---

## PART 1: PIPELINE ANATOMY OF EVERY RESEARCHED SYSTEM

---

### 1. TRINO/PRESTO (Distributed SQL Query Engine)

```
Stage 1: SQL Parsing        │ String → AST (parse tree)
Stage 2: Analysis            │ AST → Annotated AST (name/type resolution against catalog)
Stage 3: Logical Planning    │ Annotated AST → Logical Plan (relational algebra tree)
Stage 4: Rule-Based Optim.   │ Logical Plan → Optimized Logical Plan (70+ optimization phases)
Stage 5: Cost-Based Optim.   │ Optimized Logical Plan → CBO-reordered Plan (join reordering, predicate pushdown)
Stage 6: Plan Fragmentation  │ Optimized Plan → Distributed Plan (stages + tasks for workers)
Stage 7: Task Scheduling     │ Distributed Plan → Assigned Tasks (data-locality-aware placement)
Stage 8: Pipelined Execution │ Tasks → Pages (columnar data streamed between stages)
Stage 9: Result Assembly     │ Pages → Final Result (coordinator merges worker results)
```

**Key insight:** The separation of rule-based optimization (always beneficial rewrites) from cost-based optimization (statistics-dependent choices) is a universal pattern. Trino's 70+ optimization phases execute sequentially — each phase transforms the entire plan tree.

**Error handling:** Every stage produces typed errors. The EXPLAIN and EXPLAIN ANALYZE commands expose the plan at any stage, including actual execution statistics.

---

### 2. APACHE CALCITE (Universal Query Planning Framework)

```
Stage 1: SQL Parsing         │ String → SqlNode AST (JavaCC parser)
Stage 2: SQL Validation      │ SqlNode → Validated SqlNode (name resolution, type inference, scope checking)
Stage 3: Rel Conversion      │ Validated SqlNode → LogicalRelNode tree (relational algebra)
Stage 4: Query Optimization  │ LogicalRelNode → PhysicalRelNode (VolcanoPlanner or HepPlanner)
Stage 5: Code Generation     │ PhysicalRelNode → Java expressions (Janino bytecode)
Stage 6: Execution           │ Compiled plan → Enumerable result iterator
```

**Key insight:** Calcite separates VALIDATION (stage 2) from REL CONVERSION (stage 3). The validated AST is still SQL-shaped; only after conversion does it become relational algebra. This two-step approach means validation logic doesn't need to understand relational algebra, and the relational conversion doesn't need to handle invalid inputs.

**Optimization architecture:** Two planner engines: VolcanoPlanner (cost-based, dynamic programming) and HepPlanner (rule-based, deterministic). 100+ transformation rules in CoreRules. Cost model driven by RelMetadataQuery statistics.

**Critical to UNIQL:** Calcite's adapter architecture lets different data sources plug in via "conventions" — directly analogous to UNIQL's backend transpilers.

---

### 3. APACHE DATAFUSION (Rust-Native Query Engine)

```
Stage 1: SQL Parsing         │ String → AST (sqlparser crate)
Stage 2: Logical Planning    │ AST → LogicalPlan enum (SqlToRel conversion)
Stage 3: Analysis            │ LogicalPlan → Validated LogicalPlan (type coercion, semantic checks)
Stage 4: Logical Optimization│ Validated LogicalPlan → Optimized LogicalPlan (~30 rules)
Stage 5: Physical Planning   │ Optimized LogicalPlan → ExecutionPlan (concrete operators)
Stage 6: Physical Optimization│ ExecutionPlan → Optimized ExecutionPlan (join reorder, partition pruning)
Stage 7: Execution           │ Optimized ExecutionPlan → Stream of Arrow RecordBatch
```

**Key insight:** DataFusion has BOTH logical optimization (stage 4) AND physical optimization (stage 6). Logical optimization operates on abstract plan nodes (e.g., "push filter below join"). Physical optimization operates on concrete execution strategies (e.g., "use hash join instead of nested loop given these partition counts"). These are fundamentally different concerns.

**Optimization rules include:** PushDownFilter, CommonSubexprEliminate, SimplifyExpressions, PushDownLimit, PushDownProjection, EliminateOuterJoin, RewriteDisjunctivePredicate.

---

### 4. CLICKHOUSE (Analytical Query Engine)

```
Stage 1: SQL Parsing         │ String → AST
Stage 2: Analysis/Binding    │ AST → Resolved AST (table/column resolution)
Stage 3: Logical Planning    │ Resolved AST → Query Plan
Stage 4: Optimization        │ Query Plan → Optimized Plan (prune, pushdown)
Stage 5: Pipeline Building   │ Optimized Plan → Processor Pipeline (sources + transforms + sinks)
Stage 6: Parallel Execution  │ Pipeline → Chunks (parallel across CPU cores)
Stage 7: Result Merging      │ Chunks → Final Result (format + return)
```

**Key insight:** ClickHouse's pipeline model of "processors with ports" (input ports, output ports, data chunks flowing between them) is an elegant execution abstraction. The pipeline can be "pulling" (SELECT), "pushing" (INSERT), or "completed" (INSERT SELECT).

**Query pipeline types:** The EXPLAIN PIPELINE command shows the execution DAG with parallelism annotations ("x N" where N = thread count).

---

### 5. DUCKDB (Single-Node Analytical Engine)

```
Stage 1: Parsing             │ String → PG-format AST → DuckDB SQLStatement
Stage 2: Binding             │ SQLStatement → BoundStatement (catalog resolution)
Stage 3: Logical Planning    │ BoundStatement → LogicalOperator tree
Stage 4: Logical Optimization│ LogicalOperator tree → Optimized LogicalOperator tree (26 built-in optimizers)
Stage 5: Physical Planning   │ Optimized Logical Plan → PhysicalOperator tree
Stage 6: Pipeline Building   │ Physical Plan → Pipelines (split at pipeline breakers)
Stage 7: Pipeline Execution  │ Pipelines → 4 events: Initialize → Execute → Finish → Complete
```

**Key insight:** DuckDB's concept of "pipeline breakers" — operators that must consume all input before producing output (e.g., hash build side, sort, aggregate) — determines where pipelines split. This is irrelevant for UNIQL now but important if UNIQL ever does client-side aggregation.

**Optimization highlights:** Expression Rewriter (constant folding, simplification), Filter Pushdown, Join Order Optimizer (DPccp algorithm), 26 total optimization passes.

---

### 6. VELOX (Meta's Unified Execution Engine)

```
Stage 1: Plan Reception      │ Optimized plan fragment (from Presto/Spark) → Task
Stage 2: Pipeline Decomp.    │ Task → Linear Pipeline sub-trees
Stage 3: Driver Instantiation│ Pipeline → N Drivers (parallel execution threads)
Stage 4: Vectorized Execution│ Drivers → Velox vectors (columnar SIMD processing)
Stage 5: Memory Management   │ Hierarchical memory pools with zero-fragmentation allocator
```

**Key insight:** Velox does NOT parse or optimize queries — it only executes pre-optimized plans. This proves that execution is a separable concern. UNIQL similarly delegates execution to backends (VictoriaMetrics, Loki) rather than executing queries itself.

---

### 7. GRAFANA MIMIR (PromQL Processing)

```
Stage 1: HTTP Reception      │ HTTP request → PromQL string + parameters
Stage 2: Middleware Chain     │ PromQL → Processed PromQL (5 middleware stages):
         ├── Limits Enforce   │ Check per-tenant and global query limits
         ├── Query Splitting  │ Split long time ranges into sub-queries
         ├── Results Caching  │ Check/serve cached results
         ├── Query Sharding   │ Shard high-cardinality queries for parallelism
         └── Retry Logic      │ Auto-retry failed queries
Stage 3: Query Scheduling    │ Processed query → Scheduled work unit (fair queuing)
Stage 4: Querier Execution   │ Work unit → Data fetch from ingesters + store-gateways
Stage 5: Data Merging        │ Multi-source data → Deduplicated SeriesSet
Stage 6: PromQL Evaluation   │ SeriesSet → Query engine evaluation (MQE streaming engine)
Stage 7: Response Return     │ Evaluation result → JSON through middleware chain
```

**Key insight for UNIQL:** The MIDDLEWARE CHAIN pattern is the most important architectural lesson from observability systems. Mimir wraps query processing in composable middleware that handles orthogonal concerns (limits, caching, splitting, sharding, retries) without modifying core query logic. Each middleware is independent and can be enabled/disabled/reordered.

---

### 8. VICTORIAMETRICS (MetricsQL/PromQL Processing)

```
Stage 1: HTTP Request Parse  │ HTTP params → EvalConfig (query, start, end, step, timeout)
Stage 2: Parameter Validation│ EvalConfig → Validated EvalConfig (start≤end, step bounds, points limit)
Stage 3: Query Parse + Cache │ Query string → AST (with parse cache for repeated queries)
Stage 4: Query Optimization  │ AST → Optimized AST (filter pushdown to storage)
Stage 5: Expression Eval     │ AST node dispatch: MetricExpr→fetch, RollupExpr→compute, AggrFuncExpr→aggregate, BinaryOpExpr→math
Stage 6: Parallel Data Fetch │ SearchQuery → packedTimeseries (work-stealing parallel workers)
Stage 7: Deduplication       │ Raw samples → Deduplicated series (min-heap, configurable interval)
Stage 8: Rollup Computation  │ Deduplicated data → Computed results (with rollup result cache)
Stage 9: Result Formatting   │ Results → Sorted, rounded, JSON-marshalled response
```

**Key insights for UNIQL:**
- **Parse cache:** VM caches parsed ASTs for repeated queries. UNIQL should consider this.
- **Concurrency control:** Buffered channel with capacity set by max concurrent requests. Query queue with max wait duration, returning 429 if exceeded.
- **Two-generation result cache:** Current + previous cache with rotation every 30 minutes.

---

### 9. CORTEX (Distributed PromQL Pipeline)

```
Stage 1: Query Frontend      │ HTTP request → Middleware processing
         ├── Queue            │ Internal FIFO queue with fair scheduling
         ├── Splitting        │ Long queries → Time-range sub-queries
         └── Caching          │ Result cache with sub-query gap-filling
Stage 2: Query Scheduler     │ Queue → Fair-queued work distribution (optional separate service)
Stage 3: Querier             │ Work unit → PromQL evaluation against store
Stage 4: Result Assembly     │ Sub-query results → Merged final result
```

**Key insight:** Cortex invented the query-frontend → query-scheduler → querier pattern that Mimir and Loki inherited. The separation of "accepting and optimizing queries" from "scheduling them fairly" from "executing them" enables independent scaling of each tier.

---

### 10. THANOS (Distributed PromQL Layer)

```
Stage 1: Query Reception     │ HTTP PromQL request → Querier
Stage 2: Logical Planning    │ PromQL → Declarative (logical) plan
Stage 3: Optimization        │ Logical plan → Optimized plan (multiple optimizer passes)
Stage 4: Physical Planning   │ Optimized plan → Operator tree (Volcano-model)
Stage 5: Store Fanout        │ Operator tree → Parallel requests to StoreAPI endpoints
Stage 6: Series Merging      │ Multi-store results → Merged, deduplicated series
Stage 7: Expression Eval     │ Merged series → PromQL evaluation result
```

**Key insight:** Thanos's distributed mode (`--query.mode=distributed`) breaks queries into INDEPENDENT FRAGMENTS and delegates them to remote queriers. This is exactly what UNIQL's planner does when decomposing multi-signal queries. Thanos's optimizer passes (before execution) transform the logical plan for better distributed execution — an approach UNIQL should adopt.

---

### 11. RUST COMPILER (rustc)

```
Stage 1:  Lexing              │ Source text → Token stream
Stage 2:  Parsing             │ Token stream → AST
Stage 3:  Macro Expansion     │ AST → Expanded AST (proc macros, declarative macros)
Stage 4:  AST Validation      │ Expanded AST → Validated AST
Stage 5:  Name Resolution     │ Validated AST → Resolved symbols
Stage 6:  HIR Lowering        │ AST → HIR (desugared, compiler-friendly)
Stage 7:  Type Inference/Check│ HIR → Typed HIR (trait solving, type verification)
Stage 8:  THIR Lowering       │ Typed HIR → THIR (fully typed, method calls explicit)
Stage 9:  MIR Lowering        │ THIR → MIR (Control Flow Graph)
Stage 10: Borrow Checking     │ MIR → Validation result (ownership rules)
Stage 11: MIR Optimization    │ MIR → Optimized MIR
Stage 12: Monomorphization    │ Optimized MIR → Concrete type instantiations
Stage 13: Code Generation     │ MIR → LLVM-IR (with monomorphization)
Stage 14: LLVM Optimization   │ LLVM-IR → Optimized LLVM-IR
Stage 15: Machine Code Gen    │ Optimized LLVM-IR → Object files
Stage 16: Linking             │ Object files → Final binary
```

**Key insight for UNIQL:** Rust has MULTIPLE intermediate representations (AST → HIR → THIR → MIR → LLVM-IR), each progressively lower-level. Each IR is designed for specific analyses. The lesson: don't try to do everything on a single AST. UNIQL's current single AST works now, but if query optimization becomes complex, a lowered IR may be needed.

**Also critical:** Macro expansion (stage 3) happens BEFORE validation and type checking (stages 4-7). This matches UNIQL's current design where DEFINE expansion precedes semantic validation.

---

### 12. TYPESCRIPT COMPILER (tsc)

```
Stage 1: Scanning/Lexing     │ Source text → Token stream (scanner.ts)
Stage 2: Parsing             │ Token stream → AST (parser.ts)
Stage 3: Binding             │ AST → Symbol table (binder.ts — create scopes, connect declarations to uses)
Stage 4: Type Checking       │ AST + Symbols → Type-annotated AST (checker.ts — the largest file)
Stage 5: Transformation      │ Typed AST → JavaScript AST (strip TS constructs, downlevel features)
Stage 6: Emit                │ JavaScript AST → Output files (.js, .d.ts, .js.map)
```

**Key insight:** TypeScript's BINDING stage (stage 3) creates scopes and connects symbol declarations to their uses. This is separate from both parsing and type checking. UNIQL currently doesn't have this — name resolution happens implicitly in the transpiler. For UNIQL, a binding stage would resolve `metrics.cpu` to "the cpu metric on whatever backend handles metrics", resolve DEFINE references, and build a scope tree.

**Also critical:** The transformation stage (5) is the transpilation step. It operates on a FULLY TYPED AST, meaning all type information is available during transpilation. UNIQL's transpiler currently operates on a semantically-validated AST, which is similar.

---

### 13. GRAPHQL EXECUTION (Resolver Chain)

```
Stage 1: Parsing             │ Query string → AST (document)
Stage 2: Validation          │ AST + Schema → Validated AST (type checking, field existence)
Stage 3: Execution Planning  │ Validated AST → Execution order (resolver chain)
Stage 4: Resolver Execution  │ Execution order → Resolver chain (root → nested, with context passing)
Stage 5: Result Assembly     │ Resolver results → JSON response (tree reassembly)
```

**Key insight:** GraphQL's resolver chain model — where each field has an independent resolver that receives the parent's result — is a clean pattern for multi-backend fan-out. The `context` object passed to every resolver (containing auth, dataloaders, config) is analogous to what UNIQL should pass through its pipeline stages.

**Also relevant:** GraphQL's DataLoader pattern (batching + caching at the resolver level) prevents N+1 query problems. UNIQL could apply similar batching when making multiple requests to the same backend.

---

### 14. SQLGLOT (SQL Transpiler)

```
Stage 1: Tokenization        │ String → Token list (dialect-specific Tokenizer)
Stage 2: Parsing             │ Token list → Expression Tree (dialect-specific Parser, recursive descent)
Stage 3: AST Normalization   │ Expression Tree → Canonical Expression Tree (dialect-neutral)
Stage 4: Optimization        │ Canonical Tree → Optimized Tree (optional: scope analysis, type annotation,
         │                     expression simplification, predicate pushdown, subquery rewrite)
Stage 5: Generation          │ Expression Tree → Target SQL string (dialect-specific Generator)
```

**Key insights for UNIQL:**

1. **UNIVERSAL AST:** sqlglot's single Expression Tree (350+ node types) serves as the universal IR for ALL SQL dialects. Dialect-specific syntax is consumed during parsing and emitted during generation. The AST itself is dialect-neutral. This is EXACTLY what UNIQL's AST should be — a canonical representation that doesn't favor PromQL, LogQL, or LogsQL.

2. **N+M vs N*M:** With N source dialects and M target dialects, a universal AST requires N parsers + M generators = N+M implementations. Without it, you need N*M direct translators. UNIQL already follows this pattern (1 parser, 3 generators).

3. **Dialect system architecture:**
   ```
   Dialect
   ├── Tokenizer (subclass)  — overrides keywords, identifiers, quotes
   ├── Parser (subclass)     — overrides functions, statement handlers
   └── Generator (subclass)  — overrides transforms, type mapping, preprocessing
   ```

4. **Pre-generation AST transforms:** sqlglot's Generator.preprocess() applies dialect-specific AST rewrites BEFORE string generation. For example, converting `DISTINCT ON` to a subquery for dialects that don't support it. UNIQL's transpilers should similarly pre-process the AST before generating native query strings.

---

## PART 2: THE SUPERSET PIPELINE

Union of all stages across all 14 systems, deduplicated and categorized:

```
 #  │ STAGE                    │ INPUT → OUTPUT                          │ FOUND IN
────┼──────────────────────────┼─────────────────────────────────────────┼─────────────────────────────
 1  │ Lexing/Tokenization      │ String → Token stream                  │ ALL compilers/parsers
 2  │ Parsing                  │ Tokens → AST                           │ ALL systems
 3  │ Macro/Template Expansion │ AST → Expanded AST                     │ rustc, UNIQL (DEFINE)
 4  │ Name Resolution/Binding  │ Expanded AST → Resolved AST            │ rustc, TypeScript, DuckDB, Calcite
 5  │ Semantic Validation      │ Resolved AST → Validated AST           │ ALL systems (type check, scope check)
 6  │ AST Normalization        │ Validated AST → Canonical AST          │ sqlglot, Calcite, MLIR, rustc
 7  │ IR Lowering              │ Canonical AST → Lower IR               │ rustc (HIR/MIR), DataFusion (LogicalPlan)
 8  │ Logical Optimization     │ IR → Optimized IR                      │ Trino (70+ rules), DataFusion (30), DuckDB (26)
 9  │ Physical Planning        │ Optimized IR → Execution Plan          │ DataFusion, DuckDB, Trino, Thanos
10  │ Physical Optimization    │ Execution Plan → Optimized Exec Plan   │ DataFusion, Trino
11  │ Query Decomposition      │ Exec Plan → Sub-queries per backend    │ Thanos, Mimir, UNIQL (planner)
12  │ Middleware Processing    │ Sub-queries → Enhanced sub-queries      │ Mimir, Loki, Cortex
    │  ├── Limits Enforcement  │ Check resource/tenant limits            │ Mimir, Cortex
    │  ├── Query Splitting     │ Split by time interval                  │ Mimir, Loki, Cortex
    │  ├── Result Caching      │ Check/serve cached results              │ Mimir, Cortex, VictoriaMetrics
    │  ├── Query Sharding      │ Shard for parallelism                   │ Mimir, Loki
    │  └── Retry/Circuit Break │ Handle transient failures               │ Mimir, Cortex
13  │ Scheduling               │ Enhanced queries → Scheduled work units │ Cortex, Mimir, Trino
14  │ Transpilation/CodeGen    │ Plan → Native format (SQL/PromQL/etc)   │ sqlglot, Calcite, UNIQL
15  │ Execution                │ Native query → Raw backend results      │ ALL execution engines
16  │ Result Normalization     │ Raw results → Uniform format            │ Thanos, UNIQL (implicit)
17  │ Result Correlation/Merge │ Multi-source results → Joined result    │ Thanos, Mimir, UNIQL (correlator)
18  │ Result Formatting        │ Joined result → Client response format  │ ALL systems
```

---

## PART 3: MAPPING TO UNIQL — NOW vs LATER vs NEVER

### Current UNIQL Pipeline (7 layers):
```
1. Lexer        → tokens
2. Parser       → AST
3. Expander     → expanded AST (DEFINE/USE)
4. Validator    → validated AST (semantic rules)
5. Transpiler   → native query string
6. Executor     → HTTP backend results
7. Correlator   → merged multi-signal results
```

### Missing layers from the superset mapped to UNIQL:

```
 SUPERSET STAGE            │ STATUS      │ PRIORITY   │ REASONING
───────────────────────────┼─────────────┼────────────┼─────────────────────────────────────
 Name Resolution/Binding   │ MISSING     │ NOW        │ Currently implicit in transpiler. Painful to retrofit.
 AST Normalization/Canon.  │ MISSING     │ NOW        │ Must be between validation and transpilation.
 Query Rewrite/Optim.      │ MISSING     │ NOW        │ Even basic rewrites need a dedicated stage.
 Middleware Chain           │ MISSING     │ NOW        │ Rate limiting, caching, retries are cross-cutting.
 Result Normalization      │ MISSING     │ NOW        │ Currently mixed into correlator. Must separate.
 IR Lowering               │ NOT NEEDED  │ LATER      │ Single AST works until complexity demands multiple IRs.
 Logical Optimization      │ NOT NEEDED  │ LATER      │ Meaningful only when UNIQL has subqueries/joins.
 Physical Planning         │ NOT NEEDED  │ LATER      │ Relevant only with client-side execution.
 Physical Optimization     │ NOT NEEDED  │ LATER      │ Same as above.
 Scheduling                │ NOT NEEDED  │ LATER      │ Needed at scale (multi-tenant, queue management).
 Query Sharding            │ NOT NEEDED  │ LATER      │ Needed when backends can't handle cardinality.
 Pipeline Execution Engine │ NOT NEEDED  │ NEVER      │ UNIQL delegates execution, not a DB engine.
 Vectorized Processing     │ NOT NEEDED  │ NEVER      │ UNIQL never processes raw data.
 Borrow Checking/Ownership │ NOT NEEDED  │ NEVER      │ Language-specific concern.
```

---

## PART 4: WHAT WILL BE PAINFUL TO RETROFIT

These are ranked by "retrofit pain" — how much existing code must change if added later:

### 1. AST NORMALIZATION / CANONICALIZATION (Pain: EXTREME)

**What it is:** A pass between semantic validation and transpilation that rewrites the AST into a canonical form. Examples:
- `WHERE service != "nginx" AND service != "envoy"` → `WHERE service NOT IN ["nginx", "envoy"]`
- `COMPUTE rate(value, 5m)` on metrics → ensures window aligns with WITHIN clause
- Constant folding: `WHERE x > 3 + 2` → `WHERE x > 5`
- Normalize comparison direction: `WHERE 5 < x` → `WHERE x > 5`

**Why it's painful to skip:** Every transpiler currently handles its own ad-hoc normalization. When you add a 4th or 5th backend, you'll find each transpiler has reimplemented slightly different normalization logic. Common bugs appear in one transpiler but not others because they each handle edge cases differently.

**Evidence:** sqlglot's universal AST + canonicalization is why it can support 31 dialects with shared optimization. Calcite's RelNode conversion is why 20+ adapters share optimization rules. Every system that reached scale added canonicalization retroactively and paid heavily.

**Recommendation:** Add a `normalize` pass in `uniql-core` between `validate` and `transpile`. Start with:
- Expression flattening (nested AND/OR → flat lists)
- Comparison direction normalization
- IN-list coalescing
- Duration normalization (all durations to canonical form)

### 2. NAME RESOLUTION / BINDING (Pain: HIGH)

**What it is:** A pass that resolves all identifiers to their concrete meanings before the transpiler sees them. Currently, the transpiler has to figure out what `service` means (is it a label? a field? a metric name?).

**Why it's painful to skip:** Right now, each transpiler independently interprets identifiers. The PromQL transpiler treats `__name__` as a metric name; the LogsQL transpiler treats `_msg` as a message field. If you add function overloading (`count` means different things in metrics vs logs), the transpiler has to do name resolution AND code generation simultaneously. These concerns compound.

**Evidence:** TypeScript's binder (the largest component besides the type checker) exists precisely because name resolution is too complex to embed in code generation. DuckDB's Binder is a mandatory stage. Calcite's SqlValidatorImpl does resolution before conversion to relational algebra.

**Recommendation:** Add a `bind` pass after `expand` that:
- Resolves signal-qualified identifiers (`metrics.cpu` → binds to metric signal, field `cpu`)
- Resolves function names to their canonical form per signal type
- Resolves backend hints to concrete backend configurations
- Attaches type information to every expression node (so the transpiler receives a fully-resolved tree)

### 3. MIDDLEWARE / CROSS-CUTTING CONCERNS LAYER (Pain: HIGH)

**What it is:** A composable pipeline of middleware that wraps query execution with orthogonal concerns: rate limiting, caching, retries, timeout enforcement, telemetry.

**Why it's painful to skip:** These concerns are currently either absent or will be ad-hoc when added. Every observability backend (Mimir, Cortex, Loki, VictoriaMetrics) learned that query-level middleware must be a first-class architectural concept. Without it, caching logic ends up scattered across executor code, retry logic gets duplicated per backend, and rate limiting becomes a bolt-on.

**Evidence:** Mimir's middleware chain (limits → splitting → caching → sharding → retry) is modular and composable — each middleware is independent. Cortex extracted the query scheduler from the query frontend specifically because these concerns were entangled. VictoriaMetrics has concurrency control, parse caching, and rollup caching as separate systems.

**Recommendation:** Add a middleware trait in `uniql-engine`:
```
trait QueryMiddleware {
    async fn process(&self, ctx: &mut QueryContext, next: Next) -> Result<QueryResult>;
}
```
Start with: RateLimiter, TimeoutEnforcer, RequestLogger. Add ResultCache and RetryMiddleware next.

### 4. RESULT NORMALIZATION (Pain: MODERATE)

**What it is:** A dedicated stage that converts heterogeneous backend responses into a uniform internal format BEFORE correlation.

**Why it's painful to skip:** The correlator currently reaches into raw backend JSON and extracts fields with backend-specific logic (`data.result[].metric` for Prometheus, `result[]._msg` for VictoriaLogs). This means every new backend requires modifying the correlator. If you add Loki, Elasticsearch, or ClickHouse backends, the correlator becomes a sprawling switch statement.

**Evidence:** Thanos normalizes all StoreAPI responses into a uniform SeriesSet before the PromQL engine evaluates them. Mimir's querier merges ingester and store-gateway results into a single SeriesSet interface. The uniform format is the contract between data fetching and evaluation.

**Recommendation:** Define a `UnifiedResult` enum:
```
enum UnifiedResult {
    TimeSeries { series: Vec<Series> },         // metrics
    LogEntries { entries: Vec<LogEntry> },       // logs
    Spans { spans: Vec<Span> },                 // traces
    Events { events: Vec<Event> },              // events
}
```
Each executor backend produces `UnifiedResult` directly. The correlator only works with `UnifiedResult`, never raw JSON.

### 5. EXPLAIN / INTROSPECTION (Pain: MODERATE)

**What it is:** The ability to show the query plan at every pipeline stage without executing it.

**Current state:** UNIQL already has a basic `/explain` endpoint that shows parse → decompose → transpile → execute steps. But it doesn't show the AST, doesn't show what normalization/optimization did, and doesn't have execution statistics.

**Why it matters now:** Every production query engine has EXPLAIN (Trino, DuckDB, ClickHouse, PostgreSQL). Without it, debugging why a transpiled query is wrong requires printf debugging. As the pipeline gets more stages, EXPLAIN becomes the primary debugging tool.

**Recommendation:** Enhance the existing explain to carry the AST representation at each stage boundary. Add an `explain_verbose` mode that returns the AST diff between stages.

---

## PART 5: RECOMMENDED FINAL LAYER ARCHITECTURE

### Target: 12 Layers (up from current 7)

```
UNIQL-CORE (compile-time pipeline, no I/O):
═══════════════════════════════════════════════════════════════

  ┌─────────────────────────────────────────────────────────┐
  │ Layer 1: LEXER                                          │
  │ Input:  query string                                    │
  │ Output: Vec<Token>                                      │
  │ Status: EXISTS — no changes needed                      │
  └──────────────────────────┬──────────────────────────────┘
                             │
  ┌──────────────────────────▼──────────────────────────────┐
  │ Layer 2: PARSER                                         │
  │ Input:  Vec<Token>                                      │
  │ Output: ast::Query (unresolved, unexpanded)             │
  │ Status: EXISTS — no changes needed                      │
  └──────────────────────────┬──────────────────────────────┘
                             │
  ┌──────────────────────────▼──────────────────────────────┐
  │ Layer 3: MACRO EXPANDER                                 │
  │ Input:  ast::Query with DefineClause entries            │
  │ Output: ast::Query with all DEFINE/USE resolved         │
  │ Status: EXISTS — no changes needed                      │
  └──────────────────────────┬──────────────────────────────┘
                             │
  ┌──────────────────────────▼──────────────────────────────┐
  │ Layer 4: BINDER / NAME RESOLVER  ★ NEW                  │
  │ Input:  expanded ast::Query                             │
  │ Output: BoundQuery (all idents resolved, typed)         │
  │                                                         │
  │ Responsibilities:                                       │
  │ - Resolve signal-qualified idents (metrics.cpu → metric │
  │   signal, field "cpu")                                  │
  │ - Resolve function names to canonical form per signal   │
  │ - Attach type info to every Expr node                   │
  │ - Validate that referenced fields/functions exist for   │
  │   the target signal type                                │
  │ - Build a scope map for downstream stages               │
  │                                                         │
  │ Why now: Prevents every transpiler from reimplementing  │
  │ name resolution logic differently. The transpiler       │
  │ should receive a fully-resolved tree.                   │
  └──────────────────────────┬──────────────────────────────┘
                             │
  ┌──────────────────────────▼──────────────────────────────┐
  │ Layer 5: SEMANTIC VALIDATOR                             │
  │ Input:  BoundQuery                                      │
  │ Output: ValidatedQuery (same structure, errors/warnings)│
  │ Status: EXISTS — enhance to work with bound/typed info  │
  │                                                         │
  │ Enhancements:                                           │
  │ - Type-aware validation (now has type info from binder) │
  │ - Function arity/type checking per signal type          │
  │ - Cross-clause consistency checks                       │
  │ - Warning for performance anti-patterns                 │
  └──────────────────────────┬──────────────────────────────┘
                             │
  ┌──────────────────────────▼──────────────────────────────┐
  │ Layer 6: NORMALIZER / CANONICALIZER  ★ NEW              │
  │ Input:  ValidatedQuery                                  │
  │ Output: NormalizedQuery (canonical form)                │
  │                                                         │
  │ Rewrite rules (start with these):                       │
  │ - Flatten nested AND/OR into flat lists                 │
  │ - Coalesce `x != a AND x != b` → `x NOT IN [a, b]`    │
  │ - Normalize comparison direction: `5 < x` → `x > 5`   │
  │ - Constant fold: `x > 3 + 2` → `x > 5`                │
  │ - Duration canonicalization: "300s" → "5m"              │
  │ - Default injection: add WITHIN last 5m if absent       │
  │ - Remove tautologies: `x = x` → true                   │
  │ - Remove contradictions: `x = 1 AND x = 2` → error     │
  │                                                         │
  │ Why now: Without this, every transpiler develops its    │
  │ own normalization quirks. 3 backends × ad-hoc handling  │
  │ = bugs that appear in one backend but not others.       │
  └──────────────────────────┬──────────────────────────────┘
                             │
  ┌──────────────────────────▼──────────────────────────────┐
  │ Layer 7: TRANSPILER                                     │
  │ Input:  NormalizedQuery (canonical, typed, resolved)     │
  │ Output: TranspileOutput { native_query, backend_type }  │
  │ Status: EXISTS — simplify because binder+normalizer     │
  │         handle work the transpilers currently do ad-hoc  │
  │                                                         │
  │ Each transpiler becomes simpler:                        │
  │ - No more name resolution (binder did it)               │
  │ - No more normalization (normalizer did it)             │
  │ - Pure structural mapping: UNIQL AST node → native      │
  │   syntax string                                         │
  └─────────────────────────────────────────────────────────┘


UNIQL-ENGINE (runtime pipeline, with I/O):
═══════════════════════════════════════════════════════════════

  ┌─────────────────────────────────────────────────────────┐
  │ Layer 8: PLANNER / DECOMPOSER                           │
  │ Input:  NormalizedQuery + EngineConfig                  │
  │ Output: QueryPlan { sub_queries, correlation_plan }     │
  │ Status: EXISTS — no structural changes needed           │
  │                                                         │
  │ Enhancement: Call transpile for each sub-query here     │
  │ (already does this). Attach estimated cost per sub-query│
  │ for explain/middleware decisions.                        │
  └──────────────────────────┬──────────────────────────────┘
                             │
  ┌──────────────────────────▼──────────────────────────────┐
  │ Layer 9: MIDDLEWARE CHAIN  ★ NEW                         │
  │ Input:  QueryPlan + QueryContext                        │
  │ Output: QueryPlan (possibly modified) or cached result  │
  │                                                         │
  │ Composable middleware stack:                            │
  │                                                         │
  │  ┌─ RateLimiter ──────────────────────────────────────┐ │
  │  │ Reject if concurrency/rate limit exceeded (429)    │ │
  │  └────────────────────────────────────────────────────┘ │
  │  ┌─ TimeoutEnforcer ──────────────────────────────────┐ │
  │  │ Set per-query deadline, propagate to backends      │ │
  │  └────────────────────────────────────────────────────┘ │
  │  ┌─ ResultCache ──────────────────────────────────────┐ │
  │  │ Check cache for identical queries within window    │ │
  │  │ On miss: proceed. On hit: return cached result.    │ │
  │  └────────────────────────────────────────────────────┘ │
  │  ┌─ RequestLogger / Telemetry ────────────────────────┐ │
  │  │ Log query, plan, timing for observability          │ │
  │  └────────────────────────────────────────────────────┘ │
  │  ┌─ RetryMiddleware (LATER) ──────────────────────────┐ │
  │  │ Auto-retry transient backend failures              │ │
  │  └────────────────────────────────────────────────────┘ │
  │  ┌─ QuerySplitter (LATER) ───────────────────────────┐ │
  │  │ Split long time ranges into parallel sub-queries   │ │
  │  └────────────────────────────────────────────────────┘ │
  │                                                         │
  │ Why now: Cross-cutting concerns WILL be added. Without  │
  │ a middleware architecture, they get bolted onto the      │
  │ executor ad-hoc. Every observability system learned this.│
  └──────────────────────────┬──────────────────────────────┘
                             │
  ┌──────────────────────────▼──────────────────────────────┐
  │ Layer 10: EXECUTOR                                      │
  │ Input:  QueryPlan (sub-queries with native queries)     │
  │ Output: Vec<(signal_type, BackendResult)>               │
  │ Status: EXISTS — enhance output type                    │
  │                                                         │
  │ Enhancement: Each backend executor returns              │
  │ UnifiedResult instead of raw JSON. This moves backend-  │
  │ specific response parsing INTO the executor (where it   │
  │ belongs) and OUT of the correlator.                     │
  └──────────────────────────┬──────────────────────────────┘
                             │
  ┌──────────────────────────▼──────────────────────────────┐
  │ Layer 11: RESULT NORMALIZER  ★ NEW                      │
  │ Input:  Vec<(signal_type, BackendResult)>               │
  │ Output: Vec<(signal_type, UnifiedResult)>               │
  │                                                         │
  │ Responsibilities:                                       │
  │ - Convert Prometheus JSON → UnifiedResult::TimeSeries   │
  │ - Convert VictoriaLogs JSON → UnifiedResult::LogEntries │
  │ - Convert Loki JSON → UnifiedResult::LogEntries         │
  │ - Deduplication of series/entries                       │
  │ - Timestamp normalization to common epoch format        │
  │ - Label/field name normalization                        │
  │                                                         │
  │ Why now: Without this, backend-specific JSON parsing    │
  │ lives in the correlator. Each new backend requires      │
  │ modifying correlator code. With this layer, adding a    │
  │ new backend means implementing one normalizer — the     │
  │ correlator never changes.                               │
  └──────────────────────────┬──────────────────────────────┘
                             │
  ┌──────────────────────────▼──────────────────────────────┐
  │ Layer 12: CORRELATOR / RESULT MERGER                    │
  │ Input:  Vec<(signal_type, UnifiedResult)>               │
  │ Output: CorrelatedResult or single UnifiedResult        │
  │ Status: EXISTS — simplify to work with UnifiedResult    │
  │                                                         │
  │ For single-signal: pass through                         │
  │ For multi-signal: join on fields + time window          │
  │                                                         │
  │ The correlator becomes MUCH simpler because it only     │
  │ works with UnifiedResult, never raw backend JSON.       │
  └──────────────────────────┬──────────────────────────────┘
                             │
  ┌──────────────────────────▼──────────────────────────────┐
  │ Layer 13: RESPONSE FORMATTER                            │
  │ Input:  CorrelatedResult or UnifiedResult               │
  │ Output: HTTP JSON response (shaped per SHOW clause)     │
  │ Status: EXISTS (in API handler) — extract into layer    │
  │                                                         │
  │ Formats output based on SHOW format:                    │
  │ - timeseries → Grafana-compatible JSON                  │
  │ - table → column-oriented JSON                          │
  │ - count → scalar                                        │
  └─────────────────────────────────────────────────────────┘
```

---

## PART 6: IMPLEMENTATION PRIORITY MATRIX

### Phase 1: ADD NOW (prevents painful retrofitting)

| Layer | Effort | Files affected if added later | Risk if skipped |
|-------|--------|-------------------------------|-----------------|
| **Normalizer/Canonicalizer** | 2-3 days | Every transpiler + tests | Every new backend magnifies divergence |
| **Binder/Name Resolver** | 3-4 days | AST types, semantic validator, every transpiler | Transpilers grow unbounded complexity |
| **Middleware trait + skeleton** | 1-2 days | Engine pipeline, API handlers | Cross-cutting concerns get bolted on ad-hoc |
| **Result Normalizer (UnifiedResult)** | 2-3 days | Correlator, executor backends, API handlers | Correlator becomes unmaintainable |

### Phase 2: ADD WHEN NEEDED (clean interfaces make these easy to add later)

| Layer | Trigger | Prerequisite |
|-------|---------|-------------|
| RetryMiddleware | Backend reliability issues in prod | Middleware trait from Phase 1 |
| ResultCache | Repeated identical queries in prod | Middleware trait from Phase 1 |
| QuerySplitter | Queries spanning >24h time ranges | Middleware trait from Phase 1 |
| Query cost estimation | Need to prioritize/reject expensive queries | Binder + type info from Phase 1 |
| Parse cache | High QPS with repeated query patterns | Normalizer from Phase 1 (cache canonical form) |
| Logical Optimization | Subqueries, multi-step queries, query composition | Normalizer from Phase 1 |

### Phase 3: LIKELY NEVER NEEDED

| Layer | Why not |
|-------|---------|
| Physical planning/optimization | UNIQL delegates execution to backends |
| Vectorized/pipeline execution | UNIQL doesn't process raw data |
| Distributed task scheduling | Backends handle their own distribution |
| Code generation / JIT | UNIQL emits strings, not bytecode |
| IR lowering to multiple levels | Single AST sufficient for transpilation |

---

## PART 7: SUMMARY OF THE KEY INSIGHT FROM EACH SYSTEM

| System | Key lesson for UNIQL |
|--------|---------------------|
| **Trino** | Separate rule-based optimization (always good) from cost-based (statistics-dependent). UNIQL's normalizer is the rule-based stage. |
| **Calcite** | Validation and relational conversion are SEPARATE stages. Don't mix "is this valid?" with "what does this mean?". |
| **DataFusion** | Logical optimization and physical optimization are different concerns. Even with 7 stages, each stage is simple. |
| **ClickHouse** | EXPLAIN PIPELINE is invaluable for debugging. Invest in introspection early. |
| **DuckDB** | The Binder stage (catalog resolution) is mandatory before planning. Name resolution cannot be deferred to code generation. |
| **Velox** | Execution can be fully separated from planning/optimization. Validates UNIQL's architecture of delegating execution to backends. |
| **Mimir** | Middleware chain is the correct architecture for cross-cutting concerns (caching, limits, splitting, sharding, retries). |
| **VictoriaMetrics** | Parse cache + concurrency control + result cache = three levels of caching. All are valuable independently. |
| **Cortex** | The query-frontend / scheduler / querier separation enables independent scaling. Three-tier architecture at scale. |
| **Thanos** | Distributed query decomposition with optimization passes before fan-out. This is exactly UNIQL's planner pattern. |
| **rustc** | Multiple IR levels allow each to be optimized for specific analysis. Macro expansion before validation is correct. |
| **TypeScript** | Binding (name resolution + scope building) is a dedicated stage, separate from parsing AND type checking. |
| **GraphQL** | Context object passed through resolver chain. UNIQL's pipeline stages should share a QueryContext with accumulated metadata. |
| **sqlglot** | Universal AST + N+M architecture. Dialect-specific transforms in Generator.preprocess(). Canonicalization enables shared optimization. |

---

## PART 8: THE PIPELINE FLOW — BEFORE AND AFTER

### CURRENT (7 layers):
```
string → [Lexer] → tokens → [Parser] → AST → [Expander] → AST → [Validator] → AST → [Transpiler] → native string
                                                                                                       ↓
                                                               JSON ← [Correlator] ← results ← [Executor]
```

### RECOMMENDED (12 layers):
```
string → [Lexer] → tokens → [Parser] → AST → [Expander] → AST → [Binder] → BoundAST → [Validator] → ValidAST
                                                                                                         ↓
                                                                              NormalizedAST ← [Normalizer]
                                                                                       ↓
          JSON ← [Formatter] ← CorrelatedResult ← [Correlator] ← UnifiedResult ← [ResultNorm] ← results
                                                                                                     ↑
                                                                 [Transpiler] → native → [Middleware] → [Executor]
                                                                      ↑
                                                                 [Planner/Decomposer]
```

The 5 new layers (Binder, Normalizer, Middleware, Result Normalizer, Formatter) each do ONE thing. They simplify the layers around them by removing responsibilities that don't belong there.
