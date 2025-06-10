pub mod tcp;
pub mod udp;

use crate::config::{Config, ServerConfig};
use crate::health::HealthChecker;
use crate::metrics::MetricsCollector;
use crate::router::{Protocol, RouterManager};
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

pub use tcp::TcpProxy;
pub use udp::UdpProxy;

/// 代理服务器 - 统一管理TCP和UDP代理
pub struct ProxyServer {
    config: ServerConfig,
    router_manager: Arc<RouterManager>,
    health_checker: Arc<HealthChecker>,
    metrics_collector: Arc<MetricsCollector>,
    tcp_proxy: Option<TcpProxy>,
    udp_proxy: Option<UdpProxy>,
    running: Arc<RwLock<bool>>,
    start_time: Instant,
}

impl ProxyServer {
    pub fn new(
        config: ServerConfig,
        router_manager: Arc<RouterManager>,
        health_checker: Arc<HealthChecker>,
        metrics_collector: Arc<MetricsCollector>,
    ) -> Self {
        // 只在配置了TCP时创建TCP代理
        let tcp_proxy = if config.tcp_bind.is_some() {
            Some(TcpProxy::new(
                config.clone(),
                router_manager.clone(),
                metrics_collector.clone(),
            ))
        } else {
            None
        };

        // 只在配置了UDP时创建UDP代理
        let udp_proxy = if config.udp_bind.is_some() {
            Some(UdpProxy::new(
                config.clone(),
                router_manager.clone(),
                metrics_collector.clone(),
            ))
        } else {
            None
        };

        Self {
            config,
            router_manager,
            health_checker,
            metrics_collector,
            tcp_proxy,
            udp_proxy,
            running: Arc::new(RwLock::new(false)),
            start_time: Instant::now(),
        }
    }

    /// 启动代理服务器
    pub async fn start(&self) -> Result<()> {
        {
            let mut running = self.running.write().await;
            if *running {
                warn!("Proxy server is already running");
                return Ok(());
            }
            *running = true;
        }

        info!("Starting proxy server...");

        // 显示配置的绑定地址
        if let Some(tcp_bind) = &self.config.tcp_bind {
            info!("TCP bind address: {}", tcp_bind);
        } else {
            info!("TCP proxy disabled");
        }

        if let Some(udp_bind) = &self.config.udp_bind {
            info!("UDP bind address: {}", udp_bind);
        } else {
            info!("UDP proxy disabled");
        }

        // 启动健康检查
        self.health_checker
            .start()
            .await
            .context("Failed to start health checker")?;

        // 启动指标收集
        self.metrics_collector
            .start_collection()
            .await
            .context("Failed to start metrics collection")?;

        // 启动TCP代理
        if let Some(tcp_proxy) = &self.tcp_proxy {
            tcp_proxy
                .start()
                .await
                .context("Failed to start TCP proxy")?;
        }

        // 启动UDP代理
        if let Some(udp_proxy) = &self.udp_proxy {
            udp_proxy
                .start()
                .await
                .context("Failed to start UDP proxy")?;
        }

        info!("Proxy server started successfully");
        Ok(())
    }

    /// 停止代理服务器
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping proxy server...");

        {
            let mut running = self.running.write().await;
            *running = false;
        }

        // 停止TCP代理
        if let Some(tcp_proxy) = &self.tcp_proxy {
            tcp_proxy.stop().await.context("Failed to stop TCP proxy")?;
        }

        // 停止UDP代理
        if let Some(udp_proxy) = &self.udp_proxy {
            udp_proxy.stop().await.context("Failed to stop UDP proxy")?;
        }

        // 停止健康检查
        self.health_checker
            .stop()
            .await
            .context("Failed to stop health checker")?;

