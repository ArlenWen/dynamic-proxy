use dynamic_proxy::{config::Settings, proxy::ProxyServer};
use tracing::{error, info, warn};
use tracing_subscriber;
use clap::{Arg, Command};

// 版本信息常量
const VERSION: &str = env!("CARGO_PKG_VERSION");
const BUILD_TIME: &str = env!("BUILD_TIME");
const GIT_HASH: &str = env!("GIT_HASH");
const GIT_BRANCH: &str = env!("GIT_BRANCH");
const RUST_VERSION: &str = env!("RUST_VERSION");
const TARGET_ARCH: &str = env!("TARGET_ARCH");
const BUILD_PROFILE: &str = env!("BUILD_PROFILE");

fn get_version_info() -> String {
    format!(
        "Dynamic Proxy Server v{}\n\
        Build Information:\n\
        ├─ Build Time: {}\n\
        ├─ Git Hash: {}\n\
        ├─ Git Branch: {}\n\
        ├─ Rust Version: {}\n\
        ├─ Target Architecture: {}\n\
        └─ Build Profile: {}",
        VERSION, BUILD_TIME, GIT_HASH, GIT_BRANCH, RUST_VERSION, TARGET_ARCH, BUILD_PROFILE
    )
}

fn build_cli() -> Command {
    Command::new("dynamic-proxy")
        .version(VERSION)
        .about("A high-performance dynamic proxy server with Redis backend and TLS support")
        .long_about(
            "Dynamic Proxy Server is a high-performance proxy solution built with Rust and Pingora framework.\n\
            It supports TCP/UDP traffic routing, TLS with SNI-based routing, Redis backend for dynamic\n\
            configuration, plugin-based routing rules, and comprehensive monitoring with graceful shutdown."
        )
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Specify configuration file path")
                .default_value("config.toml")
        )
        .arg(
            Arg::new("version-info")
                .long("version-info")
                .help("Show detailed version and build information")
                .action(clap::ArgAction::SetTrue)
        )
        .arg(
            Arg::new("log-level")
                .short('l')
                .long("log-level")
                .value_name("LEVEL")
                .help("Set log level (trace, debug, info, warn, error)")
                .default_value("info")
        )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 解析命令行参数
    let matches = build_cli().get_matches();

    // 处理版本信息显示
    if matches.get_flag("version-info") {
        println!("{}", get_version_info());
        return Ok(());
    }

    // 获取日志级别
    let log_level = matches.get_one::<String>("log-level").unwrap();
    let log_filter = format!("dynamic_proxy={}", log_level);

    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(&log_filter)
        .init();

    info!("Starting Dynamic Proxy Server v{}", VERSION);
    info!("=============================");
    info!("Build: {} ({})", GIT_HASH, BUILD_TIME);
    info!("Branch: {} | Profile: {}", GIT_BRANCH, BUILD_PROFILE);

    // 获取配置文件路径
    let config_path = matches.get_one::<String>("config").unwrap();

    // 加载配置
    let settings = Settings::load_from_path(config_path).await?;
    info!("Configuration loaded successfully");

    // 显示配置信息
    info!("Server Configuration:");
    info!("  TCP Bind: {}", settings.server.tcp_bind);
    info!("  UDP Bind: {}", settings.server.udp_bind);
    info!("  Metrics: {}", settings.monitoring.metrics_bind);
    info!("  Process ID: {}", std::process::id());

    if let Some(tls_config) = &settings.tls {
        info!("  TLS Enabled: SNI routing = {}", tls_config.sni_routing);
    } else {
        info!("  TLS: Disabled");
    }

    info!("  Redis: {}", settings.redis.url);
    info!("");

    // 创建并启动代理服务器
    let mut proxy_server = ProxyServer::new(settings).await?;

    info!("Proxy server initialized successfully");
    info!("Signal handling enabled:");
    info!("  - SIGINT (Ctrl+C): Graceful shutdown");
    info!("  - SIGTERM: Graceful shutdown");
    info!("  - SIGQUIT: Force shutdown");
    info!("  - SIGHUP: Configuration reload (graceful shutdown)");
    info!("");

    // 启动服务器（这会阻塞直到收到关机信号）
    match proxy_server.start().await {
        Ok(()) => {
            info!("Dynamic Proxy Server stopped gracefully");
        }
        Err(e) => {
            error!("Proxy server error: {}", e);

            // 尝试获取关机状态以提供更好的错误信息
            let state = proxy_server.get_shutdown_state().await;
            warn!("Final shutdown state: {:?}", state);

            return Err(e);
        }
    }

    info!("Dynamic Proxy Server shutdown complete");
    Ok(())
}
