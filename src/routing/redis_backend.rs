use async_trait::async_trait;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use anyhow::{Result, Context};
use tokio::sync::RwLock;
use redis::{AsyncCommands, Client};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use crate::routing::plugin::RoutingPluginMut;
use crate::routing::rules::{RoutingContext, RoutingDecision, RoutingAction};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendInfo {
    pub address: SocketAddr,
    pub weight: u32,
    pub healthy: bool,
    pub last_check: u64,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct CachedBackend {
    info: BackendInfo,
    cached_at: SystemTime,
}

pub struct RedisBackend {
    name: String,
    redis_url: String,
    redis_client: Option<Client>,
    backend_cache: Arc<RwLock<HashMap<String, CachedBackend>>>,
    cache_ttl: Duration,
    connection_timeout: Duration,
    max_retries: u32,
    fallback_backends: HashMap<String, BackendInfo>,
    redis_available: Arc<tokio::sync::RwLock<bool>>,
}

impl RedisBackend {
    pub fn new() -> Self {
        let mut fallback_backends = HashMap::new();

        // 添加一些默认的回退后端
        fallback_backends.insert(
            "default".to_string(),
            BackendInfo {
                address: "127.0.0.1:8000".parse().unwrap(),
                weight: 100,
                healthy: true,
                last_check: SystemTime::now().duration_since(UNIX_EPOCH)
                    .unwrap_or_default().as_secs(),
                metadata: HashMap::new(),
            }
        );

        Self {
            name: "redis_backend".to_string(),
            redis_url: "redis://127.0.0.1:6379".to_string(),
            redis_client: None,
            backend_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: Duration::from_secs(300), // 5分钟缓存
            connection_timeout: Duration::from_secs(5),
            max_retries: 3,
            fallback_backends,
            redis_available: Arc::new(tokio::sync::RwLock::new(false)),
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        info!("Connecting to Redis at: {}", self.redis_url);

        let client = Client::open(self.redis_url.clone())
            .with_context(|| format!("Failed to create Redis client for URL: {}", self.redis_url))?;

        // Test the connection
        let mut conn = client.get_multiplexed_async_connection().await
            .with_context(|| "Failed to establish Redis connection")?;

        // Ping to verify connection
        let _: String = conn.ping().await
            .with_context(|| "Failed to ping Redis server")?;

        self.redis_client = Some(client);

        // 更新Redis可用状态
        {
            let mut redis_available = self.redis_available.write().await;
            *redis_available = true;
        }

        info!("Successfully connected to Redis");
        Ok(())
    }

    async fn get_redis_connection(&self) -> Result<redis::aio::MultiplexedConnection> {
        let client = self.redis_client.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Redis client not initialized"))?;

        match client.get_multiplexed_async_connection().await {
            Ok(conn) => Ok(conn),
            Err(e) => {
                // 标记Redis为不可用
                {
                    let mut redis_available = self.redis_available.write().await;
                    *redis_available = false;
                }
                warn!("Redis connection failed, marking as unavailable: {}", e);
                Err(anyhow::anyhow!("Failed to get Redis connection: {}", e))
            }
        }
    }

    async fn resolve_backend(&self, backend_name: &str) -> Result<Option<BackendInfo>> {
        // 检查缓存
        {
            let cache = self.backend_cache.read().await;
            if let Some(cached) = cache.get(backend_name) {
                let age = SystemTime::now().duration_since(cached.cached_at)
                    .unwrap_or(Duration::from_secs(u64::MAX));

                if age < self.cache_ttl {
                    debug!("Cache hit for backend: {}", backend_name);
                    return Ok(Some(cached.info.clone()));
                } else {
                    debug!("Cache expired for backend: {}", backend_name);
                }
            }
        }

        // 从Redis获取后端信息
        match self.fetch_backend_from_redis(backend_name).await {
            Ok(Some(backend_info)) => {
                // 更新缓存
                let cached = CachedBackend {
                    info: backend_info.clone(),
                    cached_at: SystemTime::now(),
                };

                let mut cache = self.backend_cache.write().await;
                cache.insert(backend_name.to_string(), cached);

                debug!("Fetched backend from Redis: {} -> {}", backend_name, backend_info.address);
                Ok(Some(backend_info))
            }
            Ok(None) => {
                debug!("Backend not found in Redis: {}", backend_name);
                Ok(None)
            }
            Err(e) => {
                warn!("Failed to fetch backend from Redis: {}, falling back to defaults", e);
                // 回退到默认配置
                self.get_default_backend(backend_name).await
            }
        }
    }

    async fn fetch_backend_from_redis(&self, backend_name: &str) -> Result<Option<BackendInfo>> {
        let mut conn = self.get_redis_connection().await?;

        let key = format!("proxy:backends:{}", backend_name);
        let backend_data: Option<String> = conn.get(&key).await
            .with_context(|| format!("Failed to get backend data for key: {}", key))?;

        if let Some(data) = backend_data {
            let backend_info: BackendInfo = serde_json::from_str(&data)
                .with_context(|| format!("Failed to deserialize backend info for: {}", backend_name))?;
            Ok(Some(backend_info))
        } else {
            Ok(None)
        }
    }

    async fn get_default_backend(&self, backend_name: &str) -> Result<Option<BackendInfo>> {
        // 检查Redis是否可用
        let redis_available = *self.redis_available.read().await;

        if !redis_available {
            // Redis不可用时，使用回退后端
            if let Some(fallback) = self.fallback_backends.get(backend_name) {
                debug!("Using fallback backend for: {}", backend_name);
                return Ok(Some(fallback.clone()));
            }

            // 如果没有特定的回退后端，尝试使用默认后端
            if let Some(default) = self.fallback_backends.get("default") {
                debug!("Using default fallback backend for: {}", backend_name);
                return Ok(Some(default.clone()));
            }
        }

        debug!("No fallback backend available for: {}", backend_name);
        Ok(None)
    }

    pub fn add_fallback_backend(&mut self, name: String, backend_info: BackendInfo) {
        self.fallback_backends.insert(name, backend_info);
    }

    pub fn remove_fallback_backend(&mut self, name: &str) {
        self.fallback_backends.remove(name);
    }

    pub async fn update_backend(&self, backend_name: &str, backend_info: BackendInfo) -> Result<()> {
        // 更新Redis
        let mut conn = self.get_redis_connection().await?;
        let key = format!("proxy:backends:{}", backend_name);
        let data = serde_json::to_string(&backend_info)
            .with_context(|| "Failed to serialize backend info")?;

        let _: () = conn.set(&key, data).await
            .with_context(|| format!("Failed to update backend in Redis: {}", backend_name))?;

        // 更新本地缓存
        let cached = CachedBackend {
            info: backend_info.clone(),
            cached_at: SystemTime::now(),
        };

        let mut cache = self.backend_cache.write().await;
        cache.insert(backend_name.to_string(), cached);

        info!("Updated backend {} to {}", backend_name, backend_info.address);
        Ok(())
    }

    pub async fn update_backend_simple(&self, backend_name: &str, addr: SocketAddr) -> Result<()> {
        let backend_info = BackendInfo {
            address: addr,
            weight: 100,
            healthy: true,
            last_check: SystemTime::now().duration_since(UNIX_EPOCH)
                .unwrap_or_default().as_secs(),
            metadata: HashMap::new(),
        };

        self.update_backend(backend_name, backend_info).await
    }

    pub async fn remove_backend(&self, backend_name: &str) -> Result<()> {
        // 从Redis删除
        let mut conn = self.get_redis_connection().await?;
        let key = format!("proxy:backends:{}", backend_name);

        let _: () = conn.del(&key).await
            .with_context(|| format!("Failed to delete backend from Redis: {}", backend_name))?;

        // 从本地缓存移除
        let mut cache = self.backend_cache.write().await;
        cache.remove(backend_name);

        info!("Removed backend {}", backend_name);
        Ok(())
    }

    pub async fn list_backends(&self) -> Result<Vec<(String, BackendInfo)>> {
        // 从Redis获取所有后端
        let mut conn = self.get_redis_connection().await?;
        let pattern = "proxy:backends:*";

        let keys: Vec<String> = conn.keys(pattern).await
            .with_context(|| "Failed to get backend keys from Redis")?;

        let mut backends = Vec::new();

        for key in keys {
            if let Some(backend_name) = key.strip_prefix("proxy:backends:") {
                if let Ok(Some(backend_info)) = self.resolve_backend(backend_name).await {
                    backends.push((backend_name.to_string(), backend_info));
                }
            }
        }

        Ok(backends)
    }

    pub async fn list_backends_simple(&self) -> Result<Vec<(String, SocketAddr)>> {
        let backends = self.list_backends().await?;
        Ok(backends.into_iter()
            .map(|(name, info)| (name, info.address))
            .collect())
    }

    pub async fn health_check(&self, backend_name: &str) -> Result<bool> {
        if let Some(backend_info) = self.resolve_backend(backend_name).await? {
            let health_result = self.perform_health_check(&backend_info).await;

            // 更新健康状态到Redis
            let mut updated_info = backend_info;
            updated_info.healthy = health_result;
            updated_info.last_check = SystemTime::now().duration_since(UNIX_EPOCH)
                .unwrap_or_default().as_secs();

            if let Err(e) = self.update_backend(backend_name, updated_info).await {
                warn!("Failed to update health status in Redis: {}", e);
            }

            Ok(health_result)
        } else {
            warn!("Backend not found for health check: {}", backend_name);
            Ok(false)
        }
    }

    async fn perform_health_check(&self, backend_info: &BackendInfo) -> bool {
        // 检查metadata中是否指定了健康检查类型
        let check_type = backend_info.metadata.get("health_check_type")
            .map(|s| s.as_str())
            .unwrap_or("tcp");

        match check_type {
            "http" => self.http_health_check(backend_info).await,
            "https" => self.https_health_check(backend_info).await,
            "tcp" | _ => self.tcp_health_check(backend_info).await,
        }
    }

    async fn tcp_health_check(&self, backend_info: &BackendInfo) -> bool {
        match tokio::time::timeout(
            self.connection_timeout,
            tokio::net::TcpStream::connect(backend_info.address)
        ).await {
            Ok(Ok(_)) => {
                debug!("TCP health check passed for backend: {}", backend_info.address);
                true
            }
            Ok(Err(e)) => {
                warn!("TCP health check failed for backend {}: {}", backend_info.address, e);
                false
            }
            Err(_) => {
                warn!("TCP health check timed out for backend: {}", backend_info.address);
                false
            }
        }
    }

    async fn http_health_check(&self, backend_info: &BackendInfo) -> bool {
        let health_path = backend_info.metadata.get("health_check_path")
            .map(|s| s.as_str())
            .unwrap_or("/health");

        let _url = format!("http://{}{}", backend_info.address, health_path);

        // 简化的HTTP健康检查实现
        match self.tcp_health_check(backend_info).await {
            true => {
                debug!("HTTP health check passed for backend: {} (simplified TCP check)", backend_info.address);
                true
            }
            false => {
                warn!("HTTP health check failed for backend: {}", backend_info.address);
                false
            }
        }
    }

    async fn https_health_check(&self, backend_info: &BackendInfo) -> bool {
        let health_path = backend_info.metadata.get("health_check_path")
            .map(|s| s.as_str())
            .unwrap_or("/health");

        let _url = format!("https://{}{}", backend_info.address, health_path);

        // 简化的HTTPS健康检查实现
        match self.tcp_health_check(backend_info).await {
            true => {
                debug!("HTTPS health check passed for backend: {} (simplified TCP check)", backend_info.address);
                true
            }
            false => {
                warn!("HTTPS health check failed for backend: {}", backend_info.address);
                false
            }
        }
    }

    async fn wildcard_route(&self, sni: &str) -> Result<Option<RoutingDecision>> {
        // 实现通配符路由匹配
        let backends = self.list_backends().await?;

        for (backend_name, backend_info) in backends {
            if backend_name.starts_with("*.") {
                let domain = &backend_name[2..];
                if sni.ends_with(domain) && backend_info.healthy {
                    debug!("Wildcard match: {} -> {}", sni, backend_name);
                    return Ok(Some(RoutingDecision {
                        action: RoutingAction::Forward { backend: backend_name },
                        backend_addr: Some(backend_info.address),
                        tls_terminate: false,
                        rule_id: format!("redis_wildcard_{}", sni),
                    }));
                }
            }
        }

        Ok(None)
    }
}

#[async_trait]
impl RoutingPluginMut for RedisBackend {
    fn name(&self) -> &str {
        &self.name
    }

    async fn initialize(&mut self, config: serde_json::Value) -> Result<()> {
        if let Some(redis_url) = config.get("redis_url").and_then(|v| v.as_str()) {
            self.redis_url = redis_url.to_string();
        }

        if let Some(ttl) = config.get("cache_ttl").and_then(|v| v.as_u64()) {
            self.cache_ttl = Duration::from_secs(ttl);
        }

        if let Some(timeout) = config.get("connection_timeout").and_then(|v| v.as_u64()) {
            self.connection_timeout = Duration::from_secs(timeout);
        }

        if let Some(retries) = config.get("max_retries").and_then(|v| v.as_u64()) {
            self.max_retries = retries as u32;
        }

        // 连接到Redis
        self.connect().await?;

        info!("Redis backend plugin initialized with URL: {}", self.redis_url);
        Ok(())
    }

    async fn route(&self, context: &RoutingContext) -> Result<Option<RoutingDecision>> {
        // 基于SNI进行路由决策，支持更复杂的路由规则
        if let Some(sni) = &context.sni_hostname {
            debug!("Routing request for SNI: {}", sni);

            // 首先尝试精确匹配
            if let Some(backend_info) = self.resolve_backend(sni).await? {
                if backend_info.healthy {
                    return Ok(Some(RoutingDecision {
                        action: RoutingAction::Forward { backend: sni.to_string() },
                        backend_addr: Some(backend_info.address),
                        tls_terminate: false,
                        rule_id: format!("redis_exact_{}", sni),
                    }));
                }
            }

            // 直接尝试通配符匹配，不使用硬编码的前缀匹配
            return self.wildcard_route(sni).await;
        }

        // 尝试基于IP和端口的路由
        if let Some(backend_info) = self.resolve_backend("default").await? {
            if backend_info.healthy {
                return Ok(Some(RoutingDecision {
                    action: RoutingAction::Forward { backend: "default".to_string() },
                    backend_addr: Some(backend_info.address),
                    tls_terminate: false,
                    rule_id: "redis_default".to_string(),
                }));
            }
        }

        Ok(None)
    }

    async fn shutdown(&self) -> Result<()> {
        info!("Redis backend plugin shutting down");

        // 清理缓存
        let mut cache = self.backend_cache.write().await;
        cache.clear();

        // Redis连接会自动关闭
        Ok(())
    }
}
