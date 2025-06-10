use super::{ProxyError, ProxyResult};
use crate::config::ServerConfig;
use crate::metrics::MetricsCollector;
use crate::router::{Protocol, RouterManager, RoutingContext};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// UDP代理服务器
pub struct UdpProxy {
    config: ServerConfig,
    router_manager: Arc<RouterManager>,
    metrics_collector: Arc<MetricsCollector>,
    #[allow(dead_code)]
    socket: Option<Arc<UdpSocket>>,
    running: Arc<RwLock<bool>>,
    active_sessions: Arc<RwLock<HashMap<SocketAddr, UdpSession>>>,
}

/// UDP会话信息
#[derive(Debug, Clone)]
struct UdpSession {
    #[allow(dead_code)]
    client_addr: SocketAddr,
    backend_addr: SocketAddr,
    backend_socket: Arc<UdpSocket>,
    last_activity: Instant,
    bytes_sent: u64,
    bytes_received: u64,
    rule_name: Option<String>,
    #[allow(dead_code)]
    router_name: Option<String>,
}

impl UdpProxy {
    pub fn new(
        config: ServerConfig,
        router_manager: Arc<RouterManager>,
        metrics_collector: Arc<MetricsCollector>,
    ) -> Self {
        Self {
            config,
            router_manager,
            metrics_collector,
            socket: None,
            running: Arc::new(RwLock::new(false)),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 启动UDP代理
    pub async fn start(&self) -> Result<()> {
        // 检查是否配置了UDP绑定地址
        let udp_bind = match &self.config.udp_bind {
            Some(addr) => *addr,
            None => {
                warn!("UDP bind address not configured, skipping UDP proxy startup");
                return Ok(());
            }
        };

        {
            let mut running = self.running.write().await;
            if *running {
                warn!("UDP proxy is already running");
                return Ok(());
            }
            *running = true;
        }

        info!("Starting UDP proxy on {}", udp_bind);

        let socket = UdpSocket::bind(udp_bind)
            .await
            .with_context(|| format!("Failed to bind UDP socket to {}", udp_bind))?;

        let socket = Arc::new(socket);
        info!("UDP proxy listening on {}", udp_bind);

        let router_manager = self.router_manager.clone();
        let metrics_collector = self.metrics_collector.clone();
        let running = self.running.clone();
        let active_sessions = self.active_sessions.clone();
        let config = self.config.clone();

        // 启动主接收循环
        let socket_clone = socket.clone();
        tokio::spawn(async move {
            Self::receive_loop(
                socket_clone,
                router_manager,
                metrics_collector,
                running,
                active_sessions,
                config,
            )
            .await;
        });

        // 启动会话清理任务
        let active_sessions_cleanup = self.active_sessions.clone();
        let running_cleanup = self.running.clone();
        tokio::spawn(async move {
            Self::cleanup_sessions(active_sessions_cleanup, running_cleanup).await;
        });

        Ok(())
    }

    /// 停止UDP代理
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping UDP proxy...");

        {
            let mut running = self.running.write().await;
            *running = false;
        }

        // 清理所有会话
        {
            let mut sessions = self.active_sessions.write().await;
            sessions.clear();
        }

        info!("UDP proxy stopped");
        Ok(())
    }

    /// 主接收循环
    async fn receive_loop(
        socket: Arc<UdpSocket>,
        router_manager: Arc<RouterManager>,
        metrics_collector: Arc<MetricsCollector>,
        running: Arc<RwLock<bool>>,
        active_sessions: Arc<RwLock<HashMap<SocketAddr, UdpSession>>>,
        config: ServerConfig,
    ) {
        let mut buffer = vec![0u8; config.buffer_size.unwrap_or(8192)];

        loop {
            // 检查是否应该停止
            {
                let running_guard = running.read().await;
                if !*running_guard {
                    break;
                }
            }

            // 接收数据包
            match socket.recv_from(&mut buffer).await {
                Ok((len, client_addr)) => {
                    debug!("Received UDP packet from {} ({} bytes)", client_addr, len);

                    let data = buffer[..len].to_vec();
                    let socket = socket.clone();
                    let router_manager = router_manager.clone();
                    let metrics_collector = metrics_collector.clone();
                    let active_sessions = active_sessions.clone();
                    let config = config.clone();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_packet(
                            socket,
                            client_addr,
                            data,
                            router_manager,
                            metrics_collector,
                            active_sessions,
                            config,
                        )
                        .await
                        {
                            error!("Failed to handle UDP packet from {}: {}", client_addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to receive UDP packet: {}", e);
                    // 短暂延迟以避免忙循环
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }

        info!("UDP proxy stopped receiving packets");
    }

    /// 处理单个数据包
    async fn handle_packet(
        socket: Arc<UdpSocket>,
        client_addr: SocketAddr,
        data: Vec<u8>,
        router_manager: Arc<RouterManager>,
        metrics_collector: Arc<MetricsCollector>,
        active_sessions: Arc<RwLock<HashMap<SocketAddr, UdpSession>>>,
        config: ServerConfig,
    ) -> ProxyResult<()> {
        // 检查是否已有会话
        let existing_session = {
            let sessions = active_sessions.read().await;
            sessions.get(&client_addr).cloned()
        };

        let session = if let Some(mut session) = existing_session {
            // 更新现有会话
            session.last_activity = Instant::now();
            session.bytes_sent += data.len() as u64;

            // 转发到后端
            if let Err(e) = session
                .backend_socket
                .send_to(&data, session.backend_addr)
                .await
            {
                error!(
                    "Failed to send UDP packet to backend {}: {}",
                    session.backend_addr, e
                );
                return Err(ProxyError::BackendConnectionFailed(e));
            }

            debug!(
                "Forwarded UDP packet from {} to backend {} ({} bytes)",
                client_addr,
                session.backend_addr,
                data.len()
            );

            // 更新会话
            {
                let mut sessions = active_sessions.write().await;
                sessions.insert(client_addr, session.clone());
            }

            session
        } else {
            // 创建新会话
            let session = Self::create_new_session(
                socket,
                client_addr,
                data,
                router_manager,
                active_sessions,
                config,
            )
            .await?;

            debug!(
                "Created new UDP session for {} -> {}",
                client_addr, session.backend_addr
            );

            session
        };

        // 记录指标
        if let Some(rule_name) = &session.rule_name {
            let metrics = crate::metrics::RequestMetrics::new(
                "udp",
                rule_name,
                &session.backend_addr.to_string(),
                "success",
                Duration::from_millis(1), // UDP是无连接的，使用固定值
                session.bytes_sent,
                session.bytes_received,
            );
            metrics_collector.record_request(&metrics);
        }

        Ok(())
    }

    /// 创建新的UDP会话
    async fn create_new_session(
        client_socket: Arc<UdpSocket>,
        client_addr: SocketAddr,
        data: Vec<u8>,
        router_manager: Arc<RouterManager>,
        active_sessions: Arc<RwLock<HashMap<SocketAddr, UdpSession>>>,
        config: ServerConfig,
    ) -> ProxyResult<UdpSession> {
        // 创建路由上下文
        let target_addr = config.udp_bind.unwrap_or_else(|| "0.0.0.0:0".parse().unwrap());
        let routing_context = RoutingContext::new(
            client_addr,
            target_addr, // 这里应该是实际的目标地址
            Protocol::Udp,
        );

        // 路由到后端
        let backend = router_manager
            .route(&routing_context)
            .await
            .map_err(|e| ProxyError::RoutingFailed(e.to_string()))?
            .ok_or(ProxyError::NoBackendAvailable)?;

        debug!(
            "Routing UDP session from {} to backend {}",
            client_addr, backend.address
        );

        // 创建后端socket
        let backend_socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(ProxyError::BackendConnectionFailed)?;
        let backend_socket = Arc::new(backend_socket);

        // 发送初始数据包到后端
        backend_socket
            .send_to(&data, backend.address)
            .await
            .map_err(ProxyError::BackendConnectionFailed)?;

        // 创建会话
        let session = UdpSession {
            client_addr,
            backend_addr: backend.address,
            backend_socket: backend_socket.clone(),
            last_activity: Instant::now(),
            bytes_sent: data.len() as u64,
            bytes_received: 0,
            rule_name: None,   // 需要从路由结果获取
            router_name: None, // 需要从路由结果获取
        };

        // 启动后端响应监听
        let client_socket_clone = client_socket.clone();
        let active_sessions_clone = active_sessions.clone();
        let session_clone = session.clone();
        tokio::spawn(async move {
            Self::listen_backend_response(
                client_socket_clone,
                backend_socket,
                client_addr,
                active_sessions_clone,
                session_clone,
            )
            .await;
        });

        // 添加到活跃会话
        {
            let mut sessions = active_sessions.write().await;
            sessions.insert(client_addr, session.clone());
        }

        Ok(session)
    }

    /// 监听后端响应
    async fn listen_backend_response(
        client_socket: Arc<UdpSocket>,
        backend_socket: Arc<UdpSocket>,
        client_addr: SocketAddr,
        active_sessions: Arc<RwLock<HashMap<SocketAddr, UdpSession>>>,
        mut session: UdpSession,
    ) {
        let mut buffer = vec![0u8; 8192];
        let timeout_duration = Duration::from_secs(30); // 30秒超时

        loop {
            match timeout(timeout_duration, backend_socket.recv(&mut buffer)).await {
                Ok(Ok(len)) => {
                    debug!(
                        "Received UDP response from backend {} ({} bytes)",
                        session.backend_addr, len
                    );

                    // 转发响应到客户端
                    if let Err(e) = client_socket.send_to(&buffer[..len], client_addr).await {
                        error!(
                            "Failed to send UDP response to client {}: {}",
                            client_addr, e
                        );
                        break;
                    }

                    // 更新会话
                    session.last_activity = Instant::now();
                    session.bytes_received += len as u64;

                    {
                        let mut sessions = active_sessions.write().await;
                        sessions.insert(client_addr, session.clone());
                    }

                    debug!(
                        "Forwarded UDP response to client {} ({} bytes)",
                        client_addr, len
                    );
                }
                Ok(Err(e)) => {
                    error!(
                        "Failed to receive from backend {}: {}",
                        session.backend_addr, e
                    );
                    break;
                }
                Err(_) => {
                    debug!("Backend response timeout for session {}", client_addr);
                    break;
                }
            }
        }

        // 清理会话
        {
            let mut sessions = active_sessions.write().await;
            sessions.remove(&client_addr);
        }

        debug!(
            "UDP session {} -> {} ended",
            client_addr, session.backend_addr
        );
    }

    /// 清理过期会话
    async fn cleanup_sessions(
        active_sessions: Arc<RwLock<HashMap<SocketAddr, UdpSession>>>,
        running: Arc<RwLock<bool>>,
    ) {
        let cleanup_interval = Duration::from_secs(60); // 每分钟清理一次
        let session_timeout = Duration::from_secs(300); // 5分钟超时

        loop {
            tokio::time::sleep(cleanup_interval).await;

            // 检查是否应该停止
            {
                let running_guard = running.read().await;
                if !*running_guard {
                    break;
                }
            }

            let now = Instant::now();
            let mut expired_sessions = Vec::new();

            // 查找过期会话
            {
                let sessions = active_sessions.read().await;
                for (client_addr, session) in sessions.iter() {
                    if now.duration_since(session.last_activity) > session_timeout {
                        expired_sessions.push(*client_addr);
                    }
                }
            }

            // 移除过期会话
            if !expired_sessions.is_empty() {
                let mut sessions = active_sessions.write().await;
                for client_addr in expired_sessions {
                    sessions.remove(&client_addr);
                    debug!("Cleaned up expired UDP session for {}", client_addr);
                }
            }
        }

        info!("UDP session cleanup task stopped");
    }

    /// 获取UDP代理状态
    pub async fn get_status(&self) -> UdpProxyStatus {
        let running = *self.running.read().await;
        let active_sessions = {
            let sessions = self.active_sessions.read().await;
            sessions.len()
        };

        UdpProxyStatus {
            running,
            bind_address: self.config.udp_bind,
            active_sessions,
        }
    }
}

/// UDP代理状态
#[derive(Debug, Clone)]
pub struct UdpProxyStatus {
    pub running: bool,
    pub bind_address: Option<SocketAddr>,
    pub active_sessions: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HealthCheckConfig, MetricsConfig};
    use crate::health::HealthChecker;
    use crate::router::RouterManager;

    #[tokio::test]
    async fn test_udp_proxy_creation() {
        let config = ServerConfig {
            tcp_bind: Some("127.0.0.1:0".parse().unwrap()),
            udp_bind: Some("127.0.0.1:0".parse().unwrap()),
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

        let udp_proxy = UdpProxy::new(config, router_manager, metrics_collector);

        let status = udp_proxy.get_status().await;
        assert!(!status.running);
        assert_eq!(status.active_sessions, 0);
    }
}
