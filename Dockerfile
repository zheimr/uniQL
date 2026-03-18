FROM rust:1.94-bookworm AS builder
WORKDIR /app
COPY Cargo.toml ./
COPY uniql-core ./uniql-core
COPY uniql-engine ./uniql-engine
COPY uniql-wasm ./uniql-wasm
RUN cargo build --release -p uniql-engine

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/uniql-engine /usr/local/bin/uniql-engine
EXPOSE 9090
CMD ["uniql-engine"]
