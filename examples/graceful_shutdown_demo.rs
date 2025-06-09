use anyhow::Result;
use dynamic_proxy::config::Settings;
use dynamic_proxy::proxy::ProxyServer;
use dynamic_proxy::shutdown::{GracefulShutdown, ShutdownSignal};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn, error};

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter("dynamic_proxy=debug,graceful_shutdown_demo=info")
        .init();

    info!("Graceful Shutdown Demo");
    info!("=====================");
    info!("");
    info!("This demo shows how the proxy handles graceful shutdown");
    info!("You can test different shutdown signals:");
    info!("  - Ctrl+C (SIGINT): Graceful shutdown");
    info!("  - kill -TERM <pid>: Graceful shutdown");
    info!("  - kill -QUIT <pid>: Force shutdown");
    info!("  - kill -HUP <pid>: Configuration reload (graceful shutdown)");
    info!("");

    // 创建自定义配置
    let mut settings = Settings::default();
    
    // 修改端口以避免冲突
    settings.server.tcp_bind = "127.0.0.1:28080".to_string();
    settings.server.udp_bind = "127.0.0.1:28081".to_string();
    settings.monitoring.metrics_bind = "127.0.0.1:29090".to_string();
    
    // 禁用TLS以简化示例
    settings.tls = None;

    info!("Configuration:");
    info!("  TCP Bind: {}", settings.server.tcp_bind);
    info!("  UDP Bind: {}", settings.server.udp_bind);
    info!("  Metrics: {}", settings.monitoring.metrics_bind);
    info!("  Process ID: {}", std::process::id());
    info!("");

    // 创建代理服务器
    let mut proxy_server = ProxyServer::new(settings).await?;
    
    // 获取关机管理器的引用
    let shutdown_manager = proxy_server.shutdown_manager().clone();
    
    // 启动状态监控任务
    start_status_monitor(shutdown_manager.clone()).await;
    
    // 启动模拟客户端任务
    start_mock_clients().await;
    
    info!("Starting proxy server...");
    info!("The server will demonstrate graceful shutdown handling");
    info!("");
    info!("Try sending different signals:");
    info!("  kill -INT {}   # Graceful shutdown", std::process::id());
    info!("  kill -TERM {}  # Graceful shutdown", std::process::id());
    info!("  kill -QUIT {}  # Force shutdown", std::process::id());
    info!("  kill -HUP {}   # Configuration reload", std::process::id());
    info!("");
    
    // 启动服务器（这会阻塞直到收到关机信号）
    if let Err(e) = proxy_server.start().await {
        error!("Proxy server error: {}", e);
        return Err(e);
    }

    info!("Graceful shutdown demo completed successfully!");
    Ok(())
}

/// 启动状态监控任务
async fn start_status_monitor(shutdown_manager: Arc<GracefulShutdown>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        
        loop {
            interval.tick().await;
            
            let state = shutdown_manager.get_state().await;
            let active_connections = shutdown_manager.get_active_connections().await;
            
            info!("Status: State={:?}, Active Connections={}", state, active_connections);
            
            // 如果已经关闭，退出监控
            if matches!(state, dynamic_proxy::shutdown::ShutdownState::Shutdown) {
                break;
            }
        }
        
        info!("Status monitor stopped");
    });
}

/// 启动模拟客户端任务
async fn start_mock_clients() {
    tokio::spawn(async move {
        info!("Starting mock clients to simulate load...");
        
        // 等待服务器启动
        sleep(Duration::from_secs(2)).await;
        
        // 模拟一些TCP连接
        for i in 0..5 {
            let client_id = i;
            tokio::spawn(async move {
                if let Err(e) = simulate_tcp_client(client_id).await {
                    warn!("Mock TCP client {} error: {}", client_id, e);
                }
            });
            
            // 间隔启动客户端
            sleep(Duration::from_millis(500)).await;
        }
        
        // 模拟一些UDP数据包
        for i in 0..3 {
            let client_id = i;
            tokio::spawn(async move {
                if let Err(e) = simulate_udp_client(client_id).await {
                    warn!("Mock UDP client {} error: {}", client_id, e);
                }
            });
            
            sleep(Duration::from_millis(300)).await;
        }
        
        info!("Mock clients started");
    });
}

/// 模拟TCP客户端
async fn simulate_tcp_client(client_id: usize) -> Result<()> {
    use tokio::net::TcpStream;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    
    // 连接到代理
    let mut stream = TcpStream::connect("127.0.0.1:28080").await?;
    info!("Mock TCP client {} connected", client_id);
    
    // 发送一些数据
    for i in 0..10 {
        let message = format!("Hello from TCP client {} - message {}\n", client_id, i);
        stream.write_all(message.as_bytes()).await?;
        
        // 尝试读取响应（可能会失败，因为没有真实的后端）
        let mut buffer = [0; 1024];
        let _ = tokio::time::timeout(
            Duration::from_millis(100),
            stream.read(&mut buffer)
        ).await;
        
        // 间隔发送
        sleep(Duration::from_secs(2)).await;
    }
    
    info!("Mock TCP client {} finished", client_id);
    Ok(())
}

/// 模拟UDP客户端
async fn simulate_udp_client(client_id: usize) -> Result<()> {
    use tokio::net::UdpSocket;
    
    // 创建UDP socket
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    info!("Mock UDP client {} started", client_id);
    
    // 发送一些UDP数据包
    for i in 0..5 {
        let message = format!("Hello from UDP client {} - packet {}", client_id, i);
        socket.send_to(message.as_bytes(), "127.0.0.1:28081").await?;
        
        // 尝试接收响应
        let mut buffer = [0; 1024];
        let _ = tokio::time::timeout(
            Duration::from_millis(100),
            socket.recv(&mut buffer)
        ).await;
        
        // 间隔发送
        sleep(Duration::from_secs(3)).await;
    }
    
    info!("Mock UDP client {} finished", client_id);
    Ok(())
}

/// 演示手动触发关机
#[allow(dead_code)]
async fn demonstrate_manual_shutdown(proxy_server: &ProxyServer) -> Result<()> {
    info!("Demonstrating manual shutdown trigger...");
    
    // 等待一段时间
    sleep(Duration::from_secs(10)).await;
    
    // 手动触发关机
    proxy_server.trigger_shutdown(ShutdownSignal::Internal).await?;
    
    info!("Manual shutdown triggered");
    Ok(())
}

/// 演示关机状态检查
#[allow(dead_code)]
async fn demonstrate_shutdown_status_check(proxy_server: &ProxyServer) {
    loop {
        let is_shutting_down = proxy_server.is_shutting_down().await;
        let state = proxy_server.get_shutdown_state().await;
        
        info!("Shutdown status: is_shutting_down={}, state={:?}", is_shutting_down, state);
        
        if matches!(state, dynamic_proxy::shutdown::ShutdownState::Shutdown) {
            break;
        }
        
        sleep(Duration::from_secs(1)).await;
    }
}
