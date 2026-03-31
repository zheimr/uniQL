/// BENCHMARK C: Accuracy / Semantic Equivalence
///
/// For each query in corpus:
/// 1. Transpile UNIQL → native query
/// 2. Send UNIQL via engine → get result
/// 3. Send native query directly to backend → get result
/// 4. Compare: exact match, sorted equivalence, float tolerance
///
/// Output: pass rate, exact equivalence %, tolerated equivalence %
mod corpus;

use std::time::Instant;

const ENGINE_URL: &str = "http://localhost:9090";
const VM_URL: &str = "http://10.0.1.100:8428";
const VL_URL: &str = "http://10.0.1.101:9428";

#[derive(Debug)]
#[allow(dead_code)]
struct AccuracyResult {
    id: String,
    tier: String,
    backend: String,
    uniql_query: String,
    native_query: String,
    transpile_ok: bool,
    execute_ok: bool,
    direct_ok: bool,
    exact_match: bool,
    sorted_match: bool,
    float_tolerance_match: bool,
    uniql_count: usize,
    direct_count: usize,
    uniql_ms: u64,
    direct_ms: u64,
    error: Option<String>,
}

#[tokio::main]
async fn main() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();

    let queries = corpus::corpus();
    let mut results: Vec<AccuracyResult> = Vec::new();

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║  UNIQL ACCURACY BENCHMARK — Semantic Equivalence Test          ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
    println!(
        "  Corpus: {} queries ({} simple, {} realistic, {} stress)",
        queries.len(),
        queries
            .iter()
            .filter(|q| q.tier == corpus::Tier::Simple)
            .count(),
        queries
            .iter()
            .filter(|q| q.tier == corpus::Tier::Realistic)
            .count(),
        queries
            .iter()
            .filter(|q| q.tier == corpus::Tier::Stress)
            .count(),
    );
    println!("  Engine: {ENGINE_URL}");
    println!("  VM:     {VM_URL}");
    println!("  VL:     {VL_URL}");
    println!();

    for q in &queries {
        let result = test_query(&client, q).await;
        let status = if result.exact_match {
            "\x1b[32mEXACT\x1b[0m"
        } else if result.sorted_match {
            "\x1b[33mSORTED\x1b[0m"
        } else if result.float_tolerance_match {
            "\x1b[33mFLOAT_TOL\x1b[0m"
        } else if result.execute_ok && result.direct_ok {
            "\x1b[31mMISMATCH\x1b[0m"
        } else if !result.transpile_ok {
            "\x1b[31mTRANSPILE_ERR\x1b[0m"
        } else {
            "\x1b[31mEXEC_ERR\x1b[0m"
        };

        println!(
            "  [{:8}] {:25} {:>5}ms vs {:>5}ms  cnt: {:>4} vs {:>4}  {}",
            q.tier.to_string().to_uppercase(),
            q.id,
            result.uniql_ms,
            result.direct_ms,
            result.uniql_count,
            result.direct_count,
            status,
        );
        results.push(result);
    }

    // Summary
    println!();
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║  RESULTS                                                       ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    let total = results.len();
    let transpile_ok = results.iter().filter(|r| r.transpile_ok).count();
    let execute_ok = results.iter().filter(|r| r.execute_ok).count();
    let exact = results.iter().filter(|r| r.exact_match).count();
    let sorted = results.iter().filter(|r| r.sorted_match).count();
    let float_tol = results.iter().filter(|r| r.float_tolerance_match).count();
    let any_match = results
        .iter()
        .filter(|r| r.exact_match || r.sorted_match || r.float_tolerance_match)
        .count();

    println!(
        "  Transpile success:       {:>3}/{} ({:.1}%)",
        transpile_ok,
        total,
        transpile_ok as f64 / total as f64 * 100.0
    );
    println!(
        "  Execute success:         {:>3}/{} ({:.1}%)",
        execute_ok,
        total,
        execute_ok as f64 / total as f64 * 100.0
    );
    println!(
        "  Exact match:             {:>3}/{} ({:.1}%)",
        exact,
        total,
        exact as f64 / total as f64 * 100.0
    );
    println!(
        "  Sorted equivalence:      {:>3}/{} ({:.1}%)",
        sorted,
        total,
        sorted as f64 / total as f64 * 100.0
    );
    println!(
        "  Float tolerance match:   {:>3}/{} ({:.1}%)",
        float_tol,
        total,
        float_tol as f64 / total as f64 * 100.0
    );
    println!(
        "  Any match (semantic eq): {:>3}/{} ({:.1}%)",
        any_match,
        total,
        any_match as f64 / total as f64 * 100.0
    );
    println!();

    // By tier
    for tier in [
        corpus::Tier::Simple,
        corpus::Tier::Realistic,
        corpus::Tier::Stress,
    ] {
        let tier_results: Vec<_> = results
            .iter()
            .filter(|r| r.tier == tier.to_string())
            .collect();
        let t = tier_results.len();
        if t == 0 {
            continue;
        }
        let m = tier_results
            .iter()
            .filter(|r| r.exact_match || r.sorted_match || r.float_tolerance_match)
            .count();
        println!(
            "  {:10} {}/{} semantic match ({:.1}%)",
            tier.to_string().to_uppercase(),
            m,
            t,
            m as f64 / t as f64 * 100.0
        );
    }

    // Latency summary
    println!();
    let uniql_latencies: Vec<u64> = results
        .iter()
        .filter(|r| r.execute_ok)
        .map(|r| r.uniql_ms)
        .collect();
    let direct_latencies: Vec<u64> = results
        .iter()
        .filter(|r| r.direct_ok)
        .map(|r| r.direct_ms)
        .collect();

    if !uniql_latencies.is_empty() {
        println!(
            "  UNIQL latency:  p50={}ms  p95={}ms  p99={}ms",
            percentile(&uniql_latencies, 50),
            percentile(&uniql_latencies, 95),
            percentile(&uniql_latencies, 99),
        );
    }
    if !direct_latencies.is_empty() {
        println!(
            "  Direct latency: p50={}ms  p95={}ms  p99={}ms",
            percentile(&direct_latencies, 50),
            percentile(&direct_latencies, 95),
            percentile(&direct_latencies, 99),
        );
    }

    if !uniql_latencies.is_empty() && !direct_latencies.is_empty() {
        let uniql_avg: f64 =
            uniql_latencies.iter().sum::<u64>() as f64 / uniql_latencies.len() as f64;
        let direct_avg: f64 =
            direct_latencies.iter().sum::<u64>() as f64 / direct_latencies.len() as f64;
        let overhead = uniql_avg - direct_avg;
        println!(
            "  UNIQL overhead: {:.1}ms avg ({:.1}%)",
            overhead,
            if direct_avg > 0.0 {
                overhead / direct_avg * 100.0
            } else {
                0.0
            }
        );
    }

    // Errors
    let errors: Vec<_> = results.iter().filter(|r| r.error.is_some()).collect();
    if !errors.is_empty() {
        println!();
        println!("  Errors ({}):", errors.len());
        for e in errors {
            println!("    {} — {}", e.id, e.error.as_ref().unwrap());
        }
    }
}

