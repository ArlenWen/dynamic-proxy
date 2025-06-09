use crate::config::Settings;
use crate::monitoring::ProxyMetrics;
use crate::routing::{PluginManager, RoutingContext, RoutingAction};
use crate::shutdown::GracefulShutdown;
use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub struct UdpProxy {
    settings: Settings,
    metrics: Arc<ProxyMetrics>,
    plugin_manager: Arc<PluginManager>,
    client_sessions: Arc<RwLock<HashMap<SocketAddr, UdpSession>>>,
    shutdown_manager: Arc<GracefulShutdown>,
}

#[derive(Debug, Clone)]
pub struct UdpSession {
    session_id: String,
    backend_addr: SocketAddr,
    last_activity: Instant,
    bytes_sent: u64,
    bytes_received: u64,
}

impl UdpProxy {
    pub async fn new(
        settings: Settings,
        metrics: Arc<ProxyMetrics>,
        plugin_manager: Arc<PluginManager>,
        shutdown_manager: Arc<GracefulShutdown>,
    ) -> Result<Self> {
        Ok(Self {
            settings,
            metrics,
            plugin_manager,
            client_sessions: Arc::new(RwLock::new(HashMap::new())),
            shutdown_manager,
        })
    }

    pub async fn run(&self) -> Result<()> {
        let socket = UdpSocket::bind(&self.settings.server.udp_bind).await?;
        info!("UDP proxy listening on: {}", self.settings.server.udp_bind);

        // 启动会话清理任务
        self.start_session_cleanup().await;

        let mut buffer = vec![0u8; self.settings.server.buffer_size];

        let mut shutdown_rx = self.shutdown_manager.subscribe();

        loop {
            tokio::select! {
                // 处理UDP数据包
                result = socket.recv_from(&mut buffer) => {
                    match result {
                        Ok((len, client_addr)) => {
                            // 检查是否正在关机
                            if self.shutdown_manager.is_shutting_down().await {
                                debug!("Dropping UDP packet during shutdown");
                                continue;
                            }

                            let packet_data = buffer[..len].to_vec();

                            // 更新指标
                            self.metrics.udp_packets_total.inc();
                            self.metrics.bytes_transferred_total.inc_by(len as f64);

                            debug!("Received UDP packet from {}, {} bytes", client_addr, len);

                            // 处理数据包
                            let proxy = self.clone_for_packet();
                            let socket_addr = socket.local_addr().unwrap();

                            tokio::spawn(async move {
                                if let Err(e) = proxy.handle_packet(socket_addr, client_addr, packet_data).await {
                                    error!("Error handling UDP packet from {}: {}", client_addr, e);
                                    proxy.metrics.errors_total.inc();
                                }
                            });
                        }
                        Err(e) => {
                            error!("Failed to receive UDP packet: {}", e);
                            self.metrics.errors_total.inc();
                        }
                    }
                }
                // 监听关机信号
                _ = shutdown_rx.recv() => {
                    info!("UDP proxy received shutdown signal");
                    break;
                }
            }
        }

        info!("UDP proxy stopped accepting new packets");
        Ok(())
    }

    fn clone_for_packet(&self) -> Self {
        Self {
            settings: self.settings.clone(),
            metrics: Arc::clone(&self.metrics),
            plugin_manager: Arc::clone(&self.plugin_manager),
            client_sessions: Arc::clone(&self.client_sessions),
            shutdown_manager: Arc::clone(&self.shutdown_manager),
        }
    }

