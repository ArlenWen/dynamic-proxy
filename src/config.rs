use anyhow::{Context, Result};
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use tokio::fs;
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub redis: Option<RedisConfig>,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub metrics: MetricsConfig,
    #[serde(default)]
    pub health_check: HealthCheckConfig,
    #[serde(default)]
    pub routers: HashMap<String, RouterConfig>,
    #[serde(default)]
    pub rules: Vec<RoutingRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub tcp_bind: Option<SocketAddr>,
    pub udp_bind: Option<SocketAddr>,
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

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "pretty".to_string(),
            file: None,
            max_size: Some(100),
            max_files: Some(10),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub bind: Option<SocketAddr>,
    pub path: Option<String>,
    pub collect_interval: Option<u64>, // seconds
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: Some("0.0.0.0:9090".parse().unwrap()),
            path: Some("/metrics".to_string()),
            collect_interval: Some(10),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    pub enabled: bool,
    pub interval: Option<u64>, // seconds
    pub timeout: Option<u64>,  // seconds
    pub retries: Option<u32>,
    pub failure_threshold: Option<u32>,
    pub success_threshold: Option<u32>,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: Some(30),
            timeout: Some(5),
            retries: Some(3),
            failure_threshold: Some(3),
            success_threshold: Some(2),
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
    file_path: PathBuf,
    reload_tx: Option<mpsc::UnboundedSender<()>>,
}

impl ConfigManager {
    pub fn new(file_path: String) -> Self {
        Self {
            config: RwLock::new(Config::default()),
            file_path: PathBuf::from(file_path),
            reload_tx: None,
        }
    }

    pub async fn load(&self) -> Result<()> {
        let content = fs::read_to_string(&self.file_path)
            .await
            .with_context(|| format!("Failed to read config file: {}", self.file_path.display()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", self.file_path.display()))?;

        *self.config.write().await = config;
        info!("Configuration loaded from {}", self.file_path.display());
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

    /// 启动文件监控，返回重载通知接收器
    pub fn start_file_watcher(&mut self) -> Result<mpsc::UnboundedReceiver<()>> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.reload_tx = Some(tx.clone());

        let file_path = self.file_path.clone();

        std::thread::spawn(move || {
            if let Err(e) = Self::watch_file_changes(file_path, tx) {
                error!("File watcher error: {}", e);
            }
        });

        info!("Started file watcher for: {:?}", self.file_path);
        Ok(rx)
    }

    /// 文件监控实现
    fn watch_file_changes(file_path: PathBuf, tx: mpsc::UnboundedSender<()>) -> Result<()> {
        let (watch_tx, watch_rx) = std::sync::mpsc::channel();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Err(e) = watch_tx.send(res) {
                    error!("Failed to send watch event: {}", e);
                }
            },
            NotifyConfig::default(),
        )?;

        // 监控配置文件的父目录
        let watch_dir = file_path.parent().unwrap_or_else(|| Path::new("."));
        watcher.watch(watch_dir, RecursiveMode::NonRecursive)?;

        info!("File watcher started for directory: {:?}", watch_dir);

        // 处理文件变更事件
        for res in watch_rx {
            match res {
                Ok(event) => {
                    // 检查是否是我们关心的配置文件
                    if event.paths.iter().any(|p| p == &file_path) {
                        match event.kind {
                            notify::EventKind::Modify(_) | notify::EventKind::Create(_) => {
                                info!("Configuration file changed: {:?}", file_path);
                                if let Err(e) = tx.send(()) {
                                    error!("Failed to send reload signal: {}", e);
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    error!("File watch error: {}", e);
                }
            }
        }

        Ok(())
    }

    /// 手动触发重载
    pub fn trigger_reload(&self) -> Result<()> {
        if let Some(tx) = &self.reload_tx {
            tx.send(()).context("Failed to send reload signal")?;
            info!("Manual reload triggered");
        } else {
            warn!("File watcher not started, cannot trigger reload");
        }
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                tcp_bind: Some("0.0.0.0:8080".parse().unwrap()),
                udp_bind: Some("0.0.0.0:8081".parse().unwrap()),
                worker_threads: None,
                max_connections: Some(10000),
                connection_timeout: Some(30),
                buffer_size: Some(8192),
            },
            redis: Some(RedisConfig {
                url: "redis://localhost:6379".to_string(),
                password: None,
                db: Some(0),
                pool_size: Some(10),
                connection_timeout: Some(5),
                command_timeout: Some(3),
            }),
            logging: LoggingConfig {
                level: "info".to_string(),
                format: "pretty".to_string(),
                file: None,
                max_size: Some(100),
                max_files: Some(10),
            },
            metrics: MetricsConfig {
                enabled: true,
                bind: Some("0.0.0.0:9090".parse().unwrap()),
                path: Some("/metrics".to_string()),
                collect_interval: Some(10),
            },
            health_check: HealthCheckConfig {
                enabled: true,
                interval: Some(30),
                timeout: Some(5),
                retries: Some(3),
                failure_threshold: Some(3),
                success_threshold: Some(2),
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
        // Validate server configuration - at least one protocol must be enabled
        if self.server.tcp_bind.is_none() && self.server.udp_bind.is_none() {
            return Err(anyhow::anyhow!(
                "At least one of TCP or UDP bind address must be configured"
            ));
        }

        // Validate TCP bind address if configured
        if let Some(tcp_bind) = &self.server.tcp_bind {
            if tcp_bind.port() == 0 {
                return Err(anyhow::anyhow!("TCP bind port cannot be 0"));
            }
        }

        // Validate UDP bind address if configured
        if let Some(udp_bind) = &self.server.udp_bind {
            if udp_bind.port() == 0 {
                return Err(anyhow::anyhow!("UDP bind port cannot be 0"));
            }
        }

        // Validate Redis configuration if provided
        if let Some(redis_config) = &self.redis {
            if redis_config.url.is_empty() {
                return Err(anyhow::anyhow!("Redis URL cannot be empty"));
            }
        }

        // Validate metrics configuration
        if self.metrics.enabled {
            if self.metrics.bind.is_none() {
                return Err(anyhow::anyhow!(
                    "Metrics bind address must be specified when metrics are enabled"
                ));
            }
            if self.metrics.path.is_none() {
                return Err(anyhow::anyhow!(
                    "Metrics path must be specified when metrics are enabled"
                ));
            }
        }

        // Validate health check configuration
        if self.health_check.enabled {
            if self.health_check.interval.is_none() {
                return Err(anyhow::anyhow!(
                    "Health check interval must be specified when health check is enabled"
                ));
            }
            if self.health_check.timeout.is_none() {
                return Err(anyhow::anyhow!(
                    "Health check timeout must be specified when health check is enabled"
                ));
            }
            if self.health_check.retries.is_none() {
                return Err(anyhow::anyhow!(
                    "Health check retries must be specified when health check is enabled"
                ));
            }
            if self.health_check.failure_threshold.is_none() {
                return Err(anyhow::anyhow!(
                    "Health check failure_threshold must be specified when health check is enabled"
                ));
            }
            if self.health_check.success_threshold.is_none() {
                return Err(anyhow::anyhow!(
                    "Health check success_threshold must be specified when health check is enabled"
                ));
            }
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
