export const EXAMPLES = [
  {
    title: 'Basic Metric Query',
    backend: 'promql',
    query: `FROM metrics
WHERE __name__ = "http_requests_total" AND env = "prod"
WITHIN last 5m
COMPUTE rate(value, 1m) GROUP BY service
SHOW timeseries`,
  },
  {
    title: 'Log Search with Parsing',
    backend: 'logql',
    query: `FROM logs
WHERE service = "api" AND level = "error"
WITHIN last 1h
PARSE json
SHOW timeline`,
  },
  {
    title: 'VictoriaLogs Search',
    backend: 'logsql',
    query: `FROM logs
WHERE service = "api" AND message CONTAINS "timeout"
WITHIN last 15m`,
  },
  {
    title: 'Log Rate by Level (LogQL)',
    backend: 'logql',
    query: `FROM logs
WHERE service = "api"
COMPUTE rate(count, 5m) GROUP BY level`,
  },
  {
    title: 'SNMP + Syslog (AETHERIS)',
    backend: 'logsql',
    query: `-- AETHERIS: SNMP metric errors + syslog
FROM logs
WHERE service = "syslog-collector"
  AND message CONTAINS "link down"
WITHIN last 5m`,
  },
  {
    title: 'Percentile Histogram',
    backend: 'promql',
    query: `FROM metrics
WHERE __name__ = "http_request_duration_seconds_bucket"
WITHIN last 5m
COMPUTE p99(value)`,
  },
  {
    title: 'Multiple Labels',
    backend: 'promql',
    query: `FROM metrics
WHERE __name__ = "http_requests_total"
  AND job = "api"
  AND env = "prod"
  AND region = "us-east"`,
  },
  {
    title: 'Regex Match',
    backend: 'promql',
    query: `FROM metrics
WHERE __name__ = "http_requests_total"
  AND service =~ "api-.*"`,
  },
  {
    title: 'IN List Filter',
    backend: 'promql',
    query: `FROM metrics
WHERE __name__ = "http_requests_total"
  AND service IN ["nginx", "envoy", "haproxy"]`,
  },
  {
    title: 'Pipe Syntax',
    backend: 'promql',
    query: `FROM metrics:victoria
|> WHERE __name__ = "cpu_usage" AND env = "prod"
|> WITHIN last 1h
|> COMPUTE avg(value) GROUP BY host
|> SHOW heatmap`,
  },
  {
    title: 'HAVING Threshold',
    backend: 'promql',
    query: `FROM metrics
WHERE __name__ = "http_requests_total"
COMPUTE rate(value, 5m) GROUP BY service
HAVING rate > 0.01`,
  },
  {
    title: 'Contains + Starts With',
    backend: 'promql',
    query: `FROM metrics
WHERE __name__ = "http_requests_total"
  AND path CONTAINS "api"
  AND host STARTS WITH "prod-"`,
  },
];
