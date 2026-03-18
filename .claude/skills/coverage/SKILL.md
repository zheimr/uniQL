---
name: coverage
description: Run coverage analysis and show per-file breakdown. Use to check test coverage status.
allowed-tools: Bash, Read
---

# UniQL Coverage Report

1. Run `cd /home/zheimer/uniQL && /home/zheimer/.cargo/bin/cargo llvm-cov --workspace --summary-only 2>&1`
2. Show per-file coverage table sorted by coverage ascending (worst first)
3. Highlight files below 70%
4. Report total line coverage percentage
5. Compare to target (85%) and suggest which files to improve
