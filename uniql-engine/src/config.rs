use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct EngineConfig {
    pub listen: String,
    pub backends: Vec<BackendConfig>,
    /// API keys for authentication. Empty = auth disabled.
    #[serde(default)]
    pub api_keys: Vec<String>,
    /// Allowed CORS origins. Empty = permissive (all origins).
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackendConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub backend_type: String,
    pub url: String,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            listen: "0.0.0.0:9090".to_string(),
            backends: vec![
                BackendConfig {
                    name: "victoria".to_string(),
                    backend_type: "prometheus".to_string(),
                    url: "http://victoria-metrics:8428".to_string(),
                },
                BackendConfig {
                    name: "vlogs".to_string(),
                    backend_type: "victorialogs".to_string(),
                    url: "http://victoria-logs:9428".to_string(),
                },
            ],
            api_keys: Vec::new(),
            cors_origins: Vec::new(),
        }
    }
}

impl EngineConfig {
    /// Load config with priority: TOML file > env JSON > defaults
    pub fn load() -> Self {
        // 1. Try TOML config file
        if let Ok(path) = std::env::var("UNIQL_CONFIG") {
            match std::fs::read_to_string(&path) {
                Ok(contents) => {
                    match toml_from_str(&contents) {
                        Ok(config) => {
                            tracing::info!("Config loaded from {}", path);
                            return config;
                        }
                        Err(e) => {
                            tracing::error!("Failed to parse config file {}: {}", path, e);
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to read config file {}: {}", path, e);
                    std::process::exit(1);
                }
            }
        }

        // 2. Try env variables
        let listen = std::env::var("UNIQL_LISTEN")
            .unwrap_or_else(|_| "0.0.0.0:9090".to_string());

        let backends = match std::env::var("UNIQL_BACKENDS") {
            Ok(json) => {
                match serde_json::from_str(&json) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::error!("UNIQL_BACKENDS is invalid JSON: {}. Fix it or remove it to use defaults.", e);
                        std::process::exit(1);
                    }
                }
            }
            Err(_) => {
                tracing::info!("No UNIQL_CONFIG or UNIQL_BACKENDS set, using defaults");
                Self::default().backends
            }
        };

        // API keys from env (comma-separated)
        let api_keys = std::env::var("UNIQL_API_KEYS")
            .map(|s| s.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect())
            .unwrap_or_default();

        // CORS origins from env (comma-separated)
        let cors_origins = std::env::var("UNIQL_CORS_ORIGINS")
            .map(|s| s.split(',').map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect())
            .unwrap_or_default();

        EngineConfig { listen, backends, api_keys, cors_origins }
    }

    pub fn find_backend(&self, signal: &str, hint: Option<&str>) -> Option<&BackendConfig> {
        if let Some(hint) = hint {
            return self.backends.iter().find(|b| b.name == hint);
        }

        match signal {
            "metrics" => self.backends.iter().find(|b| {
                b.backend_type == "prometheus" || b.backend_type == "victoriametrics"
            }),
            "logs" | "vlogs" | "victorialogs" => self.backends.iter().find(|b| {
                b.backend_type == "victorialogs" || b.backend_type == "loki"
            }),
            _ => self.backends.first(),
        }
    }
}

