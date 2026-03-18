# UNIQL Core

**Unified Observability Query Language** — Write once, query everything.

Parser + Transpiler library. Part of the UNIQL execution engine.

## Quick Start

```bash
cargo build --release
cargo test

# Run a query
cargo run -- query 'FROM metrics WHERE __name__ = "http_requests_total" AND env = "prod" WITHIN last 5m COMPUTE rate(value, 5m) GROUP BY service' --backend promql

# Validate
cargo run -- validate 'FROM metrics WHERE service = "nginx" SHOW timeseries'

# Explain
cargo run -- explain 'FROM metrics WHERE __name__ = "ifInErrors" COMPUTE rate(value, 5m) GROUP BY host' --backend promql

# Debug
cargo run -- query 'FROM metrics WHERE service = "api"' --tokens
cargo run -- query 'FROM metrics WHERE service = "api"' --ast
```

## Architecture

```
src/
├── main.rs              # CLI entrypoint (clap)
├── lib.rs               # Public API
├── lexer/mod.rs         # Tokenizer (handwritten)
├── ast/mod.rs           # AST node definitions
├── parser/mod.rs        # Recursive descent + Pratt parsing
└── transpiler/
    ├── mod.rs           # Transpiler trait + registry
    └── promql.rs        # UNIQL → PromQL/MetricsQL
```

## Supported Backends

| Backend | Status | Signal |
|---------|--------|--------|
| PromQL / MetricsQL | ✅ Working | metrics |
| LogQL | 🚧 Sprint 1 | logs |
| TraceQL | 📋 Planned | traces |

## Test

```bash
cargo test                    # All tests
cargo test lexer              # Lexer tests only
cargo test parser             # Parser tests only
cargo test transpiler         # Transpiler tests only
```

---

*UNIQL Core — Samet Yağcı*
