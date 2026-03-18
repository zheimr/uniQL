---
name: debug-transpiler
description: Debug transpiler issues by tracing query through the pipeline. Use when UniQL produces wrong native query output.
allowed-tools: Bash, Read, Grep, Edit
---

# Debug UniQL Transpiler

Given a UniQL query that produces incorrect output:

1. **Validate**: Run query through `/v1/validate` endpoint
2. **Explain**: Run through `/v1/explain` to see execution plan
3. **Trace AST**: Parse query and inspect AST structure
4. **Check transpile**: Run through each transpiler (PromQL, LogQL, LogsQL)
5. **Identify**: Which pipeline stage produces the wrong output
6. **Fix**: Edit the responsible file and add regression test
7. **Verify**: Run `cargo test --workspace` to confirm fix

Engine URL: http://localhost:9090
Cargo: /home/zheimer/.cargo/bin/cargo
Workspace: /home/zheimer/uniQL
