use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use tokio::sync::RwLock;
use crate::routing::rules::{RoutingContext, RoutingDecision};

#[async_trait]
pub trait RoutingPlugin: Send + Sync {
    fn name(&self) -> &str;
    async fn initialize(&self, config: serde_json::Value) -> Result<()>;
    async fn route(&self, context: &RoutingContext) -> Result<Option<RoutingDecision>>;
    async fn shutdown(&self) -> Result<()>;
}

// 包装器，用于支持插件的内部可变性
pub struct PluginWrapper {
    plugin: Box<dyn RoutingPluginMut>,
}

#[async_trait]
pub trait RoutingPluginMut: Send + Sync {
    fn name(&self) -> &str;
    async fn initialize(&mut self, config: serde_json::Value) -> Result<()>;
    async fn route(&self, context: &RoutingContext) -> Result<Option<RoutingDecision>>;
    async fn shutdown(&self) -> Result<()>;
}

impl PluginWrapper {
    pub fn new(plugin: Box<dyn RoutingPluginMut>) -> Self {
        Self { plugin }
    }
}

#[async_trait]
impl RoutingPlugin for PluginWrapper {
    fn name(&self) -> &str {
        self.plugin.name()
    }

    async fn initialize(&self, _config: serde_json::Value) -> Result<()> {
        // 这里我们需要使用内部可变性，但为了简化，我们将在PluginManager中处理
        Ok(())
    }

    async fn route(&self, context: &RoutingContext) -> Result<Option<RoutingDecision>> {
        self.plugin.route(context).await
    }

    async fn shutdown(&self) -> Result<()> {
        self.plugin.shutdown().await
    }
}

pub struct PluginManager {
    plugins: HashMap<String, Arc<dyn RoutingPlugin>>,
    mutable_plugins: Arc<RwLock<HashMap<String, Box<dyn RoutingPluginMut>>>>,
    execution_order: Vec<String>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            mutable_plugins: Arc::new(RwLock::new(HashMap::new())),
            execution_order: Vec::new(),
        }
    }

    pub fn register_plugin(&mut self, plugin: Arc<dyn RoutingPlugin>) {
        let name = plugin.name().to_string();
        self.execution_order.push(name.clone());
        self.plugins.insert(name, plugin);
    }

    pub async fn register_mutable_plugin(&mut self, plugin: Box<dyn RoutingPluginMut>) {
        let name = plugin.name().to_string();
        self.execution_order.push(name.clone());
        let mut mutable_plugins = self.mutable_plugins.write().await;
        mutable_plugins.insert(name, plugin);
    }

    pub async fn initialize_plugins(&mut self, configs: HashMap<String, serde_json::Value>) -> Result<()> {
        // 初始化可变插件
        {
            let mut mutable_plugins = self.mutable_plugins.write().await;
            for (name, plugin) in mutable_plugins.iter_mut() {
                let config = configs.get(name).cloned().unwrap_or(serde_json::Value::Null);
                tracing::info!("Initializing mutable plugin: {}", name);
                if let Err(e) = plugin.initialize(config).await {
                    tracing::error!("Failed to initialize plugin {}: {}", name, e);
                    return Err(e);
                }
                tracing::info!("Successfully initialized plugin: {}", name);
            }
        }

        // 初始化不可变插件（如果有的话）
        for (name, plugin) in &self.plugins {
            let config = configs.get(name).cloned().unwrap_or(serde_json::Value::Null);
            tracing::info!("Initializing immutable plugin: {}", name);
            if let Err(e) = plugin.initialize(config).await {
                tracing::error!("Failed to initialize plugin {}: {}", name, e);
                return Err(e);
            }
            tracing::info!("Successfully initialized plugin: {}", name);
        }

        Ok(())
    }

    pub async fn route(&self, context: &RoutingContext) -> Result<Option<RoutingDecision>> {
        for plugin_name in &self.execution_order {
            // 首先检查可变插件
            {
                let mutable_plugins = self.mutable_plugins.read().await;
                if let Some(plugin) = mutable_plugins.get(plugin_name) {
                    if let Some(decision) = plugin.route(context).await? {
                        tracing::debug!("Mutable plugin {} made routing decision: {:?}", plugin_name, decision);
                        return Ok(Some(decision));
                    }
                }
            }

            // 然后检查不可变插件
            if let Some(plugin) = self.plugins.get(plugin_name) {
                if let Some(decision) = plugin.route(context).await? {
                    tracing::debug!("Immutable plugin {} made routing decision: {:?}", plugin_name, decision);
                    return Ok(Some(decision));
                }
            }
        }
        Ok(None)
    }

    pub async fn shutdown(&self) -> Result<()> {
        // 关闭可变插件
        {
            let mutable_plugins = self.mutable_plugins.read().await;
            for plugin in mutable_plugins.values() {
                plugin.shutdown().await?;
            }
        }

        // 关闭不可变插件
        for plugin in self.plugins.values() {
            plugin.shutdown().await?;
        }
        Ok(())
    }
}

// SNI路由插件实现
pub struct SniRouterPlugin {
    name: String,
    rules: Vec<crate::routing::rules::RoutingRule>,
}

impl SniRouterPlugin {
    pub fn new() -> Self {
        Self {
            name: "sni_router".to_string(),
            rules: Vec::new(),
        }
    }
}

#[async_trait]
impl RoutingPluginMut for SniRouterPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    async fn initialize(&mut self, config: serde_json::Value) -> Result<()> {
        // 从配置中加载路由规则
        if let Some(rules_config) = config.get("rules") {
            self.rules = serde_json::from_value(rules_config.clone())?;
        }
        tracing::info!("SNI Router plugin initialized with {} rules", self.rules.len());
        Ok(())
    }

    async fn route(&self, context: &RoutingContext) -> Result<Option<RoutingDecision>> {
        // 按优先级排序并查找匹配的规则
        let mut sorted_rules = self.rules.clone();
        sorted_rules.sort_by(|a, b| b.priority.cmp(&a.priority));

        for rule in sorted_rules {
            if let Some(decision) = rule.apply(context) {
                return Ok(Some(decision));
            }
        }

        Ok(None)
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!("SNI Router plugin shutting down");
        Ok(())
    }
}
