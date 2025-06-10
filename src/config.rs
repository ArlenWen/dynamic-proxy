use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub redis: RedisConfig,
    pub logging: LoggingConfig,
    pub metrics: MetricsConfig,
    pub health_check: HealthCheckConfig,
    pub routers: HashMap<String, RouterConfig>,
    pub rules: Vec<RoutingRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub tcp_bind: SocketAddr,
    pub udp_bind: SocketAddr,
    pub worker_threads: Option<usize>,
    pub max_connections: Option<usize>,
    pub connection_timeout: Option<u64>, // seconds
    pub buffer_size: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    pub url: String,
    pub password: Option<String>,
    pub db: Option<i64>,
    pub pool_size: Option<u32>,
    pub connection_timeout: Option<u64>, // seconds
    pub command_timeout: Option<u64>,    // seconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String, // "json" or "pretty"
    pub file: Option<String>,
    pub max_size: Option<u64>, // MB
    pub max_files: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub bind: SocketAddr,
    pub path: String,
    pub collect_interval: Option<u64>, // seconds
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: "0.0.0.0:9090".parse().unwrap(),
            path: "/metrics".to_string(),
            collect_interval: Some(10),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    pub enabled: bool,
    pub interval: u64, // seconds
    pub timeout: u64,  // seconds
    pub retries: u32,
    pub failure_threshold: u32,
    pub success_threshold: u32,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: 30,
            timeout: 5,
            retries: 3,
            failure_threshold: 3,
            success_threshold: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    pub plugin: String,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    pub name: String,
    pub protocol: Protocol,
    pub matcher: RuleMatcher,
    pub router: String,
    pub backends: Vec<Backend>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RuleMatcher {
    #[serde(rename = "port")]
    Port { port: u16 },
    #[serde(rename = "port_range")]
    PortRange { start: u16, end: u16 },
    #[serde(rename = "sni")]
    Sni { pattern: String },
    #[serde(rename = "source_ip")]
    SourceIp { cidr: String },
    #[serde(rename = "destination_ip")]
    DestinationIp { cidr: String },
    #[serde(rename = "custom")]
    Custom {
        plugin: String,
        config: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backend {
    pub id: String,
    pub address: SocketAddr,
    pub weight: Option<u32>,
    pub enabled: bool,
    pub metadata: Option<HashMap<String, String>>,
}

pub struct ConfigManager {
    config: RwLock<Config>,
    file_path: String,
}

impl ConfigManager {
    pub fn new(file_path: String) -> Self {
        Self {
            config: RwLock::new(Config::default()),
            file_path,
        }
    }

    pub async fn load(&self) -> Result<()> {
        let content = fs::read_to_string(&self.file_path)
            .await
            .with_context(|| format!("Failed to read config file: {}", self.file_path))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", self.file_path))?;

        *self.config.write().await = config;
        info!("Configuration loaded from {}", self.file_path);
        Ok(())
    }

    pub async fn reload(&self) -> Result<()> {
        info!("Reloading configuration...");
        self.load().await?;
        info!("Configuration reloaded successfully");
        Ok(())
    }

    pub async fn get(&self) -> Config {
        self.config.read().await.clone()
    }

    pub async fn update_router(&self, name: String, router_config: RouterConfig) -> Result<()> {
        let mut config = self.config.write().await;
        config.routers.insert(name.clone(), router_config);
        info!("Router '{}' configuration updated", name);
        Ok(())
    }

    pub async fn update_rule(&self, rule: RoutingRule) -> Result<()> {
        let mut config = self.config.write().await;

        // Find and update existing rule or add new one
        if let Some(existing_rule) = config.rules.iter_mut().find(|r| r.name == rule.name) {
            *existing_rule = rule.clone();
            info!("Routing rule '{}' updated", rule.name);
        } else {
            config.rules.push(rule.clone());
            info!("New routing rule '{}' added", rule.name);
        }

        Ok(())
    }

    pub async fn remove_rule(&self, name: &str) -> Result<()> {
        let mut config = self.config.write().await;
        config.rules.retain(|r| r.name != name);
        info!("Routing rule '{}' removed", name);
        Ok(())
    }

    pub async fn watch_file(&self) -> Result<()> {
        // TODO: Implement file watching for automatic reload
        // This would use inotify on Linux or similar mechanisms on other platforms
        warn!("File watching not implemented yet");
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                tcp_bind: "0.0.0.0:8080".parse().unwrap(),
                udp_bind: "0.0.0.0:8081".parse().unwrap(),
                worker_threads: None,
                max_connections: Some(10000),
                connection_timeout: Some(30),
                buffer_size: Some(8192),
            },
            redis: RedisConfig {
                url: "redis://localhost:6379".to_string(),
                password: None,
                db: Some(0),
                pool_size: Some(10),
                connection_timeout: Some(5),
                command_timeout: Some(3),
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                format: "pretty".to_string(),
                file: None,
                max_size: Some(100),
                max_files: Some(10),
            },
            metrics: MetricsConfig {
                enabled: true,
                bind: "0.0.0.0:9090".parse().unwrap(),
                path: "/metrics".to_string(),
                collect_interval: Some(10),
            },
            health_check: HealthCheckConfig {
                enabled: true,
                interval: 30,
                timeout: 5,
                retries: 3,
                failure_threshold: 3,
                success_threshold: 2,
            },
            routers: HashMap::new(),
            rules: Vec::new(),
        }
    }
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", path.as_ref()))?;

        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        // Validate server configuration
        if self.server.tcp_bind.port() == 0 {
            return Err(anyhow::anyhow!("TCP bind port cannot be 0"));
        }
        if self.server.udp_bind.port() == 0 {
            return Err(anyhow::anyhow!("UDP bind port cannot be 0"));
        }

        // Validate Redis configuration
        if self.redis.url.is_empty() {
            return Err(anyhow::anyhow!("Redis URL cannot be empty"));
        }

        // Validate routing rules
        for rule in &self.rules {
            if rule.name.is_empty() {
                return Err(anyhow::anyhow!("Routing rule name cannot be empty"));
            }
            if rule.backends.is_empty() {
                return Err(anyhow::anyhow!(
                    "Routing rule '{}' must have at least one backend",
                    rule.name
                ));
            }
            if !self.routers.contains_key(&rule.router) {
                return Err(anyhow::anyhow!(
                    "Router '{}' referenced in rule '{}' is not defined",
                    rule.router,
                    rule.name
                ));
            }
        }

        Ok(())
    }
}
