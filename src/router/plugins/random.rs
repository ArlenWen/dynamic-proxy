use super::super::{BackendInfo, Router, RouterStats, RoutingContext};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// 随机路由器 - 随机选择后端服务器
pub struct RandomRouter {
    name: String,
    version: String,
    backends: Arc<RwLock<Vec<BackendInfo>>>,
    stats: Arc<RwLock<RouterStats>>,
}

impl RandomRouter {
    pub fn new() -> Self {
        Self {
            name: "random".to_string(),
            version: "1.0.0".to_string(),
            backends: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(RouterStats {
                name: "random".to_string(),
                ..Default::default()
            })),
        }
    }
}

#[async_trait]
impl Router for RandomRouter {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn initialize(&mut self, config: Value) -> Result<()> {
        debug!("Initializing RandomRouter with config: {}", config);

        // 随机路由器通常不需要特殊配置
        if let Some(obj) = config.as_object() {
            // 可以添加一些配置选项，比如随机种子等
            if let Some(seed) = obj.get("seed") {
                if let Some(seed_value) = seed.as_u64() {
                    debug!("RandomRouter seed: {}", seed_value);
                    // 这里可以设置随机种子，但通常不推荐在生产环境中使用固定种子
                }
            }
        }

        debug!("RandomRouter initialized successfully");
        Ok(())
    }

    async fn select_backend(
        &self,
        _context: &RoutingContext,
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

        // 随机选择
        let index = (rand::random::<f64>() * available_backends.len() as f64) as usize;
        let selected = available_backends[index].clone();

        // 更新统计信息
        {
            let mut stats = self.stats.write().await;
            stats.total_requests += 1;
            stats.successful_requests += 1;
        }

        debug!(
            "RandomRouter selected backend: {} (index: {})",
            selected.id, index
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
            "RandomRouter updated with {} backends",
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
                "RandomRouter: Backend '{}' health changed to {}",
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
        debug!("RandomRouter shutting down");

        // 清理资源
        {
            let mut backends = self.backends.write().await;
            backends.clear();
        }

        debug!("RandomRouter shutdown completed");
        Ok(())
    }
}

impl Default for RandomRouter {
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
    async fn test_random_selection() {
        let mut router = RandomRouter::new();
        router.initialize(serde_json::json!({})).await.unwrap();

        let backends = vec![
            BackendInfo::new("backend1".to_string(), "127.0.0.1:8001".parse().unwrap()),
            BackendInfo::new("backend2".to_string(), "127.0.0.1:8002".parse().unwrap()),
            BackendInfo::new("backend3".to_string(), "127.0.0.1:8003".parse().unwrap()),
        ];

        let context = RoutingContext::new(
            "127.0.0.1:12345".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
            Protocol::Tcp,
        );

        // 测试多次选择，验证随机性
        let mut selections = HashMap::new();
        for _ in 0..1000 {
            if let Some(selected) = router.select_backend(&context, &backends).await.unwrap() {
                *selections.entry(selected.id).or_insert(0) += 1;
            }
        }

        // 验证所有后端都被选中过
        assert_eq!(selections.len(), 3);

        // 验证分布相对均匀（允许一定的偏差）
        for (backend_id, count) in &selections {
            println!("Backend {}: {} selections", backend_id, count);
            // 期望每个后端大约被选中333次，允许20%的偏差
            assert!(
                *count > 200 && *count < 500,
                "Backend {} selection count {} is not within expected range",
                backend_id,
                count
            );
        }
    }

    #[tokio::test]
    async fn test_single_backend() {
        let mut router = RandomRouter::new();
        router.initialize(serde_json::json!({})).await.unwrap();

        let backends = vec![BackendInfo::new(
            "backend1".to_string(),
            "127.0.0.1:8001".parse().unwrap(),
        )];

        let context = RoutingContext::new(
            "127.0.0.1:12345".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
            Protocol::Tcp,
        );

        // 只有一个后端时，应该总是选择它
        for _ in 0..10 {
            let selected = router
                .select_backend(&context, &backends)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(selected.id, "backend1");
        }
    }

    #[tokio::test]
    async fn test_skip_unhealthy_backends() {
        let mut router = RandomRouter::new();
        router.initialize(serde_json::json!({})).await.unwrap();

        let mut backends = vec![
            BackendInfo::new("backend1".to_string(), "127.0.0.1:8001".parse().unwrap()),
            BackendInfo::new("backend2".to_string(), "127.0.0.1:8002".parse().unwrap()),
            BackendInfo::new("backend3".to_string(), "127.0.0.1:8003".parse().unwrap()),
        ];

        // 设置第二个后端为不健康
        backends[1].healthy = false;

        let context = RoutingContext::new(
            "127.0.0.1:12345".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
            Protocol::Tcp,
        );

        // 测试多次选择，验证不会选择不健康的后端
        let mut selections = HashMap::new();
        for _ in 0..100 {
            if let Some(selected) = router.select_backend(&context, &backends).await.unwrap() {
                *selections.entry(selected.id).or_insert(0) += 1;
            }
        }

        // 应该只选择健康的后端
        assert_eq!(selections.len(), 2);
        assert!(selections.contains_key("backend1"));
        assert!(selections.contains_key("backend3"));
        assert!(!selections.contains_key("backend2"));
    }

    #[tokio::test]
    async fn test_empty_backends() {
        let mut router = RandomRouter::new();
        router.initialize(serde_json::json!({})).await.unwrap();

        let backends = vec![];
        let context = RoutingContext::new(
            "127.0.0.1:12345".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
            Protocol::Tcp,
        );

        let selected = router.select_backend(&context, &backends).await.unwrap();
        assert!(selected.is_none());
    }
}
