pub mod manager;
pub mod plugins;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use tracing::info;
use tracing::log::debug;
pub use manager::RouterManager;

/// 路由器接口 - 所有路由器插件必须实现此接口
#[async_trait]
pub trait Router: Send + Sync {
    /// 路由器名称
    fn name(&self) -> &str;

    /// 路由器版本
    fn version(&self) -> &str;

    /// 初始化路由器
    async fn initialize(&mut self, config: Value) -> Result<()>;

    /// 选择后端服务器
    async fn select_backend(
        &self,
        context: &RoutingContext,
        backends: &[BackendInfo],
    ) -> Result<Option<BackendInfo>>;

    /// 更新后端服务器列表
    async fn update_backends(&mut self, backends: Vec<BackendInfo>) -> Result<()>;

    /// 获取路由器统计信息
    async fn get_stats(&self) -> Result<RouterStats>;

    /// 健康检查回调 - 当后端服务器状态变化时调用
    async fn on_backend_health_changed(&mut self, backend_id: &str, healthy: bool) -> Result<()>;

    /// 清理资源
    async fn shutdown(&mut self) -> Result<()>;
}

/// 路由上下文 - 包含路由决策所需的信息
#[derive(Debug, Clone)]
pub struct RoutingContext {
    /// 客户端地址
    pub client_addr: SocketAddr,
    /// 目标地址
    pub target_addr: SocketAddr,
    /// 协议类型
    pub protocol: Protocol,
    /// SNI (仅对TLS连接有效)
    pub sni: Option<String>,
    /// 请求头信息 (仅对HTTP代理有效)
    pub headers: HashMap<String, String>,
    /// 自定义元数据
    pub metadata: HashMap<String, String>,
    /// 会话ID (用于会话保持)
    pub session_id: Option<String>,
}

/// 协议类型
#[derive(Debug, Clone, PartialEq)]
pub enum Protocol {
    Tcp,
    Udp,
}

/// 后端服务器信息
#[derive(Debug, Clone)]
pub struct BackendInfo {
    /// 后端ID
    pub id: String,
    /// 后端地址
    pub address: SocketAddr,
    /// 权重 (用于加权路由)
    pub weight: u32,
    /// 是否启用
    pub enabled: bool,
    /// 是否健康
    pub healthy: bool,
    /// 当前连接数
    pub current_connections: u32,
    /// 最大连接数
    pub max_connections: Option<u32>,
    /// 响应时间 (毫秒)
    pub response_time: Option<u64>,
    /// 自定义元数据
    pub metadata: HashMap<String, String>,
}

/// 路由器统计信息
#[derive(Debug, Clone)]
pub struct RouterStats {
    /// 路由器名称
    pub name: String,
    /// 总请求数
    pub total_requests: u64,
    /// 成功请求数
    pub successful_requests: u64,
    /// 失败请求数
    pub failed_requests: u64,
    /// 平均响应时间 (毫秒)
    pub avg_response_time: f64,
    /// 当前活跃连接数
    pub active_connections: u32,
    /// 后端服务器数量
    pub backend_count: u32,
    /// 健康的后端服务器数量
    pub healthy_backend_count: u32,
    /// 自定义指标
    pub custom_metrics: HashMap<String, f64>,
}

impl BackendInfo {
    pub fn new(id: String, address: SocketAddr) -> Self {
        Self {
            id,
            address,
            weight: 1,
            enabled: true,
            healthy: true,
            current_connections: 0,
            max_connections: None,
            response_time: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn is_available(&self) -> bool {
        self.enabled && self.healthy
    }

    pub fn can_accept_connection(&self) -> bool {
        if !self.is_available() {
            return false;
        }

        if let Some(max_conn) = self.max_connections {
            self.current_connections < max_conn
        } else {
            true
        }
    }
}

impl RoutingContext {
    pub fn new(client_addr: SocketAddr, target_addr: SocketAddr, protocol: Protocol) -> Self {
        Self {
            client_addr,
            target_addr,
            protocol,
            sni: None,
            headers: HashMap::new(),
            metadata: HashMap::new(),
            session_id: None,
        }
    }

    pub fn with_sni(mut self, sni: String) -> Self {
        self.sni = Some(sni);
        self
    }

    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_session_id(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// 获取客户端IP
    pub fn client_ip(&self) -> std::net::IpAddr {
        self.client_addr.ip()
    }

    /// 获取目标端口
    pub fn target_port(&self) -> u16 {
        self.target_addr.port()
    }

    /// 检查是否匹配指定的SNI模式
    pub fn matches_sni(&self, pattern: &str) -> bool {
        debug!("matches_sni: {:?}, {:?}", self.sni, pattern);
        
        if let Some(sni) = &self.sni {
            // 简单的通配符匹配
            if pattern.contains('*') {
                let pattern = pattern.replace('*', ".*");
                if let Ok(regex) = regex::Regex::new(&pattern) {
                    return regex.is_match(sni);
                }
            }
            sni == pattern
        } else {
            false
        }
    }

    /// 检查客户端IP是否在指定的CIDR范围内
    pub fn client_in_cidr(&self, _cidr: &str) -> bool {
        // TODO: 实现CIDR匹配
        // 这里需要使用ipnet或类似的库来解析CIDR并检查IP是否在范围内
        false
    }
}

impl Default for RouterStats {
    fn default() -> Self {
        Self {
            name: String::new(),
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            avg_response_time: 0.0,
            active_connections: 0,
            backend_count: 0,
            healthy_backend_count: 0,
            custom_metrics: HashMap::new(),
        }
    }
}

/// 路由器工厂 - 用于创建路由器实例
pub type RouterFactory = Box<dyn Fn() -> Box<dyn Router> + Send + Sync>;

/// 路由器注册表
pub struct RouterRegistry {
    factories: HashMap<String, RouterFactory>,
}

impl RouterRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// 注册路由器工厂
    pub fn register<F>(&mut self, name: String, factory: F)
    where
        F: Fn() -> Box<dyn Router> + Send + Sync + 'static,
    {
        self.factories.insert(name, Box::new(factory));
    }

    /// 创建路由器实例
    pub fn create(&self, name: &str) -> Result<Box<dyn Router>> {
        if let Some(factory) = self.factories.get(name) {
            Ok(factory())
        } else {
            Err(anyhow::anyhow!("Router '{}' not found", name))
        }
    }

    /// 获取所有已注册的路由器名称
    pub fn list_routers(&self) -> Vec<String> {
        self.factories.keys().cloned().collect()
    }
}

impl Default for RouterRegistry {
    fn default() -> Self {
        Self::new()
    }
}
