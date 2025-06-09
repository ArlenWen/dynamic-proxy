pub mod tcp_proxy;
pub mod udp_proxy;
pub mod tls_handler;

use crate::config::Settings;
use crate::monitoring::ProxyMetrics;
use crate::routing::{PluginManager};
use crate::routing::plugin::SniRouterPlugin;
use crate::routing::redis_backend::RedisBackend;
use crate::shutdown::{GracefulShutdown, ShutdownSignal, ShutdownState};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{error, info, warn};

pub struct ProxyServer {
    settings: Settings,
    metrics: Arc<ProxyMetrics>,
    plugin_manager: Arc<PluginManager>,
    shutdown_manager: Arc<GracefulShutdown>,
}

impl ProxyServer {
    pub async fn new(settings: Settings) -> Result<Self> {
        let metrics = Arc::new(ProxyMetrics::new()?);

        // 初始化优雅关机管理器
        let shutdown_manager = Arc::new(
            GracefulShutdown::new()
                .with_shutdown_timeout(Duration::from_secs(30))
                .with_drain_timeout(Duration::from_secs(15))
                .with_force_shutdown(true)
        );

        // 初始化插件管理器
        let mut plugin_manager = PluginManager::new();

        // 注册内置插件（使用可变插件）
        plugin_manager.register_mutable_plugin(Box::new(SniRouterPlugin::new())).await;
        plugin_manager.register_mutable_plugin(Box::new(RedisBackend::new())).await;

        // 初始化插件
        plugin_manager.initialize_plugins(
            settings.plugins.plugin_config.clone()
        ).await?;

        Ok(Self {
            settings,
            metrics,
            plugin_manager: Arc::new(plugin_manager),
            shutdown_manager,
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting proxy server...");

        // 启动信号处理
        self.shutdown_manager.start_signal_handling().await?;

        // 启动指标服务器
        let metrics_handle = if self.settings.monitoring.enable_metrics {
            Some(self.start_metrics_server().await?)
        } else {
            None
        };

        // 启动TCP代理
        let tcp_handle = self.start_tcp_proxy().await?;

        // 启动UDP代理
        let udp_handle = self.start_udp_proxy().await?;

        info!("Proxy server started successfully");
        info!("TCP proxy listening on: {}", self.settings.server.tcp_bind);
        info!("UDP proxy listening on: {}", self.settings.server.udp_bind);

        if self.settings.monitoring.enable_metrics {
            info!("Metrics server listening on: {}", self.settings.monitoring.metrics_bind);
        }

        info!("Server is ready to accept connections");
        info!("Send SIGTERM or SIGINT for graceful shutdown, SIGQUIT for immediate shutdown");

        // 等待关闭信号并执行优雅关机
        self.shutdown_manager.execute_graceful_shutdown().await?;

        // 执行清理流程
        self.cleanup_resources(tcp_handle, udp_handle, metrics_handle).await?;

        info!("Proxy server shutdown complete");
        Ok(())
    }

    async fn start_tcp_proxy(&self) -> Result<tokio::task::JoinHandle<()>> {
        let tcp_proxy = tcp_proxy::TcpProxy::new(
            self.settings.clone(),
            Arc::clone(&self.metrics),
            Arc::clone(&self.plugin_manager),
            Arc::clone(&self.shutdown_manager),
        ).await?;

        let handle = tokio::spawn(async move {
            if let Err(e) = tcp_proxy.run().await {
                error!("TCP proxy error: {}", e);
            }
        });

        Ok(handle)
    }

    async fn start_udp_proxy(&self) -> Result<tokio::task::JoinHandle<()>> {
        let udp_proxy = udp_proxy::UdpProxy::new(
            self.settings.clone(),
            Arc::clone(&self.metrics),
            Arc::clone(&self.plugin_manager),
            Arc::clone(&self.shutdown_manager),
        ).await?;

        let handle = tokio::spawn(async move {
            if let Err(e) = udp_proxy.run().await {
                error!("UDP proxy error: {}", e);
            }
        });

        Ok(handle)
    }

    async fn start_metrics_server(&self) -> Result<tokio::task::JoinHandle<()>> {
        let metrics = Arc::clone(&self.metrics);
        let bind_addr = self.settings.monitoring.metrics_bind.clone();
        let mut shutdown_rx = self.shutdown_manager.subscribe();

        let handle = tokio::spawn(async move {
            use tokio::net::TcpListener;
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let listener = match TcpListener::bind(&bind_addr).await {
                Ok(listener) => listener,
                Err(e) => {
                    error!("Failed to bind metrics server: {}", e);
                    return;
                }
            };

            info!("Metrics server listening on: {}", bind_addr);

            loop {
                tokio::select! {
                    // 处理新连接
                    result = listener.accept() => {
                        match result {
                            Ok((mut stream, _)) => {
                                let metrics = Arc::clone(&metrics);
                                tokio::spawn(async move {
                                    let mut buffer = [0; 1024];
                                    if let Ok(_) = stream.read(&mut buffer).await {
                                        let response = format!(
                                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n{}",
                                            metrics.export_metrics()
                                        );
                                        let _ = stream.write_all(response.as_bytes()).await;
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Failed to accept metrics connection: {}", e);
                            }
                        }
                    }
                    // 监听关机信号
                    _ = shutdown_rx.recv() => {
                        info!("Metrics server received shutdown signal");
                        break;
                    }
                }
            }

            info!("Metrics server stopped");
        });

        Ok(handle)
    }

    /// 清理所有资源
    async fn cleanup_resources(
        &self,
        tcp_handle: tokio::task::JoinHandle<()>,
        udp_handle: tokio::task::JoinHandle<()>,
        metrics_handle: Option<tokio::task::JoinHandle<()>>,
    ) -> Result<()> {
        info!("Starting resource cleanup...");

        // 等待服务停止，设置超时
        let cleanup_timeout = Duration::from_secs(10);

        // 优雅停止TCP代理
        if !tcp_handle.is_finished() {
            info!("Stopping TCP proxy...");
            match timeout(cleanup_timeout, async {
                tcp_handle.abort();
                let _ = tcp_handle.await;
            }).await {
                Ok(_) => info!("TCP proxy stopped"),
                Err(_) => warn!("TCP proxy stop timeout"),
            }
        }

        // 优雅停止UDP代理
        if !udp_handle.is_finished() {
            info!("Stopping UDP proxy...");
            match timeout(cleanup_timeout, async {
                udp_handle.abort();
                let _ = udp_handle.await;
            }).await {
                Ok(_) => info!("UDP proxy stopped"),
                Err(_) => warn!("UDP proxy stop timeout"),
            }
        }

        // 停止指标服务器
        if let Some(handle) = metrics_handle {
            if !handle.is_finished() {
                info!("Stopping metrics server...");
                match timeout(cleanup_timeout, async {
                    handle.abort();
                    let _ = handle.await;
                }).await {
                    Ok(_) => info!("Metrics server stopped"),
                    Err(_) => warn!("Metrics server stop timeout"),
                }
            }
        }

        // 关闭插件
        info!("Shutting down plugins...");
        match timeout(cleanup_timeout, self.plugin_manager.shutdown()).await {
            Ok(result) => {
                if let Err(e) = result {
                    error!("Error shutting down plugins: {}", e);
                } else {
                    info!("Plugins shut down successfully");
                }
            }
            Err(_) => {
                warn!("Plugin shutdown timeout");
            }
        }

        // 等待所有连接关闭
        let active_connections = self.shutdown_manager.get_active_connections().await;
        if active_connections > 0 {
            info!("Waiting for {} active connections to close...", active_connections);

            let mut wait_count = 0;
            while wait_count < 50 { // 最多等待5秒
                let current_connections = self.shutdown_manager.get_active_connections().await;
                if current_connections == 0 {
                    break;
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
                wait_count += 1;
            }

            let final_connections = self.shutdown_manager.get_active_connections().await;
            if final_connections > 0 {
                warn!("Force closing {} remaining connections", final_connections);
            } else {
                info!("All connections closed gracefully");
            }
        }

        info!("Resource cleanup completed");
        Ok(())
    }

    /// 获取关机管理器的引用（用于外部访问）
    pub fn shutdown_manager(&self) -> &Arc<GracefulShutdown> {
        &self.shutdown_manager
    }

    /// 手动触发关机
    pub async fn trigger_shutdown(&self, signal: ShutdownSignal) -> Result<()> {
        self.shutdown_manager.trigger_shutdown(signal).await
    }

    /// 检查是否正在关机
    pub async fn is_shutting_down(&self) -> bool {
        self.shutdown_manager.is_shutting_down().await
    }

    /// 获取当前关机状态
    pub async fn get_shutdown_state(&self) -> ShutdownState {
        self.shutdown_manager.get_state().await
    }
}