    async fn handle_packet(
        &self,
        local_addr: SocketAddr,
        client_addr: SocketAddr,
        packet_data: Vec<u8>,
    ) -> Result<()> {

        // 检查是否已有会话
        let existing_session = {
            let sessions = self.client_sessions.read().await;
            sessions.get(&client_addr).cloned()
        };

        let backend_addr = if let Some(mut session) = existing_session {
            // 更新会话活动时间
            session.last_activity = Instant::now();
            session.bytes_received += packet_data.len() as u64;
            
            {
                let mut sessions = self.client_sessions.write().await;
                sessions.insert(client_addr, session.clone());
            }
            
            session.backend_addr
        } else {
            // 创建新会话，需要路由决策
            let routing_context = RoutingContext {
                source_addr: client_addr,
                destination_addr: local_addr,
                protocol: "udp".to_string(),
                sni_hostname: None, // UDP不支持SNI
                connection_id: Uuid::new_v4().to_string(),
            };

            // 执行路由决策
            self.metrics.routing_decisions_total.inc();
            let routing_decision = match self.plugin_manager.route(&routing_context).await? {
                Some(decision) => decision,
                None => {
                    warn!("No routing decision made for UDP packet from {}", client_addr);
                    return Ok(());
                }
            };

            let backend_addr = match routing_decision.action {
                RoutingAction::Forward { backend: _ } => {
                    routing_decision.backend_addr.ok_or_else(|| {
                        anyhow::anyhow!("Backend address not resolved")
                    })?
                }
                RoutingAction::LoadBalance { backends: _, method: _ } => {
                    // 简化实现：选择第一个后端
                    routing_decision.backend_addr.ok_or_else(|| {
                        anyhow::anyhow!("No backend address for load balancing")
                    })?
                }
                RoutingAction::Reject { reason } => {
                    info!("UDP packet from {} rejected: {}", client_addr, reason);
                    return Ok(());
                }
                RoutingAction::TlsTerminate { backend: _ } => {
                    warn!("TLS termination not supported for UDP");
                    return Ok(());
                }
            };

            // 创建新会话
            let session = UdpSession {
                session_id: Uuid::new_v4().to_string(),
                backend_addr,
                last_activity: Instant::now(),
                bytes_sent: packet_data.len() as u64,
                bytes_received: 0,
            };

            {
                let mut sessions = self.client_sessions.write().await;
                sessions.insert(client_addr, session.clone());
            }

            debug!("Created new UDP session for {} -> {}", client_addr, backend_addr);
            backend_addr
        };

        // 转发数据包到后端
        self.forward_to_backend(local_addr, client_addr, backend_addr, packet_data).await?;

        Ok(())
    }

    async fn forward_to_backend(
        &self,
        local_addr: SocketAddr,
        client_addr: SocketAddr,
        backend_addr: SocketAddr,
        packet_data: Vec<u8>,
    ) -> Result<()> {
        // 创建客户端socket用于回复
        let client_socket = UdpSocket::bind(local_addr).await?;
        // 创建到后端的socket
        let backend_socket = UdpSocket::bind("0.0.0.0:0").await?;
        
        // 发送数据到后端
        let backend_start = Instant::now();
        backend_socket.send_to(&packet_data, backend_addr).await?;
        
        debug!("Forwarded {} bytes from {} to backend {}", 
               packet_data.len(), client_addr, backend_addr);

        // 等待后端响应
        let mut response_buffer = vec![0u8; self.settings.server.buffer_size];
        
        match tokio::time::timeout(
            Duration::from_millis(self.settings.redis.timeout_ms),
            backend_socket.recv(&mut response_buffer)
        ).await {
            Ok(Ok(response_len)) => {
                self.metrics.backend_response_time.observe(backend_start.elapsed().as_secs_f64());
                
                // 发送响应回客户端
                client_socket.send_to(&response_buffer[..response_len], client_addr).await?;
                
                // 更新指标
                self.metrics.bytes_transferred_total.inc_by(response_len as f64);
                self.metrics.requests_total.inc();
                
                // 更新会话统计
                {
                    let mut sessions = self.client_sessions.write().await;
                    if let Some(session) = sessions.get_mut(&client_addr) {
                        session.bytes_sent += response_len as u64;
                        session.last_activity = Instant::now();
                    }
                }
                
                debug!("Forwarded {} bytes response from backend {} to client {}", 
                       response_len, backend_addr, client_addr);
            }
            Ok(Err(e)) => {
                error!("Error receiving from backend {}: {}", backend_addr, e);
                return Err(e.into());
            }
            Err(_) => {
                warn!("Timeout waiting for response from backend {}", backend_addr);
                // 不返回错误，UDP是无连接的
            }
        }

        Ok(())
    }

    async fn start_session_cleanup(&self) {
        let sessions = Arc::clone(&self.client_sessions);
        let session_timeout = Duration::from_secs(300); // 5分钟超时
        let mut shutdown_rx = self.shutdown_manager.subscribe();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60)); // 每分钟清理一次

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let now = Instant::now();
                        let mut sessions_to_remove = Vec::new();

                        {
                            let sessions_read = sessions.read().await;
                            for (client_addr, session) in sessions_read.iter() {
                                if now.duration_since(session.last_activity) > session_timeout {
                                    sessions_to_remove.push(*client_addr);
                                }
                            }
                        }

                        if !sessions_to_remove.is_empty() {
                            let mut sessions_write = sessions.write().await;
                            for client_addr in sessions_to_remove {
                                if let Some(session) = sessions_write.remove(&client_addr) {
                                    debug!("Cleaned up expired UDP session: {} ({})",
                                           client_addr, session.session_id);
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("UDP session cleanup task received shutdown signal");
                        break;
                    }
                }
            }

            info!("UDP session cleanup task stopped");
        });
    }

    pub async fn get_session_stats(&self) -> HashMap<SocketAddr, UdpSession> {
        self.client_sessions.read().await.clone()
    }

    pub async fn get_active_sessions_count(&self) -> usize {
        self.client_sessions.read().await.len()
    }
}
