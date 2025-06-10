use super::{ConnectionInfo, ProxyError, ProxyResult};
use crate::config::ServerConfig;
use crate::metrics::MetricsCollector;
use crate::router::{Protocol, RouterManager, RoutingContext};
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// TCP代理服务器
pub struct TcpProxy {
    config: ServerConfig,
    router_manager: Arc<RouterManager>,
    metrics_collector: Arc<MetricsCollector>,
    #[allow(dead_code)]
    listener: Option<TcpListener>,
    running: Arc<RwLock<bool>>,
    active_connections: Arc<RwLock<std::collections::HashMap<String, ConnectionInfo>>>,
}

impl TcpProxy {
    pub fn new(
        config: ServerConfig,
        router_manager: Arc<RouterManager>,
        metrics_collector: Arc<MetricsCollector>,
    ) -> Self {
        Self {
            config,
            router_manager,
            metrics_collector,
            listener: None,
            running: Arc::new(RwLock::new(false)),
            active_connections: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// 启动TCP代理
    pub async fn start(&self) -> Result<()> {
        {
            let mut running = self.running.write().await;
            if *running {
                warn!("TCP proxy is already running");
                return Ok(());
            }
            *running = true;
        }

        info!("Starting TCP proxy on {}", self.config.tcp_bind);

        let listener = TcpListener::bind(self.config.tcp_bind)
            .await
            .with_context(|| format!("Failed to bind TCP listener to {}", self.config.tcp_bind))?;

        info!("TCP proxy listening on {}", self.config.tcp_bind);

        let router_manager = self.router_manager.clone();
        let metrics_collector = self.metrics_collector.clone();
        let running = self.running.clone();
        let active_connections = self.active_connections.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            loop {
                // 检查是否应该停止
                {
                    let running_guard = running.read().await;
                    if !*running_guard {
                        break;
                    }
                }

                // 接受新连接
                match listener.accept().await {
                    Ok((stream, client_addr)) => {
                        debug!("Accepted TCP connection from {}", client_addr);

                        let router_manager = router_manager.clone();
                        let metrics_collector = metrics_collector.clone();
                        let active_connections = active_connections.clone();
                        let config = config.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                client_addr,
                                router_manager,
                                metrics_collector,
                                active_connections,
                                config,
                            )
                            .await
                            {
                                error!(
                                    "Failed to handle TCP connection from {}: {}",
                                    client_addr, e
                                );
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept TCP connection: {}", e);
                        // 短暂延迟以避免忙循环
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }

            info!("TCP proxy stopped accepting connections");
        });

        Ok(())
    }

    /// 停止TCP代理
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping TCP proxy...");

        {
            let mut running = self.running.write().await;
            *running = false;
        }

        // 等待活跃连接完成
        let mut attempts = 0;
        while attempts < 30 {
            // 最多等待30秒
            let active_count = {
                let connections = self.active_connections.read().await;
                connections.len()
            };

            if active_count == 0 {
                break;
            }

            debug!(
                "Waiting for {} active TCP connections to finish",
                active_count
            );
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            attempts += 1;
        }

        info!("TCP proxy stopped");
        Ok(())
    }

    /// 处理单个连接
    async fn handle_connection(
        mut client_stream: TcpStream,
        client_addr: std::net::SocketAddr,
        router_manager: Arc<RouterManager>,
        metrics_collector: Arc<MetricsCollector>,
        active_connections: Arc<RwLock<std::collections::HashMap<String, ConnectionInfo>>>,
        config: ServerConfig,
    ) -> ProxyResult<()> {
        let start_time = Instant::now();

        // 记录新连接
        metrics_collector.record_new_connection();

        // 创建连接信息
        let mut connection_info = ConnectionInfo::new(
            Protocol::Tcp,
            client_addr,
            config.tcp_bind, // 这里应该是实际的目标地址
        );

        // 添加到活跃连接列表
        {
            let mut connections = active_connections.write().await;
            connections.insert(connection_info.id.clone(), connection_info.clone());
        }

        let result = Self::proxy_connection(
            &mut client_stream,
            client_addr,
            &router_manager,
            &metrics_collector,
            &mut connection_info,
            &config,
        )
        .await;

        // 从活跃连接列表中移除
        {
            let mut connections = active_connections.write().await;
            connections.remove(&connection_info.id);
        }

        // 记录连接结束
        let duration = start_time.elapsed();
        metrics_collector.record_connection_closed(duration);

        // 记录请求完成
        let success = result.is_ok();
        if let Some(rule_name) = &connection_info.rule_name {
            let backend_id = connection_info
                .backend_addr
                .map(|addr| addr.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let metrics = crate::metrics::RequestMetrics::new(
                "tcp",
                rule_name,
                &backend_id,
                if success { "success" } else { "error" },
                duration,
                connection_info.bytes_sent,
                connection_info.bytes_received,
            );
            metrics_collector.record_request(&metrics);
        }

        result
    }

    /// 代理连接
    async fn proxy_connection(
        client_stream: &mut TcpStream,
        client_addr: std::net::SocketAddr,
        router_manager: &RouterManager,
        _metrics_collector: &MetricsCollector,
        connection_info: &mut ConnectionInfo,
        config: &ServerConfig,
    ) -> ProxyResult<()> {
        // 创建路由上下文
        let routing_context = RoutingContext::new(
            client_addr,
            config.tcp_bind, // 这里应该是实际的目标地址
            Protocol::Tcp,
        );

        // 路由到后端
        let backend = router_manager
            .route(&routing_context)
            .await
            .map_err(|e| ProxyError::RoutingFailed(e.to_string()))?
            .ok_or(ProxyError::NoBackendAvailable)?;

        debug!(
            "Routing TCP connection from {} to backend {}",
            client_addr, backend.address
        );

        // 更新连接信息
        connection_info.backend_addr = Some(backend.address);
        // connection_info.rule_name = Some(rule_name); // 需要从路由结果获取
        // connection_info.router_name = Some(router_name); // 需要从路由结果获取

        // 连接到后端
        let mut backend_stream = TcpStream::connect(backend.address)
            .await
            .map_err(ProxyError::BackendConnectionFailed)?;

        debug!(
            "Connected to backend {} for client {}",
            backend.address, client_addr
        );

        // 开始代理数据
        let result = Self::proxy_data(client_stream, &mut backend_stream, connection_info).await;

        if let Err(ref e) = result {
            error!(
                "Data proxy failed for connection {}: {}",
                connection_info.id, e
            );
        }

        result
    }

    /// 代理数据传输
    async fn proxy_data(
        client_stream: &mut TcpStream,
        backend_stream: &mut TcpStream,
        connection_info: &mut ConnectionInfo,
    ) -> ProxyResult<()> {
        let (mut client_read, mut client_write) = client_stream.split();
        let (mut backend_read, mut backend_write) = backend_stream.split();

        let client_to_backend = async {
            let mut buffer = vec![0u8; 8192];
            let mut total_bytes = 0u64;

            loop {
                match client_read.read(&mut buffer).await {
                    Ok(0) => break, // 连接关闭
                    Ok(n) => {
                        if let Err(e) = backend_write.write_all(&buffer[..n]).await {
                            error!("Failed to write to backend: {}", e);
                            break;
                        }
                        total_bytes += n as u64;
                    }
                    Err(e) => {
                        error!("Failed to read from client: {}", e);
                        break;
                    }
                }
            }

            total_bytes
        };

        let backend_to_client = async {
            let mut buffer = vec![0u8; 8192];
            let mut total_bytes = 0u64;

            loop {
                match backend_read.read(&mut buffer).await {
                    Ok(0) => break, // 连接关闭
                    Ok(n) => {
                        if let Err(e) = client_write.write_all(&buffer[..n]).await {
                            error!("Failed to write to client: {}", e);
                            break;
                        }
                        total_bytes += n as u64;
                    }
                    Err(e) => {
                        error!("Failed to read from backend: {}", e);
                        break;
                    }
                }
            }

            total_bytes
        };

        // 并发处理双向数据传输
        let (bytes_to_backend, bytes_to_client) =
            tokio::join!(client_to_backend, backend_to_client);

        // 更新连接信息
        connection_info.bytes_sent = bytes_to_backend;
        connection_info.bytes_received = bytes_to_client;

        debug!(
            "TCP proxy completed for connection {}: sent {} bytes, received {} bytes",
            connection_info.id, bytes_to_backend, bytes_to_client
        );

        Ok(())
    }

    /// 获取TCP代理状态
    pub async fn get_status(&self) -> TcpProxyStatus {
        let running = *self.running.read().await;
        let active_connections = {
            let connections = self.active_connections.read().await;
            connections.len()
        };

        TcpProxyStatus {
            running,
            bind_address: self.config.tcp_bind,
            active_connections,
        }
    }
}

/// TCP代理状态
#[derive(Debug, Clone)]
pub struct TcpProxyStatus {
    pub running: bool,
    pub bind_address: std::net::SocketAddr,
    pub active_connections: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HealthCheckConfig, MetricsConfig};
    use crate::health::HealthChecker;
    use crate::router::RouterManager;

    #[tokio::test]
    async fn test_tcp_proxy_creation() {
        let config = ServerConfig {
            tcp_bind: "127.0.0.1:0".parse().unwrap(),
            udp_bind: "127.0.0.1:0".parse().unwrap(),
            worker_threads: None,
            max_connections: Some(1000),
            connection_timeout: Some(30),
            buffer_size: Some(8192),
        };

        let router_manager = Arc::new(RouterManager::new());
        let _health_checker = Arc::new(HealthChecker::new(
            HealthCheckConfig::default(),
            router_manager.clone(),
        ));
        let metrics_collector = Arc::new(MetricsCollector::new(MetricsConfig::default()).unwrap());

        let tcp_proxy = TcpProxy::new(config, router_manager, metrics_collector);

        let status = tcp_proxy.get_status().await;
        assert!(!status.running);
        assert_eq!(status.active_connections, 0);
    }
}
