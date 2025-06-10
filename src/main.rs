use anyhow::{Context, Result};
use clap::{Arg, Command};
use dynamic_proxy::{
    Config, ConfigManager, HealthChecker, MetricsCollector, ProxyServer, RouterManager,
};
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// 构建信息（由build.rs生成）
const BUILD_TIME: &str = env!("BUILD_TIME");
const GIT_HASH: &str = env!("GIT_HASH");
const GIT_BRANCH: &str = env!("GIT_BRANCH");
const RUST_VERSION: &str = env!("RUST_VERSION");
const TARGET_ARCH: &str = env!("TARGET_ARCH");
const BUILD_PROFILE: &str = env!("BUILD_PROFILE");

#[tokio::main]
async fn main() -> Result<()> {
    // 解析命令行参数
    let matches = Command::new("dynamic-proxy")
        .version(env!("CARGO_PKG_VERSION"))
        .author("ArlenWen")
        .about("Dynamic TCP/UDP proxy with plugin-based routing")
        .disable_version_flag(true)
        .long_about(format!(
            "Dynamic TCP/UDP proxy with plugin-based routing\n\
             \n\
             Build Information:\n\
             - Version: {}\n\
             - Build Time: {}\n\
             - Git Hash: {}\n\
             - Git Branch: {}\n\
             - Rust Version: {}\n\
             - Target: {}\n\
             - Profile: {}",
            env!("CARGO_PKG_VERSION"),
            BUILD_TIME,
            GIT_HASH,
            GIT_BRANCH,
            RUST_VERSION,
            TARGET_ARCH,
            BUILD_PROFILE
        ))
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Configuration file path")
                .default_value("config.toml"),
        )
        .arg(
            Arg::new("validate")
                .long("validate")
                .help("Validate configuration and exit")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("version-info")
                .short('V')
                .long("version-info")
                .help("Show version information")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("log-level")
                .short('l')
                .long("log-level")
                .value_name("LEVEL")
                .help("Log level (trace, debug, info, warn, error)")
                .default_value("info"),
        )
        .arg(
            Arg::new("log-format")
                .long("log-format")
                .value_name("FORMAT")
                .help("Log format (json, pretty)")
                .default_value("pretty"),
        )
        .get_matches();

    // 显示版本信息
    if matches.get_flag("version-info") {
        print_version_info();
        return Ok(());
    }

    // 初始化日志
    let log_level = matches.get_one::<String>("log-level").unwrap();
    let log_format = matches.get_one::<String>("log-format").unwrap();
    init_logging(log_level, log_format)?;

    info!("Starting Dynamic Proxy v{}", env!("CARGO_PKG_VERSION"));
    info!("Build: {} ({})", GIT_HASH, BUILD_TIME);

    // 加载配置
    let config_path = matches.get_one::<String>("config").unwrap();
    let config = Config::from_file(config_path)
        .with_context(|| format!("Failed to load configuration from {}", config_path))?;

    // 验证配置
    config
        .validate()
        .context("Configuration validation failed")?;

    if matches.get_flag("validate") {
        info!("Configuration is valid");
        return Ok(());
    }

    // 创建配置管理器
    let config_manager = Arc::new(ConfigManager::new(config_path.clone()));
    config_manager
        .load()
        .await
        .context("Failed to load configuration")?;

    // 创建核心组件
    let router_manager = Arc::new(RouterManager::new());
    let health_checker = Arc::new(HealthChecker::new(
        config.health_check.clone(),
        router_manager.clone(),
    ));
    let metrics_collector = Arc::new(
        MetricsCollector::new(config.metrics.clone())
            .context("Failed to create metrics collector")?,
    );

    // 创建代理服务器
    let proxy_server = Arc::new(ProxyServer::new(
        config.server.clone(),
        router_manager.clone(),
        health_checker.clone(),
        metrics_collector.clone(),
    ));

    // 初始化路由管理器
    router_manager
        .initialize(&config)
        .await
        .context("Failed to initialize router manager")?;

    // 添加后端到健康检查
    for rule in &config.rules {
        for backend in &rule.backends {
            health_checker
                .add_backend(rule.name.clone(), backend)
                .await
                .with_context(|| {
                    format!("Failed to add backend {} to health checker", backend.id)
                })?;
        }
    }

    // 启动代理服务器
    proxy_server
        .start()
        .await
        .context("Failed to start proxy server")?;

    info!("Dynamic Proxy started successfully");
    info!("TCP listening on: {}", config.server.tcp_bind);
    info!("UDP listening on: {}", config.server.udp_bind);
    if config.metrics.enabled {
        info!(
            "Metrics available at: http://{}{}",
            config.metrics.bind, config.metrics.path
        );
    }

    // 设置信号处理
    let proxy_server_shutdown = proxy_server.clone();
    let router_manager_shutdown = router_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = wait_for_shutdown_signal().await {
            error!("Signal handling error: {}", e);
        }

        info!("Shutdown signal received, starting graceful shutdown...");

        // 优雅关闭代理服务器
        if let Err(e) = proxy_server_shutdown.graceful_shutdown(30).await {
            error!("Failed to gracefully shutdown proxy server: {}", e);
        }

        // 关闭路由管理器
        if let Err(e) = router_manager_shutdown.shutdown().await {
            error!("Failed to shutdown router manager: {}", e);
        }

        info!("Graceful shutdown completed");
        std::process::exit(0);
    });

    // 配置热重载
    let _config_manager_reload = config_manager.clone();
    let _proxy_server_reload = proxy_server.clone();
    tokio::spawn(async move {
        let mut reload_interval = tokio::time::interval(std::time::Duration::from_secs(60));

        loop {
            reload_interval.tick().await;

            // 检查配置文件是否有更新
            // 这里可以实现文件监控或定期检查
            // 为了简化，我们暂时跳过自动重载
        }
    });

    // 主循环 - 保持程序运行
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // 这里可以添加一些周期性任务
        // 比如更新系统指标等
        metrics_collector.update_system_metrics();
    }
}

