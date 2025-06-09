use dynamic_proxy::{config::Settings, proxy::ProxyServer};
use tokio::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    println!("Dynamic Proxy Server Example");
    println!("============================");

    // 创建自定义配置
    let mut settings = Settings::default();
    
    // 修改端口以避免冲突
    settings.server.tcp_bind = "127.0.0.1:18080".to_string();
    settings.server.udp_bind = "127.0.0.1:18081".to_string();
    settings.monitoring.metrics_bind = "127.0.0.1:19090".to_string();
    
    // 禁用TLS以简化示例
    settings.tls = None;

    println!("Configuration:");
    println!("  TCP Bind: {}", settings.server.tcp_bind);
    println!("  UDP Bind: {}", settings.server.udp_bind);
    println!("  Metrics: {}", settings.monitoring.metrics_bind);
    println!();

    // 创建代理服务器
    let mut proxy_server = ProxyServer::new(settings).await?;
    
    println!("Starting proxy server...");
    
    // 在后台启动服务器
    let server_handle = tokio::spawn(async move {
        if let Err(e) = proxy_server.start().await {
            eprintln!("Proxy server error: {}", e);
        }
    });

    // 等待一段时间让服务器启动
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    println!("Proxy server is running!");
    println!("You can now:");
    println!("  - Connect to TCP proxy at 127.0.0.1:18080");
    println!("  - Send UDP packets to 127.0.0.1:18081");
    println!("  - View metrics at http://127.0.0.1:19090/metrics");
    println!();
    println!("Press Ctrl+C to stop the server");

    // 等待用户中断
    tokio::signal::ctrl_c().await?;
    
    println!("\nShutting down...");
    server_handle.abort();
    
    Ok(())
}
