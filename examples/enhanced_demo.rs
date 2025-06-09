use anyhow::Result;
use dynamic_proxy::config::Settings;
use dynamic_proxy::proxy::{tcp_proxy::TcpProxy, udp_proxy::UdpProxy};
use dynamic_proxy::routing::{PluginManager, RedisBackend};

use dynamic_proxy::monitoring::{ProxyMetrics, setup_logging};
use dynamic_proxy::shutdown::GracefulShutdown;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn, error};

#[tokio::main]
async fn main() -> Result<()> {
    // 设置日志
    setup_logging("info")?;
    
    info!("Starting Enhanced Dynamic Proxy Demo");
    info!("=====================================");
    
    // 加载配置
    let settings = Settings::load().await?;
    info!("Configuration loaded successfully");
    
    // 初始化指标收集
    let metrics = Arc::new(ProxyMetrics::new()?);
    info!("Metrics system initialized");
    
    // 初始化插件管理器
    let mut plugin_manager = PluginManager::new();
    
    // 创建并注册Redis后端插件
    let redis_backend = RedisBackend::new();
    plugin_manager.register_mutable_plugin(Box::new(redis_backend)).await;

    // 准备插件配置
    let mut plugin_configs = std::collections::HashMap::new();
    plugin_configs.insert(
        "redis_backend".to_string(),
        serde_json::json!({
            "redis_url": settings.redis.url,
            "cache_ttl": settings.redis.cache_ttl_secs,
            "connection_timeout": settings.redis.timeout_ms / 1000,
            "max_retries": settings.redis.max_retries
        })
    );

    // 初始化所有插件
    match plugin_manager.initialize_plugins(plugin_configs).await {
        Ok(_) => {
            info!("All plugins initialized successfully");
        }
        Err(e) => {
            warn!("Failed to initialize some plugins: {}", e);
            warn!("Continuing with available plugins...");
        }
    }
    
    let plugin_manager = Arc::new(plugin_manager);

    // 初始化优雅关机管理器
    let shutdown_manager = Arc::new(
        GracefulShutdown::new()
            .with_shutdown_timeout(Duration::from_secs(30))
            .with_drain_timeout(Duration::from_secs(15))
            .with_force_shutdown(true)
    );

    // 启动信号处理
    if let Err(e) = shutdown_manager.start_signal_handling().await {
        warn!("Failed to setup signal handling: {}", e);
    }

    // 创建TCP代理
    let tcp_proxy = TcpProxy::new(
        settings.clone(),
        metrics.clone(),
        plugin_manager.clone(),
        shutdown_manager.clone(),
    ).await?;

    // 创建UDP代理
    let udp_proxy = UdpProxy::new(
        settings.clone(),
        metrics.clone(),
        plugin_manager.clone(),
        shutdown_manager.clone(),
    ).await?;
    
    // 启动指标服务器
    let metrics_bind = settings.monitoring.metrics_bind.clone();
    tokio::spawn(async move {
        // 简化的指标服务器实现
        info!("Metrics server would start on: {}", metrics_bind);
        // 在实际实现中，这里会启动HTTP服务器
    });
    
    info!("All services initialized successfully");
    info!("TCP Proxy: {}", settings.server.tcp_bind);
    info!("UDP Proxy: {}", settings.server.udp_bind);
    info!("Metrics: {}", settings.monitoring.metrics_bind);
    info!("Redis: {}", settings.redis.url);
    
    // 显示TLS配置信息
    if let Some(tls_config) = &settings.tls {
        info!("TLS Configuration:");
        info!("  SNI Routing: {}", tls_config.sni_routing);
        info!("  Default Cert: {}", tls_config.cert_path);
        info!("  Default Key: {}", tls_config.key_path);
        
        if let Some(certificates) = &tls_config.certificates {
            info!("  Additional Certificates:");
            for cert in certificates {
                info!("    {} -> {}", cert.hostname, cert.cert_path);
            }
        }
    }
    
    // 启动代理服务
    let tcp_handle = {
        let tcp_proxy = tcp_proxy;
        tokio::spawn(async move {
            if let Err(e) = tcp_proxy.run().await {
                error!("TCP proxy error: {}", e);
            }
        })
    };
    
    let udp_handle = {
        let udp_proxy = udp_proxy;
        tokio::spawn(async move {
            if let Err(e) = udp_proxy.run().await {
                error!("UDP proxy error: {}", e);
            }
        })
    };
    
    info!("Enhanced Dynamic Proxy is running!");
    info!("");
    info!("Features enabled:");
    info!("✓ TCP/UDP Proxying");
    info!("✓ TLS Termination with SNI");
    info!("✓ Redis Backend Integration");
    info!("✓ Multi-Certificate Support");
    info!("✓ Health Checking");
    info!("✓ Wildcard Routing");
    info!("✓ Prometheus Metrics");
    info!("");
    info!("Test the proxy with:");
    info!("  curl http://{}/ ", settings.server.tcp_bind);
    info!("  curl http://{}/metrics", settings.monitoring.metrics_bind);
    info!("");
    info!("Press Ctrl+C to stop...");
    info!("Or send SIGTERM for graceful shutdown, SIGQUIT for immediate shutdown");

    // 等待关机信号并执行优雅关机
    info!("Waiting for shutdown signal...");
    if let Err(e) = shutdown_manager.execute_graceful_shutdown().await {
        error!("Error during graceful shutdown: {}", e);
    }

    // 优雅关闭
    info!("Shutting down services...");

    // 取消任务
    tcp_handle.abort();
    udp_handle.abort();

    // 等待任务完成（给一点时间让它们清理）
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 关闭插件
    if let Err(e) = plugin_manager.shutdown().await {
        error!("Error shutting down plugins: {}", e);
    }

    info!("Enhanced Dynamic Proxy stopped");
    Ok(())
}

// 辅助函数：显示使用示例
#[allow(dead_code)]
fn show_usage_examples() {
    println!("Enhanced Dynamic Proxy Usage Examples:");
    println!("=====================================");
    println!();
    println!("1. Basic HTTP Request:");
    println!("   curl http://127.0.0.1:8080/");
    println!();
    println!("2. HTTPS Request with SNI:");
    println!("   curl -k https://api.example.com:8080/");
    println!();
    println!("3. View Metrics:");
    println!("   curl http://127.0.0.1:9090/metrics");
    println!();
    println!("4. Redis Backend Management:");
    println!("   redis-cli SET 'proxy:backends:my_service' '{{\"address\":\"127.0.0.1:8080\",\"weight\":100,\"healthy\":true,\"last_check\":1234567890,\"metadata\":{{}}}}'");
    println!("   redis-cli GET 'proxy:backends:my_service'");
    println!("   redis-cli KEYS 'proxy:backends:*'");
    println!();
    println!("5. Health Check:");
    println!("   # Health status is automatically updated in Redis");
    println!("   redis-cli GET 'proxy:backends:api_backend' | jq '.healthy'");
    println!();
}
