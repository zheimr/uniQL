/// Benchmark query corpus — 3 tiers, 3 backends
///
/// Tier 1: Simple (basic selectors, single filter)
/// Tier 2: Realistic (aggregation, time range, group by, multiple filters)
/// Tier 3: Stress (long filter chains, complex expressions, edge cases)

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BenchQuery {
    pub id: &'static str,
    pub tier: Tier,
    pub query: &'static str,
    pub expected_backend: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tier {
    Simple,
    Realistic,
    Stress,
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tier::Simple => write!(f, "simple"),
            Tier::Realistic => write!(f, "realistic"),
            Tier::Stress => write!(f, "stress"),
        }
    }
}

pub fn corpus() -> Vec<BenchQuery> {
    vec![
        // ═══════════════════════════════════════════════════════════════
        // TIER 1: SIMPLE — basic selectors, single filter
        // ═══════════════════════════════════════════════════════════════

        // PromQL targets
        BenchQuery {
            id: "s01_simple_metric",
            tier: Tier::Simple,
            query: r#"FROM metrics WHERE __name__ = "up""#,
            expected_backend: "promql",
            description: "Simplest possible metric query",
        },
        BenchQuery {
            id: "s02_metric_one_label",
            tier: Tier::Simple,
            query: r#"FROM metrics WHERE __name__ = "up" AND job = "node-exporter""#,
            expected_backend: "promql",
            description: "Metric with single label filter",
        },
        BenchQuery {
            id: "s03_metric_with_time",
            tier: Tier::Simple,
            query: r#"FROM metrics WHERE __name__ = "up" WITHIN last 5m"#,
            expected_backend: "promql",
            description: "Metric with time range",
        },
        BenchQuery {
            id: "s04_metric_regex",
            tier: Tier::Simple,
            query: r#"FROM metrics WHERE __name__ = "http_requests_total" AND service =~ "api-.*""#,
            expected_backend: "promql",
            description: "Metric with regex filter",
        },
        // LogsQL targets
        BenchQuery {
            id: "s05_simple_log",
            tier: Tier::Simple,
            query: r#"FROM logs WHERE service = "api""#,
            expected_backend: "logsql",
            description: "Simplest log query",
        },
        BenchQuery {
            id: "s06_log_contains",
            tier: Tier::Simple,
            query: r#"FROM logs WHERE message CONTAINS "error""#,
            expected_backend: "logsql",
            description: "Log with contains filter",
        },
        BenchQuery {
            id: "s07_log_with_time",
            tier: Tier::Simple,
            query: r#"FROM logs WHERE service = "api" WITHIN last 15m"#,
            expected_backend: "logsql",
            description: "Log with time range",
        },
        // LogQL targets
        BenchQuery {
            id: "s08_logql_basic",
            tier: Tier::Simple,
            query: r#"FROM logs WHERE service = "api" AND level = "error""#,
            expected_backend: "logql",
            description: "Basic log query (LogQL target)",
        },
        // ═══════════════════════════════════════════════════════════════
        // TIER 2: REALISTIC — aggregation, group by, multiple filters
        // ═══════════════════════════════════════════════════════════════

        // PromQL targets
        BenchQuery {
            id: "r01_rate_groupby",
            tier: Tier::Realistic,
            query: r#"FROM metrics WHERE __name__ = "http_requests_total" AND env = "prod" WITHIN last 5m COMPUTE rate(value, 5m) GROUP BY service"#,
            expected_backend: "promql",
            description: "Rate with group by — classic monitoring query",
        },
        BenchQuery {
            id: "r02_cpu_rate",
            tier: Tier::Realistic,
            query: r#"FROM metrics WHERE __name__ = "node_cpu_seconds_total" AND mode = "idle" WITHIN last 1h COMPUTE rate(value, 5m) GROUP BY instance"#,
            expected_backend: "promql",
            description: "CPU idle rate by instance",
        },
        BenchQuery {
            id: "r03_multi_label",
            tier: Tier::Realistic,
            query: r#"FROM metrics WHERE __name__ = "http_requests_total" AND job = "api" AND env = "prod" AND region = "us-east" WITHIN last 30m"#,
            expected_backend: "promql",
            description: "Multiple label filters with time",
        },
        BenchQuery {
            id: "r04_in_list",
            tier: Tier::Realistic,
            query: r#"FROM metrics WHERE __name__ = "http_requests_total" AND service IN ["nginx", "envoy", "haproxy"] WITHIN last 1h"#,
            expected_backend: "promql",
            description: "IN list filter",
        },
        BenchQuery {
            id: "r05_p99",
            tier: Tier::Realistic,
            query: r#"FROM metrics WHERE __name__ = "http_request_duration_seconds_bucket" WITHIN last 5m COMPUTE p99(value)"#,
            expected_backend: "promql",
            description: "P99 histogram quantile",
        },
        BenchQuery {
            id: "r06_avg_having",
            tier: Tier::Realistic,
            query: r#"FROM metrics WHERE __name__ = "http_requests_total" COMPUTE rate(value, 5m) GROUP BY service HAVING rate > 0.01"#,
            expected_backend: "promql",
            description: "Rate with HAVING threshold",
        },
        BenchQuery {
            id: "r07_sum_by",
            tier: Tier::Realistic,
            query: r#"FROM metrics WHERE __name__ = "http_requests_total" AND env = "prod" WITHIN last 1h COMPUTE sum(value) GROUP BY service"#,
            expected_backend: "promql",
            description: "Sum aggregation by service",
        },
        BenchQuery {
            id: "r08_snmp_device",
            tier: Tier::Realistic,
            query: r#"FROM metrics WHERE __name__ = "snmpv2_device_up" AND hostname = "CORE-SW-01" WITHIN last 30m"#,
            expected_backend: "promql",
            description: "SNMP device status — AETHERIS NOC scenario",
        },
        BenchQuery {
            id: "r09_vsphere_cpu",
            tier: Tier::Realistic,
            query: r#"FROM metrics WHERE __name__ = "vsphere_host_cpu_usage_average" AND clustername = "DELLR750_Cluster" WITHIN last 1h"#,
            expected_backend: "promql",
            description: "vSphere host CPU by cluster — AETHERIS SYS scenario",
        },
        // LogsQL targets
        BenchQuery {
            id: "r10_log_multi_filter",
            tier: Tier::Realistic,
            query: r#"FROM logs WHERE service = "api" AND level = "error" AND message CONTAINS "timeout" WITHIN last 1h"#,
            expected_backend: "logsql",
            description: "Multi-filter log query with contains",
        },
        BenchQuery {
            id: "r11_fortigate_logs",
            tier: Tier::Realistic,
            query: r#"FROM logs WHERE job = "fortigate" WITHIN last 15m"#,
            expected_backend: "logsql",
            description: "FortiGate syslog — AETHERIS SOC scenario",
        },
        BenchQuery {
            id: "r12_log_parse_json",
            tier: Tier::Realistic,
            query: r#"FROM logs WHERE service = "api" AND level = "error" WITHIN last 1h PARSE json"#,
            expected_backend: "logsql",
            description: "Log with JSON parsing",
        },
        // LogQL targets
        BenchQuery {
            id: "r13_logql_rate",
            tier: Tier::Realistic,
            query: r#"FROM logs WHERE service = "api" COMPUTE rate(count, 5m) GROUP BY level"#,
            expected_backend: "logql",
            description: "Log rate by level — LogQL target",
        },
        // ═══════════════════════════════════════════════════════════════
        // TIER 3: STRESS — complex expressions, long chains, edge cases
        // ═══════════════════════════════════════════════════════════════
        BenchQuery {
            id: "x01_many_labels",
            tier: Tier::Stress,
            query: r#"FROM metrics WHERE __name__ = "http_requests_total" AND job = "api" AND env = "prod" AND region = "us-east" AND dc = "dc1" AND rack = "r01" AND instance =~ "web-.*" WITHIN last 24h COMPUTE rate(value, 5m) GROUP BY service"#,
            expected_backend: "promql",
            description: "7 label filters + rate + group by",
        },
        BenchQuery {
            id: "x02_long_in_list",
            tier: Tier::Stress,
            query: r#"FROM metrics WHERE __name__ = "http_requests_total" AND service IN ["nginx", "envoy", "haproxy", "traefik", "caddy", "kong", "apisix", "ambassador"] WITHIN last 1h COMPUTE rate(value, 5m) GROUP BY service"#,
            expected_backend: "promql",
            description: "Long IN list with rate and group by",
        },
        BenchQuery {
            id: "x03_nested_contains",
            tier: Tier::Stress,
            query: r#"FROM logs WHERE service = "api" AND level = "error" AND message CONTAINS "connection refused" AND host STARTS WITH "prod-" WITHIN last 24h"#,
            expected_backend: "logsql",
            description: "Multiple string match operations",
        },
        BenchQuery {
            id: "x04_pipe_syntax",
            tier: Tier::Stress,
            query: r#"FROM metrics:victoria |> WHERE __name__ = "cpu_usage" AND env = "prod" |> WITHIN last 1h |> COMPUTE avg(value) GROUP BY host"#,
            expected_backend: "promql",
            description: "Pipe syntax with backend hint",
        },
        BenchQuery {
            id: "x05_show_timeseries",
            tier: Tier::Stress,
            query: r#"SHOW timeseries FROM victoria WHERE __name__ = "vsphere_host_cpu_usage_average" AND clustername = "DELLR750_Cluster" WITHIN last 24h"#,
            expected_backend: "promql",
            description: "SHOW format with complex filter and long range",
        },
        BenchQuery {
            id: "x06_vlogs_direct",
            tier: Tier::Stress,
            query: r#"SHOW table FROM vlogs WHERE job = "fortigate" WITHIN last 1h"#,
            expected_backend: "logsql",
            description: "Direct vlogs backend with SHOW table",
        },
    ]
}

/// Subset for quick benchmarks
#[allow(dead_code)]
pub fn corpus_by_tier(tier: Tier) -> Vec<BenchQuery> {
    corpus().into_iter().filter(|q| q.tier == tier).collect()
}

/// All metric queries (PromQL target)
#[allow(dead_code)]
pub fn corpus_metrics() -> Vec<BenchQuery> {
    corpus()
        .into_iter()
        .filter(|q| q.expected_backend == "promql")
        .collect()
}

/// All log queries
#[allow(dead_code)]
pub fn corpus_logs() -> Vec<BenchQuery> {
    corpus()
        .into_iter()
        .filter(|q| q.expected_backend == "logsql" || q.expected_backend == "logql")
        .collect()
}
