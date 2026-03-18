---
paths:
  - "uniql-*/src/**/*.rs"
---

# Rust Rules

- Use `Result<T, E>` not `.unwrap()` in library code (test code ok)
- Add `#[cfg(test)]` module with tests in same file
- Use `thiserror` for error types
- Every public function needs a doc comment (`///`)
- Run `cargo test --workspace` after changes
- Cargo path: `/home/zheimer/.cargo/bin/cargo`
