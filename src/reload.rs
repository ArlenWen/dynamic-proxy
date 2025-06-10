use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::{Config, ConfigManager, HealthChecker, MetricsCollector, ProxyServer, RouterManager};

/// 热重载协调器
pub struct ReloadCoordinator {
    config_manager: Arc<ConfigManager>,
    proxy_server: Arc<ProxyServer>,
    router_manager: Arc<RouterManager>,
    health_checker: Arc<HealthChecker>,
    metrics_collector: Arc<MetricsCollector>,
}

impl ReloadCoordinator {
    pub fn new(
        config_manager: Arc<ConfigManager>,
        proxy_server: Arc<ProxyServer>,
        router_manager: Arc<RouterManager>,
        health_checker: Arc<HealthChecker>,
        metrics_collector: Arc<MetricsCollector>,
    ) -> Self {
        Self {
            config_manager,
            proxy_server,
            router_manager,
            health_checker,
            metrics_collector,
        }
    }

    /// 启动热重载监听
    pub async fn start_hot_reload(&self, mut reload_rx: mpsc::UnboundedReceiver<()>) -> Result<()> {
        info!("Starting hot reload coordinator");

        while let Some(()) = reload_rx.recv().await {
            info!("Received configuration reload signal");
            
            if let Err(e) = self.perform_reload().await {
                error!("Failed to reload configuration: {}", e);
            }
        }

        info!("Hot reload coordinator stopped");
        Ok(())
    }

    /// 执行配置重载
    async fn perform_reload(&self) -> Result<()> {
        info!("Starting configuration reload...");

        // 1. 重新加载配置文件
        self.config_manager
            .reload()
            .await
            .context("Failed to reload configuration file")?;

        let new_config = self.config_manager.get().await;

        // 2. 验证新配置
        new_config
            .validate()
            .context("New configuration validation failed")?;

        info!("New configuration validated successfully");

        // 3. 检查哪些组件需要重载
        let reload_result = self.reload_components(&new_config).await;

        match reload_result {
            Ok(()) => {
                info!("Configuration reload completed successfully");
            }
            Err(e) => {
                error!("Configuration reload failed: {}", e);
                warn!("System may be in an inconsistent state");
                return Err(e);
            }
        }

        Ok(())
    }

    /// 重载各个组件
    async fn reload_components(&self, new_config: &Config) -> Result<()> {
        // 1. 重载路由管理器（这个通常是安全的）
        self.router_manager
            .reload_config(new_config)
            .await
            .context("Failed to reload router manager")?;

        // 2. 重载健康检查器
        if let Err(e) = self.reload_health_checker(new_config).await {
            warn!("Failed to reload health checker: {}", e);
            // 健康检查器重载失败不是致命错误
        }

        // 3. 重载指标收集器
        if let Err(e) = self.reload_metrics_collector(new_config).await {
            warn!("Failed to reload metrics collector: {}", e);
            // 指标收集器重载失败不是致命错误
        }

        // 4. 检查是否需要重载代理服务器
        // 注意：某些配置变更（如绑定地址）需要重启代理服务器
        if let Err(e) = self.check_proxy_reload_needed(new_config).await {
            warn!("Proxy server configuration change detected: {}", e);
            warn!("Some changes require a full restart to take effect");
        }

        Ok(())
    }

    /// 重载健康检查器
    async fn reload_health_checker(&self, _new_config: &Config) -> Result<()> {
        // 如果健康检查配置发生变化，需要重启健康检查器
        info!("Reloading health checker configuration");
        
        // 停止当前的健康检查器
        self.health_checker.stop().await?;
        
        // 这里需要创建新的健康检查器实例
        // 由于架构限制，我们暂时只记录警告
        warn!("Health checker restart not fully implemented - requires service restart for configuration changes");
        
        Ok(())
    }

    /// 重载指标收集器
    async fn reload_metrics_collector(&self, _new_config: &Config) -> Result<()> {
        info!("Reloading metrics collector configuration");
        
        // 指标收集器的配置变更通常需要重启
        warn!("Metrics collector restart not fully implemented - requires service restart for configuration changes");
        
        Ok(())
    }

    /// 检查代理服务器是否需要重载
    async fn check_proxy_reload_needed(&self, new_config: &Config) -> Result<()> {
        // 获取当前配置（这里需要从代理服务器获取当前配置）
        // 由于架构限制，我们暂时只能检查一些基本变更
        
        info!("Checking if proxy server reload is needed");
        
        // 绑定地址变更需要重启
        warn!("Proxy server bind address changes require a full restart");
        
        // 尝试重载代理服务器配置
        self.proxy_server
            .reload_config(new_config)
            .await
            .context("Failed to reload proxy server configuration")?;
        
        Ok(())
    }

    /// 手动触发重载
    pub async fn manual_reload(&self) -> Result<()> {
        info!("Manual configuration reload requested");
        self.perform_reload().await
    }
}

/// 热重载配置
#[derive(Debug, Clone)]
pub struct HotReloadConfig {
    pub enabled: bool,
    pub watch_config_file: bool,
    pub reload_interval: Option<u64>, // seconds
}

impl Default for HotReloadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            watch_config_file: true,
            reload_interval: None, // 只在文件变更时重载
        }
    }
}
