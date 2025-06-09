use serde::{Deserialize, Serialize};
use std::path::Path;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub server: ServerConfig,
    pub redis: RedisConfig,
    pub tls: Option<TlsConfig>,
    pub monitoring: MonitoringConfig,
    pub plugins: PluginConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub tcp_bind: String,
    pub udp_bind: String,
    pub buffer_size: usize,
    pub max_connections: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    pub url: String,
    pub pool_size: u32,
    pub timeout_ms: u64,
    pub cache_ttl_secs: u64,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
    pub ca_path: Option<String>,
    pub sni_routing: bool,
    pub certificates: Option<Vec<CertificateConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateConfig {
    pub hostname: String,
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    pub metrics_bind: String,
    pub log_level: String,
    pub enable_metrics: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub enabled_plugins: Vec<String>,
    pub plugin_config: std::collections::HashMap<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                tcp_bind: "0.0.0.0:8080".to_string(),
                udp_bind: "0.0.0.0:8081".to_string(),
                buffer_size: 8192,
                max_connections: 1000,
            },
            redis: RedisConfig {
                url: "redis://127.0.0.1:6379".to_string(),
                pool_size: 10,
                timeout_ms: 5000,
                cache_ttl_secs: 300,
                max_retries: 3,
                retry_delay_ms: 1000,
            },
            tls: Some(TlsConfig {
                cert_path: "certs/server.crt".to_string(),
                key_path: "certs/server.key".to_string(),
                ca_path: None,
                sni_routing: true,
                certificates: Some(vec![
                    CertificateConfig {
                        hostname: "api.example.com".to_string(),
                        cert_path: "certs/api.crt".to_string(),
                        key_path: "certs/api.key".to_string(),
                    },
                    CertificateConfig {
                        hostname: "*.example.com".to_string(),
                        cert_path: "certs/wildcard.crt".to_string(),
                        key_path: "certs/wildcard.key".to_string(),
                    },
                ]),
            }),
            monitoring: MonitoringConfig {
                metrics_bind: "0.0.0.0:9090".to_string(),
                log_level: "info".to_string(),
                enable_metrics: true,
            },
            plugins: PluginConfig {
                enabled_plugins: vec!["sni_router".to_string(), "redis_backend".to_string()],
                plugin_config: std::collections::HashMap::new(),
            },
        }
    }
}

impl Settings {
    pub async fn load() -> Result<Self> {
        Self::load_from_path("config.toml").await
    }

    pub async fn load_from_path<P: AsRef<Path>>(config_path: P) -> Result<Self> {
        let config_path = config_path.as_ref();

        // 尝试从配置文件加载
        if config_path.exists() {
            let content = tokio::fs::read_to_string(config_path).await?;
            let settings: Settings = toml::from_str(&content)?;
            Ok(settings)
        } else {
            // 使用默认配置并保存到文件
            let settings = Settings::default();
            settings.save_to_path(config_path).await?;
            Ok(settings)
        }
    }

    pub async fn save(&self) -> Result<()> {
        self.save_to_path("config.toml").await
    }

    pub async fn save_to_path<P: AsRef<Path>>(&self, config_path: P) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        tokio::fs::write(config_path, content).await?;
        Ok(())
    }
}