/// Parse TOML config string into EngineConfig using the toml crate.
fn toml_from_str(s: &str) -> Result<EngineConfig, String> {
    toml::from_str(s).map_err(|e| format!("TOML parse error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(backends: Vec<BackendConfig>) -> EngineConfig {
        EngineConfig {
            listen: "0.0.0.0:9090".to_string(),
            backends,
            api_keys: Vec::new(),
            cors_origins: Vec::new(),
        }
    }

    // ─── Default ────────────────────────────────────────────────────────

    #[test]
    fn default_config_has_two_backends() {
        let cfg = EngineConfig::default();
        assert_eq!(cfg.backends.len(), 2);
        assert_eq!(cfg.listen, "0.0.0.0:9090");
        assert!(cfg.api_keys.is_empty());
        assert!(cfg.cors_origins.is_empty());
    }

    #[test]
    fn default_config_prometheus_backend() {
        let cfg = EngineConfig::default();
        let prom = cfg.backends.iter().find(|b| b.backend_type == "prometheus").unwrap();
        assert_eq!(prom.name, "victoria");
        assert!(prom.url.contains("8428"));
    }

    #[test]
    fn default_config_victorialogs_backend() {
        let cfg = EngineConfig::default();
        let vlogs = cfg.backends.iter().find(|b| b.backend_type == "victorialogs").unwrap();
        assert_eq!(vlogs.name, "vlogs");
        assert!(vlogs.url.contains("9428"));
    }

    // ─── find_backend ───────────────────────────────────────────────────

    #[test]
    fn find_backend_metrics_returns_prometheus() {
        let cfg = EngineConfig::default();
        let b = cfg.find_backend("metrics", None).unwrap();
        assert!(b.backend_type == "prometheus" || b.backend_type == "victoriametrics");
    }

    #[test]
    fn find_backend_logs_returns_victorialogs() {
        let cfg = EngineConfig::default();
        let b = cfg.find_backend("logs", None).unwrap();
        assert!(b.backend_type == "victorialogs" || b.backend_type == "loki");
    }

    #[test]
    fn find_backend_vlogs_returns_victorialogs() {
        let cfg = EngineConfig::default();
        let b = cfg.find_backend("vlogs", None).unwrap();
        assert_eq!(b.backend_type, "victorialogs");
    }

    #[test]
    fn find_backend_victorialogs_signal() {
        let cfg = EngineConfig::default();
        let b = cfg.find_backend("victorialogs", None).unwrap();
        assert_eq!(b.backend_type, "victorialogs");
    }

    #[test]
    fn find_backend_unknown_signal_returns_first() {
        let cfg = EngineConfig::default();
        let b = cfg.find_backend("unknown_signal", None).unwrap();
        assert_eq!(b.name, cfg.backends[0].name);
    }

    #[test]
    fn find_backend_with_hint_overrides_signal() {
        let cfg = EngineConfig::default();
        let b = cfg.find_backend("metrics", Some("vlogs")).unwrap();
        assert_eq!(b.name, "vlogs");
    }

    #[test]
    fn find_backend_with_nonexistent_hint_returns_none() {
        let cfg = EngineConfig::default();
        assert!(cfg.find_backend("metrics", Some("nonexistent")).is_none());
    }

    #[test]
    fn find_backend_empty_backends_returns_none() {
        let cfg = make_config(vec![]);
        assert!(cfg.find_backend("metrics", None).is_none());
    }

    #[test]
    fn find_backend_victoriametrics_type() {
        let cfg = make_config(vec![
            BackendConfig {
                name: "vm".to_string(),
                backend_type: "victoriametrics".to_string(),
                url: "http://vm:8428".to_string(),
            },
        ]);
        let b = cfg.find_backend("metrics", None).unwrap();
        assert_eq!(b.backend_type, "victoriametrics");
    }

    #[test]
    fn find_backend_loki_type() {
        let cfg = make_config(vec![
            BackendConfig {
                name: "loki".to_string(),
                backend_type: "loki".to_string(),
                url: "http://loki:3100".to_string(),
            },
        ]);
        let b = cfg.find_backend("logs", None).unwrap();
        assert_eq!(b.backend_type, "loki");
    }

    // ─── toml_from_str ──────────────────────────────────────────────────

    #[test]
    fn toml_from_str_valid() {
        let toml = r#"
listen = "0.0.0.0:8080"

[[backends]]
name = "prom"
type = "prometheus"
url = "http://localhost:9090"
"#;
        let cfg = toml_from_str(toml).unwrap();
        assert_eq!(cfg.listen, "0.0.0.0:8080");
        assert_eq!(cfg.backends.len(), 1);
        assert_eq!(cfg.backends[0].name, "prom");
        assert_eq!(cfg.backends[0].backend_type, "prometheus");
    }

    #[test]
    fn toml_from_str_multiple_backends() {
        let toml = r#"
listen = "0.0.0.0:9090"

[[backends]]
name = "vm"
type = "prometheus"
url = "http://vm:8428"

[[backends]]
name = "vlogs"
type = "victorialogs"
url = "http://vlogs:9428"
"#;
        let cfg = toml_from_str(toml).unwrap();
        assert_eq!(cfg.backends.len(), 2);
    }

    #[test]
    fn toml_from_str_with_api_keys() {
        let toml = r#"
listen = "0.0.0.0:9090"
api_keys = ["key1", "key2"]
cors_origins = ["http://localhost:3000"]
backends = []
"#;
        let cfg = toml_from_str(toml).unwrap();
        assert_eq!(cfg.api_keys.len(), 2);
        assert_eq!(cfg.cors_origins.len(), 1);
    }

    #[test]
    fn toml_from_str_invalid() {
        let result = toml_from_str("not valid toml {{{}}}");
        assert!(result.is_err());
    }

    #[test]
    fn toml_from_str_missing_required_field() {
        let toml = r#"
backends = []
"#;
        // listen is required
        let result = toml_from_str(toml);
        assert!(result.is_err());
    }

    // ─── EngineConfig::load (env-based) ─────────────────────────────────

    #[test]
    fn load_uses_env_listen() {
        // Clear config file env to ensure we hit the env var path
        std::env::remove_var("UNIQL_CONFIG");
        std::env::remove_var("UNIQL_BACKENDS");
        std::env::set_var("UNIQL_LISTEN", "127.0.0.1:3000");
        let cfg = EngineConfig::load();
        assert_eq!(cfg.listen, "127.0.0.1:3000");
        std::env::remove_var("UNIQL_LISTEN");
    }

    #[test]
    fn load_parses_backends_json() {
        std::env::remove_var("UNIQL_CONFIG");
        std::env::set_var("UNIQL_BACKENDS", r#"[{"name":"test","type":"prometheus","url":"http://test:9090"}]"#);
        std::env::remove_var("UNIQL_LISTEN");
        let cfg = EngineConfig::load();
        assert_eq!(cfg.backends.len(), 1);
        assert_eq!(cfg.backends[0].name, "test");
        std::env::remove_var("UNIQL_BACKENDS");
    }

    #[test]
    fn load_parses_api_keys_csv() {
        std::env::remove_var("UNIQL_CONFIG");
        std::env::remove_var("UNIQL_BACKENDS");
        std::env::set_var("UNIQL_API_KEYS", "key1, key2, key3");
        let cfg = EngineConfig::load();
        assert_eq!(cfg.api_keys, vec!["key1", "key2", "key3"]);
        std::env::remove_var("UNIQL_API_KEYS");
    }

    #[test]
    fn load_parses_cors_origins_csv() {
        std::env::remove_var("UNIQL_CONFIG");
        std::env::remove_var("UNIQL_BACKENDS");
        std::env::set_var("UNIQL_CORS_ORIGINS", "http://localhost, http://example.com");
        let cfg = EngineConfig::load();
        assert_eq!(cfg.cors_origins.len(), 2);
        std::env::remove_var("UNIQL_CORS_ORIGINS");
    }

    #[test]
    fn load_defaults_when_no_env() {
        std::env::remove_var("UNIQL_CONFIG");
        std::env::remove_var("UNIQL_BACKENDS");
        std::env::remove_var("UNIQL_LISTEN");
        std::env::remove_var("UNIQL_API_KEYS");
        std::env::remove_var("UNIQL_CORS_ORIGINS");
        let cfg = EngineConfig::load();
        assert_eq!(cfg.listen, "0.0.0.0:9090");
        assert_eq!(cfg.backends.len(), 2);
    }
}
