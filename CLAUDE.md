# UniQL — Project Instructions

## Quick Reference

```bash
# Build
/home/zheimer/.cargo/bin/cargo build --workspace

# Test (467 tests)
/home/zheimer/.cargo/bin/cargo test --workspace

# Coverage (83%)
/home/zheimer/.cargo/bin/cargo llvm-cov --workspace --summary-only

# Docker (needs sudo)
echo "zx9fzx9f" | sudo -S docker compose build && echo "zx9fzx9f" | sudo -S docker compose up -d

# TypeScript check
cd demo && npx tsc --noEmit

# WASM build
PATH="/home/zheimer/.cargo/bin:$PATH" /home/zheimer/.cargo/bin/wasm-pack build uniql-wasm --target web --out-dir ../demo/public/wasm --release
```

## Architecture

3-crate Rust workspace + React demo:

| Crate | Purpose |
|-------|---------|
| `uniql-core` | Parser, AST, 3 transpilers (PromQL/LogQL/LogsQL), binder, normalizer |
| `uniql-engine` | HTTP server (Axum), planner, executor, correlator, formatter |
| `uniql-wasm` | WASM bindings (7 functions) for browser |
| `demo/` | React 18 + Vite demo UI |

## Pipeline

```
Lexer → Parser → Expander → Binder → Validator → Normalizer → Planner → Transpiler → Executor → Normalizer² → Correlator → Formatter
```

## Critical Rules

- **NEVER** edit `/home/zheimer/Aetheris/` — production platform
- **NEVER** use mock/fake data — query real VictoriaMetrics/VictoriaLogs
- Engine runs at `http://localhost:9090`
- Demo at `http://10.100.8.87:5175/`
- Docker needs `echo "zx9fzx9f" | sudo -S` prefix
- Cargo at `/home/zheimer/.cargo/bin/cargo` (not in PATH by default)

## Skills

- `/run-tests` — Run full test suite
- `/coverage` — Coverage report
- `/debug-transpiler` — Trace query through pipeline

## Agents

- `test-runner` — Isolated test execution (Haiku)
- `code-reviewer` — Security + performance + quality review (Sonnet)