async fn test_query(client: &reqwest::Client, q: &corpus::BenchQuery) -> AccuracyResult {
    let mut result = AccuracyResult {
        id: q.id.to_string(),
        tier: q.tier.to_string(),
        backend: q.expected_backend.to_string(),
        uniql_query: q.query.to_string(),
        native_query: String::new(),
        transpile_ok: false,
        execute_ok: false,
        direct_ok: false,
        exact_match: false,
        sorted_match: false,
        float_tolerance_match: false,
        uniql_count: 0,
        direct_count: 0,
        uniql_ms: 0,
        direct_ms: 0,
        error: None,
    };

    // Step 1: Transpile
    let native = match q.expected_backend {
        "promql" => uniql_core::to_promql(q.query),
        "logsql" => uniql_core::to_logsql(q.query),
        "logql" => uniql_core::to_logql(q.query),
        _ => uniql_core::to_promql(q.query),
    };
    match native {
        Ok(n) => {
            result.native_query = n;
            result.transpile_ok = true;
        }
        Err(e) => {
            result.error = Some(format!("Transpile: {}", e));
            return result;
        }
    }

    // Step 2: Execute via UNIQL engine
    let uniql_start = Instant::now();
    let uniql_resp = client
        .post(format!("{}/v1/query", ENGINE_URL))
        .json(&serde_json::json!({"query": q.query, "limit": 50}))
        .send()
        .await;
    result.uniql_ms = uniql_start.elapsed().as_millis() as u64;

    let uniql_data = match uniql_resp {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(data) => {
                if data["status"] == "success" {
                    result.execute_ok = true;
                    data
                } else {
                    result.error = Some(format!("Engine: {}", data["error"]));
                    return result;
                }
            }
            Err(e) => {
                result.error = Some(format!("Engine parse: {}", e));
                return result;
            }
        },
        Err(e) => {
            result.error = Some(format!("Engine unreachable: {}", e));
            return result;
        }
    };

    // Step 3: Execute native query directly
    let direct_start = Instant::now();
    let direct_data = match q.expected_backend {
        "promql" => {
            let resp = client
                .get(format!("{}/api/v1/query", VM_URL))
                .query(&[("query", result.native_query.as_str())])
                .send()
                .await;
            match resp {
                Ok(r) => r.json::<serde_json::Value>().await.ok(),
                Err(_) => None,
            }
        }
        "logsql" => {
            let resp = client
                .get(format!("{}/select/logsql/query", VL_URL))
                .query(&[("query", result.native_query.as_str()), ("limit", "50")])
                .send()
                .await;
            match resp {
                Ok(r) => {
                    let text = r.text().await.unwrap_or_default();
                    let lines: Vec<serde_json::Value> = text
                        .lines()
                        .filter(|l| !l.is_empty())
                        .filter_map(|l| serde_json::from_str(l).ok())
                        .collect();
                    Some(serde_json::json!({"result": lines, "total": lines.len()}))
                }
                Err(_) => None,
            }
        }
        _ => None,
    };
    result.direct_ms = direct_start.elapsed().as_millis() as u64;

    let direct_data = match direct_data {
        Some(d) => {
            result.direct_ok = true;
            d
        }
        None => {
            result.error = Some("Direct query failed".to_string());
            return result;
        }
    };

    // Step 4: Compare results
    match q.expected_backend {
        "promql" => {
            let uniql_results = extract_prom_values(&uniql_data["data"]);
            let direct_results = extract_prom_values(&direct_data);
            result.uniql_count = uniql_results.len();
            result.direct_count = direct_results.len();

            result.exact_match = uniql_results == direct_results;
            if !result.exact_match {
                let mut u_sorted = uniql_results.clone();
                let mut d_sorted = direct_results.clone();
                u_sorted.sort();
                d_sorted.sort();
                result.sorted_match = u_sorted == d_sorted;
            }
            if !result.exact_match && !result.sorted_match {
                result.float_tolerance_match =
                    float_compare(&uniql_results, &direct_results, 0.001);
            }
        }
        "logsql" => {
            let uniql_logs = extract_log_messages(&uniql_data["data"]);
            let direct_logs = extract_log_messages(&direct_data);
            result.uniql_count = uniql_logs.len();
            result.direct_count = direct_logs.len();

            result.exact_match = uniql_logs == direct_logs;
            if !result.exact_match {
                let u_set: std::collections::HashSet<_> = uniql_logs.iter().collect();
                let d_set: std::collections::HashSet<_> = direct_logs.iter().collect();
                let overlap = u_set.intersection(&d_set).count();
                let min_len = uniql_logs.len().min(direct_logs.len());
                result.sorted_match = min_len > 0 && overlap >= min_len;
            }
        }
        _ => {}
    }

    result
}

