/// BENCHMARK D: Concurrency / Load Test
///
/// Tests engine under concurrent load:
/// - 1, 10, 50, 100 concurrent clients
/// - Measures throughput, p50/p95/p99 latency, error rate
/// - Reports CPU/memory if available

mod corpus;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ENGINE_URL: &str = "http://localhost:9090";
const TEST_DURATION_SECS: u64 = 10;
const CONCURRENCY_LEVELS: &[usize] = &[1, 10, 50, 100];

struct LoadResult {
    concurrency: usize,
    total_requests: u64,
    success: u64,
    errors: u64,
    latencies_us: Vec<u64>,
    duration_ms: u64,
}

#[tokio::main]
async fn main() {
    let queries = corpus::corpus();
    let query_pool: Vec<String> = queries.iter()
        .map(|q| serde_json::json!({"query": q.query, "limit": 5}).to_string())
        .collect();

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║  UNIQL LOAD TEST                                               ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  Duration: {}s per level", TEST_DURATION_SECS);
    println!("  Levels:   {:?}", CONCURRENCY_LEVELS);
    println!("  Queries:  {} in pool", query_pool.len());
    println!();

    // Health check
    let client = reqwest::Client::new();
    let health: serde_json::Value = client
        .get(format!("{}/health", ENGINE_URL))
        .send().await.expect("Engine unreachable")
        .json().await.unwrap();
    println!("  Engine: {} v{}", health["status"], health["version"]);
    println!();

    println!("  {:>5}  {:>8}  {:>6}  {:>8}  {:>8}  {:>8}  {:>6}  {:>8}",
        "CONC", "TOTAL", "ERR", "RPS", "p50", "p95", "p99", "MAX");
    println!("  {:>5}  {:>8}  {:>6}  {:>8}  {:>8}  {:>8}  {:>6}  {:>8}",
        "", "reqs", "%", "req/s", "ms", "ms", "ms", "ms");
    println!("  {}", "-".repeat(70));

    let mut all_results = Vec::new();

    for &concurrency in CONCURRENCY_LEVELS {
        let result = run_load_test(concurrency, &query_pool).await;

        let rps = result.total_requests as f64 / (result.duration_ms as f64 / 1000.0);
        let err_pct = if result.total_requests > 0 {
            result.errors as f64 / result.total_requests as f64 * 100.0
        } else { 0.0 };

        let p50 = percentile_us(&result.latencies_us, 50);
        let p95 = percentile_us(&result.latencies_us, 95);
        let p99 = percentile_us(&result.latencies_us, 99);
        let max = result.latencies_us.iter().max().copied().unwrap_or(0);

        println!("  {:>5}  {:>8}  {:>5.1}%  {:>8.1}  {:>7.1}  {:>7.1}  {:>5.1}  {:>7.1}",
            concurrency, result.total_requests, err_pct, rps,
            p50 as f64 / 1000.0, p95 as f64 / 1000.0, p99 as f64 / 1000.0, max as f64 / 1000.0,
        );

        all_results.push(result);
    }

    println!();
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║  ANALYSIS                                                      ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    // Check linearity
    if all_results.len() >= 2 {
        let first = &all_results[0];
        let last = &all_results[all_results.len() - 1];
        let first_p50 = percentile_us(&first.latencies_us, 50) as f64 / 1000.0;
        let last_p50 = percentile_us(&last.latencies_us, 50) as f64 / 1000.0;
        let conc_ratio = last.concurrency as f64 / first.concurrency.max(1) as f64;
        let latency_ratio = last_p50 / first_p50.max(0.001);

        println!("  Concurrency {}x → {}x → latency {:.1}x",
            first.concurrency, last.concurrency, latency_ratio);

        if latency_ratio < conc_ratio * 0.5 {
            println!("  \x1b[32mSub-linear scaling: good parallelism\x1b[0m");
        } else if latency_ratio < conc_ratio {
            println!("  \x1b[33mNear-linear scaling: acceptable\x1b[0m");
        } else {
            println!("  \x1b[31mSuper-linear scaling: bottleneck detected\x1b[0m");
        }
    }

    // Error analysis
    let total_errors: u64 = all_results.iter().map(|r| r.errors).sum();
    if total_errors > 0 {
        println!("  Total errors across all levels: {}", total_errors);
    } else {
        println!("  \x1b[32mZero errors across all concurrency levels\x1b[0m");
    }
}

async fn run_load_test(concurrency: usize, query_pool: &[String]) -> LoadResult {
    let success = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let latencies = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let deadline = Instant::now() + Duration::from_secs(TEST_DURATION_SECS);

    let mut handles = Vec::new();
    for worker_id in 0..concurrency {
        let query_pool = query_pool.to_vec();
        let success = success.clone();
        let errors = errors.clone();
        let latencies = latencies.clone();

        handles.push(tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap();

            let mut idx = worker_id;
            while Instant::now() < deadline {
                let query_body = &query_pool[idx % query_pool.len()];
                idx += concurrency;

                let start = Instant::now();
                let result = client.post(format!("{}/v1/query", ENGINE_URL))
                    .header("Content-Type", "application/json")
                    .body(query_body.clone())
                    .send()
                    .await;

                let elapsed_us = start.elapsed().as_micros() as u64;

                match result {
                    Ok(resp) if resp.status().is_success() => {
                        // Consume body
                        let _ = resp.bytes().await;
                        success.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {
                        errors.fetch_add(1, Ordering::Relaxed);
                    }
                }

                latencies.lock().await.push(elapsed_us);
            }
        }));
    }

    let test_start = Instant::now();
    for h in handles {
        let _ = h.await;
    }
    let duration_ms = test_start.elapsed().as_millis() as u64;

    let latencies = latencies.lock().await.clone();

    LoadResult {
        concurrency,
        total_requests: success.load(Ordering::Relaxed) + errors.load(Ordering::Relaxed),
        success: success.load(Ordering::Relaxed),
        errors: errors.load(Ordering::Relaxed),
        latencies_us: latencies,
        duration_ms,
    }
}

fn percentile_us(data: &[u64], pct: u32) -> u64 {
    if data.is_empty() { return 0; }
    let mut sorted = data.to_vec();
    sorted.sort();
    let idx = ((pct as f64 / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
