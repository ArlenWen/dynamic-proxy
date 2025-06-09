use crate::config::Settings;
use crate::monitoring::ProxyMetrics;
use crate::routing::{PluginManager, RoutingContext, RoutingAction};
use crate::proxy::tls_handler::TlsHandler;
use crate::shutdown::GracefulShutdown;
use anyhow::Result;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::copy_bidirectional;
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub struct TcpProxy {
    settings: Settings,
    metrics: Arc<ProxyMetrics>,
    plugin_manager: Arc<PluginManager>,
    tls_handler: Option<TlsHandler>,
    shutdown_manager: Arc<GracefulShutdown>,
}

impl TcpProxy {
    pub async fn new(
        settings: Settings,
        metrics: Arc<ProxyMetrics>,
        plugin_manager: Arc<PluginManager>,
        shutdown_manager: Arc<GracefulShutdown>,
    ) -> Result<Self> {
        let tls_handler = if let Some(tls_config) = &settings.tls {
            Some(TlsHandler::new(
                &tls_config.cert_path,
                &tls_config.key_path,
                tls_config.sni_routing,
            ).await?)
        } else {
            None
        };

        Ok(Self {
            settings,
            metrics,
            plugin_manager,
            tls_handler,
            shutdown_manager,
        })
    }

    pub async fn run(&self) -> Result<()> {
        let listener = TcpListener::bind(&self.settings.server.tcp_bind).await?;
        info!("TCP proxy listening on: {}", self.settings.server.tcp_bind);

        let mut shutdown_rx = self.shutdown_manager.subscribe();

        loop {
            tokio::select! {
                // 处理新连接
                result = listener.accept() => {
                    match result {
                        Ok((stream, peer_addr)) => {
                            // 检查是否正在关机
                            if self.shutdown_manager.is_shutting_down().await {
                                debug!("Rejecting new connection during shutdown");
                                continue;
                            }

                            let connection_id = Uuid::new_v4().to_string();
                            debug!("New TCP connection from {}, ID: {}", peer_addr, connection_id);

                            // 更新连接指标和计数
                            self.metrics.tcp_connections_total.inc();
                            self.metrics.active_connections.inc();
                            self.shutdown_manager.add_connection().await;

                            // 处理连接
                            let proxy = self.clone_for_connection();
                            tokio::spawn(async move {
                                let start_time = Instant::now();

                                if let Err(e) = proxy.handle_connection(stream, peer_addr, connection_id.clone()).await {
                                    error!("Error handling TCP connection {}: {}", connection_id, e);
                                    proxy.metrics.errors_total.inc();
                                }

                                // 更新指标和连接计数
                                proxy.metrics.active_connections.dec();
                                proxy.metrics.request_duration.observe(start_time.elapsed().as_secs_f64());
                                proxy.shutdown_manager.remove_connection().await;
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept TCP connection: {}", e);
                            self.metrics.errors_total.inc();
                        }
                    }
                }
                // 监听关机信号
                _ = shutdown_rx.recv() => {
                    info!("TCP proxy received shutdown signal");
                    break;
                }
            }
        }

        info!("TCP proxy stopped accepting new connections");
        Ok(())
    }

    fn clone_for_connection(&self) -> Self {
        Self {
            settings: self.settings.clone(),
            metrics: Arc::clone(&self.metrics),
            plugin_manager: Arc::clone(&self.plugin_manager),
            tls_handler: None, // TLS handler不需要克隆，因为它是无状态的
            shutdown_manager: Arc::clone(&self.shutdown_manager),
        }
    }

    async fn handle_connection(
        &self,
        stream: TcpStream,
        peer_addr: std::net::SocketAddr,
        connection_id: String,
    ) -> Result<()> {
        let local_addr = stream.local_addr()?;
        
        // 简化实现：暂时不检测SNI，直接使用普通TCP转发
        let sni_hostname = None;
        let is_tls = false;

        // 创建路由上下文
        let routing_context = RoutingContext {
            source_addr: peer_addr,
            destination_addr: local_addr,
            protocol: "tcp".to_string(),
            sni_hostname: sni_hostname.clone(),
            connection_id: connection_id.clone(),
        };

        // 执行路由决策
        self.metrics.routing_decisions_total.inc();
        let routing_decision = match self.plugin_manager.route(&routing_context).await? {
            Some(decision) => decision,
            None => {
                warn!("No routing decision made for connection {}", connection_id);
                return Ok(());
            }
        };

        debug!("Routing decision for {}: {:?}", connection_id, routing_decision);

        // 根据路由决策处理连接
        match routing_decision.action {
            RoutingAction::Forward { backend } => {
                self.forward_connection(stream, &backend, routing_decision.backend_addr, is_tls).await?;
            }
            RoutingAction::LoadBalance { backends, method: _ } => {
                // 简化实现：选择第一个后端
                if let Some(backend) = backends.first() {
                    self.forward_connection(stream, backend, routing_decision.backend_addr, is_tls).await?;
                } else {
                    warn!("No backends available for load balancing");
                }
            }
            RoutingAction::Reject { reason } => {
                info!("Connection {} rejected: {}", connection_id, reason);
                return Ok(());
            }
            RoutingAction::TlsTerminate { backend } => {
                self.handle_tls_termination(stream, &backend, routing_decision.backend_addr).await?;
            }
        }

        Ok(())
    }

    async fn forward_connection(
        &self,
        mut client_stream: TcpStream,
        backend_name: &str,
        backend_addr: Option<std::net::SocketAddr>,
        _is_tls: bool,
    ) -> Result<()> {
        let backend_addr = backend_addr.ok_or_else(|| {
            anyhow::anyhow!("Backend address not resolved for: {}", backend_name)
        })?;

        debug!("Forwarding connection to backend: {} ({})", backend_name, backend_addr);

        // 连接到后端服务器
        let backend_start = Instant::now();
        let mut backend_stream = match tokio::time::timeout(
            Duration::from_millis(self.settings.redis.timeout_ms),
            TcpStream::connect(backend_addr)
        ).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => {
                error!("Failed to connect to backend {}: {}", backend_addr, e);
                return Err(e.into());
            }
            Err(_) => {
                error!("Timeout connecting to backend {}", backend_addr);
                return Err(anyhow::anyhow!("Backend connection timeout"));
            }
        };

        self.metrics.backend_response_time.observe(backend_start.elapsed().as_secs_f64());

        // 双向数据转发
        let (client_to_backend, backend_to_client) = copy_bidirectional(&mut client_stream, &mut backend_stream).await?;
        
        // 更新流量指标
        self.metrics.bytes_transferred_total.inc_by(client_to_backend as f64);
        self.metrics.bytes_transferred_total.inc_by(backend_to_client as f64);
        self.metrics.requests_total.inc();

        debug!("Connection forwarded {} bytes (client->backend), {} bytes (backend->client)", 
               client_to_backend, backend_to_client);

        Ok(())
    }

    async fn handle_tls_termination(
        &self,
        stream: TcpStream,
        backend_name: &str,
        backend_addr: Option<std::net::SocketAddr>,
    ) -> Result<()> {
        if let Some(tls_handler) = &self.tls_handler {
            self.metrics.tls_handshakes_total.inc();
            
            match tls_handler.handle_connection(stream).await? {
                Some((tls_stream, sni)) => {
                    debug!("TLS termination successful, SNI: {:?}", sni);
                    
                    // 将解密后的流量转发到后端
                    let backend_addr = backend_addr.ok_or_else(|| {
                        anyhow::anyhow!("Backend address not resolved for: {}", backend_name)
                    })?;

                    let mut backend_stream = TcpStream::connect(backend_addr).await?;
                    let mut tls_stream = tls_stream;
                    
                    let (client_to_backend, backend_to_client) = copy_bidirectional(&mut tls_stream, &mut backend_stream).await?;
                    
                    self.metrics.bytes_transferred_total.inc_by(client_to_backend as f64);
                    self.metrics.bytes_transferred_total.inc_by(backend_to_client as f64);
                    self.metrics.requests_total.inc();
                }
                None => {
                    warn!("TLS not configured properly for termination");
                }
            }
        } else {
            self.metrics.tls_errors_total.inc();
            return Err(anyhow::anyhow!("TLS termination requested but TLS not configured"));
        }

        Ok(())
    }
}
