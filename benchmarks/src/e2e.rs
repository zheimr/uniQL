/// BENCHMARK B: End-to-End Query Benchmark
///
/// Measures full request lifecycle: HTTP → parse → transpile → execute → normalize → respond
/// Both cold path (first query) and warm path (cached)
/// Reports: p50, p95, p99 for each tier

mod corpus;

use std::time::Instant;

const ENGINE_URL: &str = "http://localhost:9090";
const ITERATIONS: usize = 20;

#[derive(Debug)]
struct E2EResult {
    id: String,
    tier: String,
    cold_ms: u64,
    warm_latencies_ms: Vec<u64>,
    native_query: String,
    status: String,
}

#[tokio::main]
async fn main() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();

    let queries = corpus::corpus();

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║  UNIQL END-TO-END BENCHMARK                                   ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  Corpus: {} queries, {} iterations per query", queries.len(), ITERATIONS);
    println!("  Engine: {ENGINE_URL}");
    println!();

    // Health check
    let health: serde_json::Value = client
        .get(format!("{}/health", ENGINE_URL))
        .send().await.expect("Engine unreachable")
        .json().await.unwrap();
    println!("  Engine: {} v{}", health["status"], health["version"]);
    println!();

    let mut results: Vec<E2EResult> = Vec::new();

    for q in &queries {
        // Cold path: first query (cache miss)
        let cold_start = Instant::now();
        let cold_resp = client.post(format!("{}/v1/query", ENGINE_URL))
            .json(&serde_json::json!({"query": q.query, "limit": 10}))
            .send().await;
        let cold_ms = cold_start.elapsed().as_millis() as u64;

        let (status, native_query) = match cold_resp {
            Ok(resp) => {
                let data: serde_json::Value = resp.json().await.unwrap_or_default();
                let s = data["status"].as_str().unwrap_or("error").to_string();
                let nq = data["metadata"]["native_query"].as_str().unwrap_or("").to_string();
                (s, nq)
            }
            Err(e) => (format!("error: {}", e), String::new()),
        };

        // Warm path: subsequent queries (cache hit possible)
        let mut warm_latencies = Vec::new();
        for _ in 0..ITERATIONS {
            let start = Instant::now();
            let _ = client.post(format!("{}/v1/query", ENGINE_URL))
                .json(&serde_json::json!({"query": q.query, "limit": 10}))
                .send().await;
            warm_latencies.push(start.elapsed().as_millis() as u64);
        }

        let warm_p50 = percentile(&warm_latencies, 50);
        let warm_p95 = percentile(&warm_latencies, 95);
        let warm_p99 = percentile(&warm_latencies, 99);

        let status_icon = if status == "success" { "\x1b[32m✓\x1b[0m" } else { "\x1b[31m✗\x1b[0m" };

        println!("  {} {:25} cold={:>4}ms  warm p50={:>4}ms p95={:>4}ms p99={:>4}ms  [{}]",
            status_icon, q.id, cold_ms, warm_p50, warm_p95, warm_p99, q.tier);

        results.push(E2EResult {
            id: q.id.to_string(),
            tier: q.tier.to_string(),
            cold_ms,
            warm_latencies_ms: warm_latencies,
            native_query,
            status,
        });
    }

    // Summary
    println!();
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║  SUMMARY                                                       ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    let success: Vec<_> = results.iter().filter(|r| r.status == "success").collect();
    let total = results.len();

    println!("  Success rate: {}/{} ({:.1}%)", success.len(), total, success.len() as f64 / total as f64 * 100.0);
    println!();

    // Cold path stats
    let cold: Vec<u64> = success.iter().map(|r| r.cold_ms).collect();
    if !cold.is_empty() {
        println!("  Cold path (first query, cache miss):");
        println!("    p50={:>4}ms  p95={:>4}ms  p99={:>4}ms  min={:>4}ms  max={:>4}ms",
            percentile(&cold, 50), percentile(&cold, 95), percentile(&cold, 99),
            cold.iter().min().unwrap(), cold.iter().max().unwrap(),
        );
    }

    // Warm path stats
    let warm: Vec<u64> = success.iter().flat_map(|r| r.warm_latencies_ms.iter().copied()).collect();
    if !warm.is_empty() {
        println!("  Warm path (cached, steady state):");
        println!("    p50={:>4}ms  p95={:>4}ms  p99={:>4}ms  min={:>4}ms  max={:>4}ms",
            percentile(&warm, 50), percentile(&warm, 95), percentile(&warm, 99),
            warm.iter().min().unwrap(), warm.iter().max().unwrap(),
        );
    }

    // Per-tier summary
    println!();
    for tier in ["simple", "realistic", "stress"] {
        let tier_warm: Vec<u64> = success.iter()
            .filter(|r| r.tier == tier)
            .flat_map(|r| r.warm_latencies_ms.iter().copied())
            .collect();
        if tier_warm.is_empty() { continue; }
        println!("  {:10}  warm p50={:>4}ms  p95={:>4}ms  p99={:>4}ms",
            tier.to_uppercase(), percentile(&tier_warm, 50), percentile(&tier_warm, 95), percentile(&tier_warm, 99));
    }

    // Cache effectiveness
    println!();
    let cold_avg: f64 = cold.iter().sum::<u64>() as f64 / cold.len().max(1) as f64;
    let warm_avg: f64 = warm.iter().sum::<u64>() as f64 / warm.len().max(1) as f64;
    println!("  Cache effectiveness:");
    println!("    Cold avg: {:.1}ms", cold_avg);
    println!("    Warm avg: {:.1}ms", warm_avg);
    if cold_avg > 0.0 {
        println!("    Speedup:  {:.1}x", cold_avg / warm_avg.max(0.1));
    }
}

fn percentile(data: &[u64], pct: u32) -> u64 {
    if data.is_empty() { return 0; }
    let mut sorted = data.to_vec();
    sorted.sort();
    let idx = ((pct as f64 / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
