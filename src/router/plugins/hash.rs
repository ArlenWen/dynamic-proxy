use super::super::{BackendInfo, Router, RouterStats, RoutingContext};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// 哈希路由器 - 基于客户端IP或其他属性进行一致性哈希
pub struct HashRouter {
    name: String,
    version: String,
    backends: Arc<RwLock<Vec<BackendInfo>>>,
    stats: Arc<RwLock<RouterStats>>,
    hash_key: HashKey,
}

#[derive(Debug, Clone)]
enum HashKey {
    ClientIp,
    ClientPort,
    TargetPort,
    SessionId,
    Sni,
    Custom(String),
}

impl HashRouter {
    pub fn new() -> Self {
        Self {
            name: "hash".to_string(),
            version: "1.0.0".to_string(),
            backends: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(RouterStats {
                name: "hash".to_string(),
                ..Default::default()
            })),
            hash_key: HashKey::ClientIp,
        }
    }
}

#[async_trait]
impl Router for HashRouter {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn initialize(&mut self, config: Value) -> Result<()> {
        debug!("Initializing HashRouter with config: {}", config);

        if let Some(obj) = config.as_object() {
            // 配置哈希键
            if let Some(hash_key) = obj.get("hash_key") {
                if let Some(key_str) = hash_key.as_str() {
                    self.hash_key = match key_str {
                        "client_ip" => HashKey::ClientIp,
                        "client_port" => HashKey::ClientPort,
                        "target_port" => HashKey::TargetPort,
                        "session_id" => HashKey::SessionId,
                        "sni" => HashKey::Sni,
                        custom => HashKey::Custom(custom.to_string()),
                    };
                    debug!("HashRouter hash_key: {:?}", self.hash_key);
                }
            }
        }

        debug!("HashRouter initialized successfully");
        Ok(())
    }

    async fn select_backend(
        &self,
        context: &RoutingContext,
        backends: &[BackendInfo],
    ) -> Result<Option<BackendInfo>> {
        if backends.is_empty() {
            return Ok(None);
        }

        // 过滤可用的后端
        let available_backends: Vec<&BackendInfo> =
            backends.iter().filter(|b| b.is_available()).collect();

        if available_backends.is_empty() {
            return Ok(None);
        }

        // 计算哈希值
        let hash_value = self.calculate_hash(context);

        // 选择后端
        let index = (hash_value as usize) % available_backends.len();
        let selected = available_backends[index].clone();

        // 更新统计信息
        {
            let mut stats = self.stats.write().await;
            stats.total_requests += 1;
            stats.successful_requests += 1;
        }

        debug!(
            "HashRouter selected backend: {} (hash: {}, index: {})",
            selected.id, hash_value, index
        );
        Ok(Some(selected))
    }

    async fn update_backends(&mut self, backends: Vec<BackendInfo>) -> Result<()> {
        let mut current_backends = self.backends.write().await;
        *current_backends = backends;

        // 更新统计信息
        {
            let mut stats = self.stats.write().await;
            stats.backend_count = current_backends.len() as u32;
            stats.healthy_backend_count =
                current_backends.iter().filter(|b| b.healthy).count() as u32;
        }

        debug!(
            "HashRouter updated with {} backends",
            current_backends.len()
        );
        Ok(())
    }

    async fn get_stats(&self) -> Result<RouterStats> {
        let stats = self.stats.read().await;
        Ok(stats.clone())
    }

    async fn on_backend_health_changed(&mut self, backend_id: &str, healthy: bool) -> Result<()> {
        let mut backends = self.backends.write().await;
        if let Some(backend) = backends.iter_mut().find(|b| b.id == backend_id) {
            backend.healthy = healthy;
            debug!(
                "HashRouter: Backend '{}' health changed to {}",
                backend_id, healthy
            );
        }

        // 更新健康后端计数
        {
            let mut stats = self.stats.write().await;
            stats.healthy_backend_count = backends.iter().filter(|b| b.healthy).count() as u32;
        }

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        debug!("HashRouter shutting down");

        // 清理资源
        {
            let mut backends = self.backends.write().await;
            backends.clear();
        }

        debug!("HashRouter shutdown completed");
        Ok(())
    }
}