        info!("Proxy server stopped");
        Ok(())
    }

    /// 优雅关闭
    pub async fn graceful_shutdown(&self, timeout_secs: u64) -> Result<()> {
        info!("Starting graceful shutdown (timeout: {}s)...", timeout_secs);

        let shutdown_start = Instant::now();

        // 停止接受新连接
        {
            let mut running = self.running.write().await;
            *running = false;
        }

        // 等待现有连接完成或超时
        let timeout_duration = std::time::Duration::from_secs(timeout_secs);

        tokio::select! {
            _ = self.wait_for_connections_to_finish() => {
                info!("All connections finished gracefully");
            }
            _ = tokio::time::sleep(timeout_duration) => {
                warn!("Graceful shutdown timeout reached, forcing shutdown");
            }
        }

        // 强制停止所有服务
        self.stop().await?;

        let shutdown_duration = shutdown_start.elapsed();
        info!("Graceful shutdown completed in {:?}", shutdown_duration);
        Ok(())
    }

    /// 等待所有连接完成
    async fn wait_for_connections_to_finish(&self) {
        // 这里应该实现等待所有活跃连接完成的逻辑
        // 可以通过监控活跃连接数指标来实现
        loop {
            let active_connections = self.get_active_connections_count().await;
            if active_connections == 0 {
                break;
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// 获取活跃连接数
    async fn get_active_connections_count(&self) -> u64 {
        // 从指标收集器获取活跃连接数
        // 这里简化实现，实际应该从指标中读取
        0
    }

    /// 重新加载配置
    pub async fn reload_config(&self, new_config: &Config) -> Result<()> {
        info!("Reloading proxy server configuration...");

        // 重新加载路由管理器配置
        self.router_manager
            .reload_config(new_config)
            .await
            .context("Failed to reload router manager configuration")?;

        // 重新加载健康检查配置
        // 注意：这里可能需要重启健康检查器

        // 重新加载代理配置
        // 注意：某些配置更改可能需要重启代理服务

        info!("Proxy server configuration reloaded successfully");
        Ok(())
    }

    /// 获取服务器状态
    pub async fn get_status(&self) -> ProxyServerStatus {
        let running = *self.running.read().await;
        let uptime = self.start_time.elapsed();

        let tcp_status = if let Some(tcp_proxy) = &self.tcp_proxy {
            Some(tcp_proxy.get_status().await)
        } else {
            None
        };

        let udp_status = if let Some(udp_proxy) = &self.udp_proxy {
            Some(udp_proxy.get_status().await)
        } else {
            None
        };

        let health_stats = self.health_checker.get_stats().await;

        ProxyServerStatus {
            running,
            uptime,
            tcp_status,
            udp_status,
            health_stats,
            active_connections: self.get_active_connections_count().await,
        }
    }

    /// 获取详细统计信息
    pub async fn get_detailed_stats(&self) -> Result<ProxyServerStats> {
        let router_stats = self.router_manager.get_all_stats().await?;
        let health_stats = self.health_checker.get_stats().await;
        let metrics_output = self.metrics_collector.export_metrics()?;

        Ok(ProxyServerStats {
            uptime: self.start_time.elapsed(),
            router_stats,
            health_stats,
            metrics_output,
        })
    }
}

/// 代理服务器状态
#[derive(Debug, Clone)]
pub struct ProxyServerStatus {
    pub running: bool,
    pub uptime: std::time::Duration,
    pub tcp_status: Option<tcp::TcpProxyStatus>,
    pub udp_status: Option<udp::UdpProxyStatus>,
    pub health_stats: crate::health::HealthCheckStats,
    pub active_connections: u64,
}

/// 代理服务器统计信息
#[derive(Debug)]
pub struct ProxyServerStats {
    pub uptime: std::time::Duration,
    pub router_stats: std::collections::HashMap<String, crate::router::RouterStats>,
    pub health_stats: crate::health::HealthCheckStats,
    pub metrics_output: String,
}

/// 连接信息
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub id: String,
    pub protocol: Protocol,
    pub client_addr: std::net::SocketAddr,
    pub target_addr: std::net::SocketAddr,
    pub backend_addr: Option<std::net::SocketAddr>,
    pub start_time: Instant,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub rule_name: Option<String>,
    pub router_name: Option<String>,
}

impl ConnectionInfo {
    pub fn new(
        protocol: Protocol,
        client_addr: std::net::SocketAddr,
        target_addr: std::net::SocketAddr,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            protocol,
            client_addr,
            target_addr,
            backend_addr: None,
            start_time: Instant::now(),
            bytes_sent: 0,
            bytes_received: 0,
            rule_name: None,
            router_name: None,
        }
    }

    pub fn duration(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    pub fn total_bytes(&self) -> u64 {
        self.bytes_sent + self.bytes_received
    }
}

/// 代理错误类型
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("No backend available")]
    NoBackendAvailable,

    #[error("Backend connection failed: {0}")]
    BackendConnectionFailed(#[from] std::io::Error),

    #[error("Routing failed: {0}")]
    RoutingFailed(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Health check failed: {0}")]
    HealthCheckFailed(String),

    #[error("Metrics error: {0}")]
    MetricsError(String),
}

/// 代理结果
pub type ProxyResult<T> = Result<T, ProxyError>;