/// 初始化日志系统
fn init_logging(level: &str, format: &str) -> Result<()> {
    let level_filter = match level.to_lowercase().as_str() {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => return Err(anyhow::anyhow!("Invalid log level: {}", level)),
    };

    let registry = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env().add_directive(level_filter.into()));

    match format.to_lowercase().as_str() {
        "json" => {
            registry.with(tracing_subscriber::fmt::layer()).init();
        }
        "pretty" => {
            registry
                .with(tracing_subscriber::fmt::layer().pretty())
                .init();
        }
        _ => {
            return Err(anyhow::anyhow!("Invalid log format: {}", format));
        }
    }

    Ok(())
}

/// 等待关闭信号
async fn wait_for_shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        use signal::unix::{SignalKind, signal};

        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sighup = signal(SignalKind::hangup())?;

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT");
            }
            _ = sighup.recv() => {
                info!("Received SIGHUP");
            }
        }
    }

    #[cfg(windows)]
    {
        let mut ctrl_c = signal::windows::ctrl_c()?;
        let mut ctrl_break = signal::windows::ctrl_break()?;

        tokio::select! {
            _ = ctrl_c.recv() => {
                info!("Received Ctrl+C");
            }
            _ = ctrl_break.recv() => {
                info!("Received Ctrl+Break");
            }
        }
    }

    Ok(())
}

/// 打印版本信息
fn print_version_info() {
    println!("Dynamic Proxy v{}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Build Information:");
    println!("  Build Time: {}", BUILD_TIME);
    println!("  Git Hash: {}", GIT_HASH);
    println!("  Git Branch: {}", GIT_BRANCH);
    println!("  Rust Version: {}", RUST_VERSION);
    println!("  Target: {}", TARGET_ARCH);
    println!("  Profile: {}", BUILD_PROFILE);
    println!();
    println!("Features:");
    println!("  - TCP/UDP traffic forwarding");
    println!("  - Plugin-based routing system");
    println!("  - Dynamic configuration loading");
    println!("  - Health checking");
    println!("  - Prometheus metrics");
    println!("  - Graceful shutdown");
}
