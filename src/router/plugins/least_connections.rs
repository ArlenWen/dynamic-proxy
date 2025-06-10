use super::super::{BackendInfo, Router, RouterStats, RoutingContext};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// 最少连接路由器 - 选择当前连接数最少的后端服务器
pub struct LeastConnectionsRouter {
    name: String,
    version: String,
    backends: Arc<RwLock<Vec<BackendInfo>>>,
    stats: Arc<RwLock<RouterStats>>,
}

impl LeastConnectionsRouter {
    pub fn new() -> Self {
        Self {
            name: "least_connections".to_string(),
            version: "1.0.0".to_string(),
            backends: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(RouterStats {
                name: "least_connections".to_string(),
                ..Default::default()
            })),
        }
    }
}

#[async_trait]
impl Router for LeastConnectionsRouter {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn initialize(&mut self, config: Value) -> Result<()> {
        debug!(
            "Initializing LeastConnectionsRouter with config: {}",
            config
        );

        // 最少连接路由器通常不需要特殊配置
        if let Some(obj) = config.as_object() {
            // 可以添加一些配置选项，比如权重因子等
            if let Some(weight_factor) = obj.get("weight_factor") {
                if let Some(factor) = weight_factor.as_f64() {
                    debug!("LeastConnectionsRouter weight_factor: {}", factor);
                    // 这里可以存储权重因子用于后续计算
                }
            }
        }

        debug!("LeastConnectionsRouter initialized successfully");
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

        // 找到连接数最少的后端
        let selected = available_backends
            .iter()
            .min_by_key(|b| b.current_connections)
            .map(|b| (*b).clone());

        // 更新统计信息
        {
            let mut stats = self.stats.write().await;
            stats.total_requests += 1;
            if selected.is_some() {
                stats.successful_requests += 1;
            } else {
                stats.failed_requests += 1;
            }
        }

        if let Some(ref backend) = selected {
            debug!(
                "LeastConnectionsRouter selected backend: {} (connections: {})",
                backend.id, backend.current_connections
            );
        }

        Ok(selected)
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
            stats.active_connections = current_backends.iter().map(|b| b.current_connections).sum();
        }

        debug!(
            "LeastConnectionsRouter updated with {} backends",
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
                "LeastConnectionsRouter: Backend '{}' health changed to {}",
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
        debug!("LeastConnectionsRouter shutting down");

        // 清理资源
        {
            let mut backends = self.backends.write().await;
            backends.clear();
        }

        debug!("LeastConnectionsRouter shutdown completed");
        Ok(())
    }
}

impl Default for LeastConnectionsRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::{Protocol, RoutingContext};

    #[tokio::test]
    async fn test_least_connections_selection() {
        let mut router = LeastConnectionsRouter::new();
        router.initialize(serde_json::json!({})).await.unwrap();

        let mut backends = vec![
            BackendInfo::new("backend1".to_string(), "127.0.0.1:8001".parse().unwrap()),
            BackendInfo::new("backend2".to_string(), "127.0.0.1:8002".parse().unwrap()),
            BackendInfo::new("backend3".to_string(), "127.0.0.1:8003".parse().unwrap()),
        ];

        // 设置不同的连接数
        backends[0].current_connections = 5;
        backends[1].current_connections = 2;
        backends[2].current_connections = 8;

        let context = RoutingContext::new(
            "127.0.0.1:12345".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
            Protocol::Tcp,
        );

        // 应该选择连接数最少的backend2
        let selected = router
            .select_backend(&context, &backends)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(selected.id, "backend2");
        assert_eq!(selected.current_connections, 2);
    }

    #[tokio::test]
    async fn test_equal_connections() {
        let mut router = LeastConnectionsRouter::new();
        router.initialize(serde_json::json!({})).await.unwrap();

        let backends = vec![
            BackendInfo::new("backend1".to_string(), "127.0.0.1:8001".parse().unwrap()),
            BackendInfo::new("backend2".to_string(), "127.0.0.1:8002".parse().unwrap()),
            BackendInfo::new("backend3".to_string(), "127.0.0.1:8003".parse().unwrap()),
        ];

        // 所有后端连接数相同（默认为0）
        let context = RoutingContext::new(
            "127.0.0.1:12345".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
            Protocol::Tcp,
        );

        // 应该选择第一个（min_by_key的行为）
        let selected = router
            .select_backend(&context, &backends)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(selected.id, "backend1");
    }

    #[tokio::test]
    async fn test_skip_unhealthy_backends() {
        let mut router = LeastConnectionsRouter::new();
        router.initialize(serde_json::json!({})).await.unwrap();

        let mut backends = vec![
            BackendInfo::new("backend1".to_string(), "127.0.0.1:8001".parse().unwrap()),
            BackendInfo::new("backend2".to_string(), "127.0.0.1:8002".parse().unwrap()),
            BackendInfo::new("backend3".to_string(), "127.0.0.1:8003".parse().unwrap()),
        ];

        // 设置连接数，但第一个（连接数最少的）不健康
        backends[0].current_connections = 1;
        backends[0].healthy = false;
        backends[1].current_connections = 3;
        backends[2].current_connections = 5;

        let context = RoutingContext::new(
            "127.0.0.1:12345".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
            Protocol::Tcp,
        );

        // 应该跳过不健康的backend1，选择backend2
        let selected = router
            .select_backend(&context, &backends)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(selected.id, "backend2");
        assert_eq!(selected.current_connections, 3);
    }
}
