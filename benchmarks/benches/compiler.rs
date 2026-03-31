/// BENCHMARK A: Compiler Pipeline Micro-Benchmark
///
/// Measures: parse, prepare (parse+expand+validate), bind, normalize, transpile
/// For each tier (simple/realistic/stress) × each pipeline stage
/// Reports: p50, p95, p99 via Criterion
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use uniql_benchmarks::corpus::{self, Tier};

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    for q in corpus::corpus() {
        group.bench_with_input(
            BenchmarkId::new(q.tier.to_string(), q.id),
            &q.query,
            |b, query| {
                b.iter(|| {
                    let _ = black_box(uniql_core::parse(query));
                });
            },
        );
    }
    group.finish();
}

fn bench_prepare(c: &mut Criterion) {
    let mut group = c.benchmark_group("prepare");
    for q in corpus::corpus() {
        group.bench_with_input(
            BenchmarkId::new(q.tier.to_string(), q.id),
            &q.query,
            |b, query| {
                b.iter(|| {
                    let _ = black_box(uniql_core::prepare(query));
                });
            },
        );
    }
    group.finish();
}

fn bench_transpile_promql(c: &mut Criterion) {
    let mut group = c.benchmark_group("transpile_promql");
    for q in corpus::corpus_metrics() {
        group.bench_with_input(
            BenchmarkId::new(q.tier.to_string(), q.id),
            &q.query,
            |b, query| {
                b.iter(|| {
                    let _ = black_box(uniql_core::to_promql(query));
                });
            },
        );
    }
    group.finish();
}

fn bench_transpile_logsql(c: &mut Criterion) {
    let mut group = c.benchmark_group("transpile_logsql");
    for q in corpus::corpus_logs() {
        group.bench_with_input(
            BenchmarkId::new(q.tier.to_string(), q.id),
            &q.query,
            |b, query| {
                b.iter(|| {
                    let _ = black_box(uniql_core::to_logsql(query));
                });
            },
        );
    }
    group.finish();
}

fn bench_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_pipeline");
    for q in corpus::corpus() {
        let backend = q.expected_backend;
        group.bench_with_input(
            BenchmarkId::new(q.tier.to_string(), q.id),
            &q.query,
            |b, query| {
                b.iter(|| {
                    let result = match backend {
                        "promql" => uniql_core::to_promql(query),
                        "logsql" => uniql_core::to_logsql(query),
                        "logql" => uniql_core::to_logql(query),
                        _ => uniql_core::to_promql(query),
                    };
                    let _ = black_box(result);
                });
            },
        );
    }
    group.finish();
}

/// Tier-aggregated benchmarks for summary statistics
fn bench_by_tier(c: &mut Criterion) {
    let mut group = c.benchmark_group("tier_summary");

    for tier in [Tier::Simple, Tier::Realistic, Tier::Stress] {
        let queries = corpus::corpus_by_tier(tier);
        group.bench_with_input(
            BenchmarkId::new("parse", tier.to_string()),
            &queries,
            |b, queries| {
                b.iter(|| {
                    for q in queries {
                        let _ = black_box(uniql_core::parse(q.query));
                    }
                });
            },
        );

        let queries = corpus::corpus_by_tier(tier);
        group.bench_with_input(
            BenchmarkId::new("transpile", tier.to_string()),
            &queries,
            |b, queries| {
                b.iter(|| {
                    for q in queries {
                        let result = match q.expected_backend {
                            "promql" => uniql_core::to_promql(q.query),
                            "logsql" => uniql_core::to_logsql(q.query),
                            "logql" => uniql_core::to_logql(q.query),
                            _ => uniql_core::to_promql(q.query),
                        };
                        let _ = black_box(result);
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_parse,
    bench_prepare,
    bench_transpile_promql,
    bench_transpile_logsql,
    bench_full_pipeline,
    bench_by_tier,
);
criterion_main!(benches);