fn extract_prom_values(data: &serde_json::Value) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(results) = data
        .get("data")
        .and_then(|d| d.get("result"))
        .and_then(|r| r.as_array())
    {
        for item in results {
            if let Some(val) = item.get("value").and_then(|v| v.as_array()) {
                if let Some(v) = val.get(1).and_then(|v| v.as_str()) {
                    values.push(v.to_string());
                }
            }
        }
    }
    values
}

fn extract_log_messages(data: &serde_json::Value) -> Vec<String> {
    let mut messages = Vec::new();
    if let Some(results) = data.get("result").and_then(|r| r.as_array()) {
        for item in results {
            if let Some(msg) = item.get("_msg").and_then(|m| m.as_str()) {
                messages.push(msg.to_string());
            }
        }
    }
    messages
}

fn float_compare(a: &[String], b: &[String], tolerance: f64) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(av, bv)| match (av.parse::<f64>(), bv.parse::<f64>()) {
            (Ok(af), Ok(bf)) => (af - bf).abs() <= tolerance * af.abs().max(bf.abs()).max(1.0),
            _ => av == bv,
        })
}

fn percentile(data: &[u64], pct: u32) -> u64 {
    if data.is_empty() {
        return 0;
    }
    let mut sorted = data.to_vec();
    sorted.sort();
    let idx = ((pct as f64 / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
