use super::{BackendInfo, Router, RouterRegistry, RouterStats, RoutingContext};
use crate::config::{Config, RouterConfig, RoutingRule};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// 路由器管理器 - 负责管理所有路由器实例和路由规则
pub struct RouterManager {
    /// 路由器注册表
    registry: RouterRegistry,
    /// 活跃的路由器实例
    routers: Arc<RwLock<HashMap<String, Box<dyn Router>>>>,
    /// 路由规则
    rules: Arc<RwLock<Vec<RoutingRule>>>,
    /// 后端服务器信息缓存
    backends_cache: Arc<RwLock<HashMap<String, Vec<BackendInfo>>>>,
}

impl Default for RouterManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RouterManager {
    pub fn new() -> Self {
        let mut registry = RouterRegistry::new();

        // 注册内置路由器
        Self::register_builtin_routers(&mut registry);

        Self {
            registry,
            routers: Arc::new(RwLock::new(HashMap::new())),
            rules: Arc::new(RwLock::new(Vec::new())),
            backends_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册内置路由器
    fn register_builtin_routers(registry: &mut RouterRegistry) {
        use super::plugins::*;

        registry.register("round_robin".to_string(), || {
            Box::new(round_robin::RoundRobinRouter::new())
        });

        registry.register("weighted".to_string(), || {
            Box::new(weighted::WeightedRouter::new())
        });

        registry.register("hash".to_string(), || Box::new(hash::HashRouter::new()));

        registry.register("least_connections".to_string(), || {
            Box::new(least_connections::LeastConnectionsRouter::new())
        });

        registry.register("random".to_string(), || {
            Box::new(random::RandomRouter::new())
        });

        info!(
            "Registered {} builtin routers",
            registry.list_routers().len()
        );
    }

    /// 注册自定义路由器
    pub fn register_router<F>(&mut self, name: String, factory: F)
    where
        F: Fn() -> Box<dyn Router> + Send + Sync + 'static,
    {
        self.registry.register(name.clone(), factory);
        info!("Registered custom router: {}", name);
    }

    /// 从配置初始化路由器管理器
    pub async fn initialize(&self, config: &Config) -> Result<()> {
        info!("Initializing router manager...");

        // 初始化路由器实例
        let mut routers = self.routers.write().await;
        for (name, router_config) in &config.routers {
            let router = self
                .create_router(name, router_config)
                .await
                .with_context(|| format!("Failed to create router '{}'", name))?;
            routers.insert(name.clone(), router);
        }

        // 加载路由规则
        let mut rules = self.rules.write().await;
        *rules = config.rules.clone();

        // 初始化后端缓存
        let mut backends_cache = self.backends_cache.write().await;
        for rule in &config.rules {
            let backends: Vec<BackendInfo> = rule
                .backends
                .iter()
                .map(|b| BackendInfo {
                    id: b.id.clone(),
                    address: b.address,
                    weight: b.weight.unwrap_or(1),
                    enabled: b.enabled,
                    healthy: true, // 初始状态假设健康
                    current_connections: 0,
                    max_connections: None,
                    response_time: None,
                    metadata: b.metadata.clone().unwrap_or_default(),
                })
                .collect();
            backends_cache.insert(rule.name.clone(), backends);
        }

        info!(
            "Router manager initialized with {} routers and {} rules",
            routers.len(),
            rules.len()
        );
        Ok(())
    }

    /// 创建路由器实例
    async fn create_router(&self, name: &str, config: &RouterConfig) -> Result<Box<dyn Router>> {
        let mut router = self
            .registry
            .create(&config.plugin)
            .with_context(|| format!("Failed to create router plugin '{}'", config.plugin))?;

        router
            .initialize(config.config.clone())
            .await
            .with_context(|| format!("Failed to initialize router '{}'", name))?;

        info!("Created router '{}' using plugin '{}'", name, config.plugin);
        Ok(router)
    }

    /// 路由请求到后端服务器
    pub async fn route(&self, context: &RoutingContext) -> Result<Option<BackendInfo>> {
        debug!(
            "Routing request from {} to {}",
            context.client_addr, context.target_addr
        );

        // 查找匹配的路由规则
        let rule = self.find_matching_rule(context).await;
        let rule = match rule {
            Some(rule) => rule,
            None => {
                debug!("No matching rule found for request");
                return Ok(None);
            }
        };

        if !rule.enabled {
            debug!("Rule '{}' is disabled", rule.name);
            return Ok(None);
        }

        // 获取路由器实例
        let routers = self.routers.read().await;
        let router = match routers.get(&rule.router) {
            Some(router) => router,
            None => {
                error!(
                    "Router '{}' not found for rule '{}'",
                    rule.router, rule.name
                );
                return Err(anyhow::anyhow!("Router '{}' not found", rule.router));
            }
        };

        // 获取后端服务器列表
        let backends_cache = self.backends_cache.read().await;
        let backends = match backends_cache.get(&rule.name) {
            Some(backends) => backends.clone(),
            None => {
                warn!("No backends found for rule '{}'", rule.name);
                return Ok(None);
            }
        };

        // 过滤可用的后端服务器
        let available_backends: Vec<BackendInfo> = backends
            .into_iter()
            .filter(|b| b.can_accept_connection())
            .collect();

        if available_backends.is_empty() {
            warn!("No available backends for rule '{}'", rule.name);
            return Ok(None);
        }

        // 使用路由器选择后端
        let selected = router
            .select_backend(context, &available_backends)
            .await
            .with_context(|| format!("Router '{}' failed to select backend", rule.router))?;

        if let Some(ref backend) = selected {
            debug!(
                "Selected backend '{}' at {} for rule '{}'",
                backend.id, backend.address, rule.name
            );
        }

        Ok(selected)
    }

    /// 查找匹配的路由规则
    async fn find_matching_rule(&self, context: &RoutingContext) -> Option<RoutingRule> {
        let rules = self.rules.read().await;

        for rule in rules.iter() {
            if self.rule_matches(rule, context) {
                return Some(rule.clone());
            }
        }

        None
    }

    /// 检查规则是否匹配
    fn rule_matches(&self, rule: &RoutingRule, context: &RoutingContext) -> bool {
        use crate::config::{Protocol, RuleMatcher};

        // 检查协议匹配
        let protocol_matches = matches!(
            (&rule.protocol, &context.protocol),
            (Protocol::Both, _)
                | (Protocol::Tcp, super::Protocol::Tcp)
                | (Protocol::Udp, super::Protocol::Udp)
        );

        if !protocol_matches {
            return false;
        }

        // 检查匹配器
        match &rule.matcher {
            RuleMatcher::Port { port } => context.target_port() == *port,
            RuleMatcher::PortRange { start, end } => {
                let target_port = context.target_port();
                target_port >= *start && target_port <= *end
            }
            RuleMatcher::Sni { pattern } => context.matches_sni(pattern),
            RuleMatcher::SourceIp { cidr } => context.client_in_cidr(cidr),
            RuleMatcher::DestinationIp { cidr: _ } => {
                // TODO: 实现目标IP CIDR匹配
                false
            }
            RuleMatcher::Custom {
                plugin: _,
                config: _,
            } => {
                // TODO: 实现自定义匹配器
                false
            }
        }
    }

    /// 更新后端服务器健康状态
    pub async fn update_backend_health(
        &self,
        rule_name: &str,
        backend_id: &str,
        healthy: bool,
    ) -> Result<()> {
        // 更新缓存中的健康状态
        let mut backends_cache = self.backends_cache.write().await;
        if let Some(backends) = backends_cache.get_mut(rule_name) {
            if let Some(backend) = backends.iter_mut().find(|b| b.id == backend_id) {
                backend.healthy = healthy;
                debug!(
                    "Updated backend '{}' health status to {}",
                    backend_id, healthy
                );
            }
        }

        // 通知相关路由器
        let rules = self.rules.read().await;
        if let Some(rule) = rules.iter().find(|r| r.name == rule_name) {
            let mut routers = self.routers.write().await;
            if let Some(router) = routers.get_mut(&rule.router) {
                router
                    .on_backend_health_changed(backend_id, healthy)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to notify router '{}' about backend health change",
                            rule.router
                        )
                    })?;
            }
        }

