//! UNIQL Transpiler Layer
//!
//! Trait-based transpiler interface. Each backend implements the Transpiler trait.
//! Available backends: PromQL (Prometheus/VictoriaMetrics), LogsQL (VictoriaLogs), LogQL (Loki).

pub mod promql;
pub mod logsql;
pub mod logql;

use crate::ast::{Query, SignalType};

// ─── Transpiler Trait ─────────────────────────────────────────────────────────

/// Backend type identifier
#[derive(Debug, Clone, PartialEq)]
pub enum BackendType {
    Prometheus,
    Loki,
    VictoriaLogs,
    Tempo,
    Elasticsearch,
    ClickHouse,
    Custom(String),
}

/// Output of a transpilation
#[derive(Debug, Clone)]
pub struct TranspileOutput {
    pub native_query: String,
    pub target_signal: SignalType,
    pub backend_type: BackendType,
}

/// Trait that all backend transpilers implement
pub trait Transpiler: Send + Sync {
    /// Backend name (e.g., "promql", "logsql", "logql")
    fn name(&self) -> &str;

    /// Which signal types this transpiler supports
    fn supported_signals(&self) -> &[SignalType];

    /// Transpile a UNIQL AST to the native query format (legacy path)
    fn transpile(&self, query: &Query) -> Result<TranspileOutput, TranspileError>;

    /// Transpile from a NormalizedQuery (new path, uses pre-computed binder/normalizer data).
    /// Default delegates to legacy `transpile()` for backward compatibility.
    fn transpile_normalized(
        &self,
        normalized: &crate::normalize::NormalizedQuery,
    ) -> Result<TranspileOutput, TranspileError> {
        self.transpile(&normalized.bound.query)
    }

    /// Whether this backend supports CORRELATE (engine-level, not transpiler)
    fn supports_correlation(&self) -> bool {
        false
    }
}

/// Unified transpiler error type
#[derive(Debug, thiserror::Error)]
pub enum TranspileError {
    #[error("Backend '{backend}' does not support signal type '{signal:?}'")]
    UnsupportedSignalType {
        backend: String,
        signal: SignalType,
    },

    #[error("CORRELATE is not supported by single-backend transpilers. Use the execution engine.")]
    CorrelateNotSupported,

    #[error("Unknown function '{0}' for this backend")]
    UnknownFunction(String),

    #[error("Cannot determine metric name. Use WHERE __name__ = \"metric_name\"")]
    NoMetricName,

    #[error("Unsupported expression: {0}")]
    UnsupportedExpression(String),
}

// ─── Registry ─────────────────────────────────────────────────────────────────

/// Get a transpiler by backend name
pub fn get_transpiler(name: &str) -> Option<Box<dyn Transpiler>> {
    match name.to_lowercase().as_str() {
        "promql" | "metricsql" | "prometheus" | "victoria" => {
            Some(Box::new(promql::PromQLTranspiler))
        }
        "logsql" | "victorialogs" | "vlogs" => {
            Some(Box::new(logsql::LogsQLTranspiler))
        }
        "logql" | "loki" => {
            Some(Box::new(logql::LogQLTranspiler))
        }
        _ => None,
    }
}
