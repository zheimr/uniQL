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