impl HashRouter {
    fn calculate_hash(&self, context: &RoutingContext) -> u64 {
        let mut hasher = DefaultHasher::new();

        match &self.hash_key {
            HashKey::ClientIp => {
                context.client_addr.ip().hash(&mut hasher);
            }
            HashKey::ClientPort => {
                context.client_addr.port().hash(&mut hasher);
            }
            HashKey::TargetPort => {
                context.target_addr.port().hash(&mut hasher);
            }
            HashKey::SessionId => {
                if let Some(session_id) = &context.session_id {
                    session_id.hash(&mut hasher);
                } else {
                    // 如果没有session_id，回退到client_ip
                    context.client_addr.ip().hash(&mut hasher);
                }
            }
            HashKey::Sni => {
                if let Some(sni) = &context.sni {
                    sni.hash(&mut hasher);
                } else {
                    // 如果没有SNI，回退到client_ip
                    context.client_addr.ip().hash(&mut hasher);
                }
            }
            HashKey::Custom(key) => {
                if let Some(value) = context.metadata.get(key) {
                    value.hash(&mut hasher);
                } else {
                    // 如果没有找到自定义键，回退到client_ip
                    context.client_addr.ip().hash(&mut hasher);
                }
            }
        }

        hasher.finish()
    }
}

impl Default for HashRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::{Protocol, RoutingContext};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_hash_consistency() {
        let mut router = HashRouter::new();
        router
            .initialize(serde_json::json!({
                "hash_key": "client_ip"
            }))
            .await
            .unwrap();

        let backends = vec![
            BackendInfo::new("backend1".to_string(), "127.0.0.1:8001".parse().unwrap()),
            BackendInfo::new("backend2".to_string(), "127.0.0.1:8002".parse().unwrap()),
            BackendInfo::new("backend3".to_string(), "127.0.0.1:8003".parse().unwrap()),
        ];

        let context = RoutingContext::new(
            "192.168.1.100:12345".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
            Protocol::Tcp,
        );

        // 多次选择同一个客户端，应该得到相同的后端
        let selected1 = router
            .select_backend(&context, &backends)
            .await
            .unwrap()
            .unwrap();
        let selected2 = router
            .select_backend(&context, &backends)
            .await
            .unwrap()
            .unwrap();
        let selected3 = router
            .select_backend(&context, &backends)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(selected1.id, selected2.id);
        assert_eq!(selected2.id, selected3.id);
    }

    #[tokio::test]
    async fn test_different_clients_distribution() {
        let mut router = HashRouter::new();
        router
            .initialize(serde_json::json!({
                "hash_key": "client_ip"
            }))
            .await
            .unwrap();

        let backends = vec![
            BackendInfo::new("backend1".to_string(), "127.0.0.1:8001".parse().unwrap()),
            BackendInfo::new("backend2".to_string(), "127.0.0.1:8002".parse().unwrap()),
            BackendInfo::new("backend3".to_string(), "127.0.0.1:8003".parse().unwrap()),
        ];

        let mut selections = HashMap::new();

        // 测试不同客户端IP的分布
        for i in 1..=100 {
            let client_addr = format!("192.168.1.{}:12345", i).parse().unwrap();
            let context = RoutingContext::new(
                client_addr,
                "127.0.0.1:8080".parse().unwrap(),
                Protocol::Tcp,
            );

            if let Some(selected) = router.select_backend(&context, &backends).await.unwrap() {
                *selections.entry(selected.id).or_insert(0) += 1;
            }
        }

        // 验证分布相对均匀
        println!("Hash distribution: {:?}", selections);
        assert!(selections.len() > 1); // 应该分布到多个后端
    }

    #[tokio::test]
    async fn test_sni_hash() {
        let mut router = HashRouter::new();
        router
            .initialize(serde_json::json!({
                "hash_key": "sni"
            }))
            .await
            .unwrap();

        let backends = vec![
            BackendInfo::new("backend1".to_string(), "127.0.0.1:8001".parse().unwrap()),
            BackendInfo::new("backend2".to_string(), "127.0.0.1:8002".parse().unwrap()),
        ];

        let context1 = RoutingContext::new(
            "192.168.1.100:12345".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
            Protocol::Tcp,
        )
        .with_sni("example.com".to_string());

        let context2 = RoutingContext::new(
            "192.168.1.101:12346".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
            Protocol::Tcp,
        )
        .with_sni("example.com".to_string());

        // 相同SNI应该路由到相同后端
        let selected1 = router
            .select_backend(&context1, &backends)
            .await
            .unwrap()
            .unwrap();
        let selected2 = router
            .select_backend(&context2, &backends)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(selected1.id, selected2.id);
    }
}
