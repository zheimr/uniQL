---
name: run-tests
description: Run full UniQL test suite and report results. Use after code changes or before commits.
allowed-tools: Bash, Read
---

# Run UniQL Tests

Run the full test suite and analyze results:

1. Run `cd /home/zheimer/uniQL && /home/zheimer/.cargo/bin/cargo test --workspace 2>&1`
2. Report: total passed, total failed, per-crate breakdown
3. If failures: show failed test names and error messages
4. If all pass: confirm count and suggest commit