        Ok(())
    }

    /// 更新后端服务器连接数
    pub async fn update_backend_connections(
        &self,
        rule_name: &str,
        backend_id: &str,
        connections: u32,
    ) -> Result<()> {
        let mut backends_cache = self.backends_cache.write().await;
        if let Some(backends) = backends_cache.get_mut(rule_name) {
            if let Some(backend) = backends.iter_mut().find(|b| b.id == backend_id) {
                backend.current_connections = connections;
                debug!(
                    "Updated backend '{}' connection count to {}",
                    backend_id, connections
                );
            }
        }
        Ok(())
    }

    /// 获取路由器统计信息
    pub async fn get_router_stats(&self, router_name: &str) -> Result<RouterStats> {
        let routers = self.routers.read().await;
        if let Some(router) = routers.get(router_name) {
            router.get_stats().await
        } else {
            Err(anyhow::anyhow!("Router '{}' not found", router_name))
        }
    }

    /// 获取所有路由器统计信息
    pub async fn get_all_stats(&self) -> Result<HashMap<String, RouterStats>> {
        let routers = self.routers.read().await;
        let mut stats = HashMap::new();

        for (name, router) in routers.iter() {
            match router.get_stats().await {
                Ok(stat) => {
                    stats.insert(name.clone(), stat);
                }
                Err(e) => {
                    error!("Failed to get stats for router '{}': {}", name, e);
                }
            }
        }

        Ok(stats)
    }

    /// 重新加载配置
    pub async fn reload_config(&self, config: &Config) -> Result<()> {
        info!("Reloading router manager configuration...");

        // 更新路由规则
        let mut rules = self.rules.write().await;
        *rules = config.rules.clone();

        // 更新后端缓存
        let mut backends_cache = self.backends_cache.write().await;
        backends_cache.clear();
        for rule in &config.rules {
            let backends: Vec<BackendInfo> = rule
                .backends
                .iter()
                .map(|b| BackendInfo {
                    id: b.id.clone(),
                    address: b.address,
                    weight: b.weight.unwrap_or(1),
                    enabled: b.enabled,
                    healthy: true,
                    current_connections: 0,
                    max_connections: None,
                    response_time: None,
                    metadata: b.metadata.clone().unwrap_or_default(),
                })
                .collect();
            backends_cache.insert(rule.name.clone(), backends);
        }

        // 更新路由器配置
        let mut routers = self.routers.write().await;
        for (name, router_config) in &config.routers {
            if let Some(router) = routers.get_mut(name) {
                // 重新初始化现有路由器
                router
                    .initialize(router_config.config.clone())
                    .await
                    .with_context(|| format!("Failed to reinitialize router '{}'", name))?;
            } else {
                // 创建新路由器
                let router = self
                    .create_router(name, router_config)
                    .await
                    .with_context(|| format!("Failed to create new router '{}'", name))?;
                routers.insert(name.clone(), router);
            }
        }

        // 移除不再存在的路由器
        let config_router_names: std::collections::HashSet<_> = config.routers.keys().collect();
        let current_router_names: Vec<_> = routers.keys().cloned().collect();
        for name in current_router_names {
            if !config_router_names.contains(&name) {
                if let Some(mut router) = routers.remove(&name) {
                    if let Err(e) = router.shutdown().await {
                        error!("Failed to shutdown router '{}': {}", name, e);
                    }
                    info!("Removed router '{}'", name);
                }
            }
        }

        info!("Router manager configuration reloaded successfully");
        Ok(())
    }

    /// 关闭路由器管理器
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down router manager...");

        let mut routers = self.routers.write().await;
        for (name, mut router) in routers.drain() {
            if let Err(e) = router.shutdown().await {
                error!("Failed to shutdown router '{}': {}", name, e);
            }
        }

        info!("Router manager shutdown completed");
        Ok(())
    }

    /// 获取可用的路由器插件列表
    pub fn list_available_plugins(&self) -> Vec<String> {
        self.registry.list_routers()
    }
}
