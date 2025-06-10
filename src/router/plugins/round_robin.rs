use super::super::{BackendInfo, Router, RouterStats, RoutingContext};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::RwLock;
use tracing::debug;

/// 轮询路由器 - 按顺序轮流选择后端服务器
pub struct RoundRobinRouter {
    name: String,
    version: String,
    counter: Arc<AtomicUsize>,
    backends: Arc<RwLock<Vec<BackendInfo>>>,
    stats: Arc<RwLock<RouterStats>>,
}

impl RoundRobinRouter {
    pub fn new() -> Self {
        Self {
            name: "round_robin".to_string(),
            version: "1.0.0".to_string(),
            counter: Arc::new(AtomicUsize::new(0)),
            backends: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(RouterStats {
                name: "round_robin".to_string(),
                ..Default::default()
            })),
        }
    }
}

#[async_trait]
impl Router for RoundRobinRouter {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn initialize(&mut self, config: Value) -> Result<()> {
        debug!("Initializing RoundRobinRouter with config: {}", config);

        // 轮询路由器通常不需要特殊配置，但可以在这里处理一些选项
        if let Some(obj) = config.as_object() {
            // 可以添加一些配置选项，比如是否跳过不健康的服务器等
            if let Some(skip_unhealthy) = obj.get("skip_unhealthy") {
                if skip_unhealthy.as_bool().unwrap_or(true) {
                    debug!("RoundRobinRouter will skip unhealthy backends");
                }
            }
        }

        debug!("RoundRobinRouter initialized successfully");
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

        // 轮询选择
        let index = self.counter.fetch_add(1, Ordering::Relaxed) % available_backends.len();
        let selected = available_backends[index].clone();

        // 更新统计信息
        {
            let mut stats = self.stats.write().await;
            stats.total_requests += 1;
            stats.successful_requests += 1;
        }

        debug!(
            "RoundRobinRouter selected backend: {} (index: {})",
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
            "RoundRobinRouter updated with {} backends",
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
                "RoundRobinRouter: Backend '{}' health changed to {}",
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
        debug!("RoundRobinRouter shutting down");

        // 清理资源
        {
            let mut backends = self.backends.write().await;
            backends.clear();
        }

        // 重置计数器
        self.counter.store(0, Ordering::Relaxed);

        debug!("RoundRobinRouter shutdown completed");
        Ok(())
    }
}

impl Default for RoundRobinRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::{Protocol, RoutingContext};

    #[tokio::test]
    async fn test_round_robin_selection() {
        let mut router = RoundRobinRouter::new();
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

        // 测试轮询选择
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
        let selected4 = router
            .select_backend(&context, &backends)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(selected1.id, "backend1");
        assert_eq!(selected2.id, "backend2");
        assert_eq!(selected3.id, "backend3");
        assert_eq!(selected4.id, "backend1"); // 应该回到第一个
    }

    #[tokio::test]
    async fn test_skip_unhealthy_backends() {
        let mut router = RoundRobinRouter::new();
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

        // 测试跳过不健康的后端
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

        assert_eq!(selected1.id, "backend1");
        assert_eq!(selected2.id, "backend3");
        assert_eq!(selected3.id, "backend1"); // 应该跳过backend2
    }

    #[tokio::test]
    async fn test_empty_backends() {
        let mut router = RoundRobinRouter::new();
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
