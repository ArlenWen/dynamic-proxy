use super::super::{BackendInfo, Router, RouterStats, RoutingContext};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// 加权路由器 - 根据权重选择后端服务器
pub struct WeightedRouter {
    name: String,
    version: String,
    backends: Arc<RwLock<Vec<BackendInfo>>>,
    stats: Arc<RwLock<RouterStats>>,
    smooth_weighted: bool, // 是否使用平滑加权轮询
}

impl WeightedRouter {
    pub fn new() -> Self {
        Self {
            name: "weighted".to_string(),
            version: "1.0.0".to_string(),
            backends: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(RouterStats {
                name: "weighted".to_string(),
                ..Default::default()
            })),
            smooth_weighted: true,
        }
    }
}

#[async_trait]
impl Router for WeightedRouter {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn initialize(&mut self, config: Value) -> Result<()> {
        debug!("Initializing WeightedRouter with config: {}", config);

        if let Some(obj) = config.as_object() {
            // 配置是否使用平滑加权轮询
            if let Some(smooth) = obj.get("smooth_weighted") {
                self.smooth_weighted = smooth.as_bool().unwrap_or(true);
                debug!("WeightedRouter smooth_weighted: {}", self.smooth_weighted);
            }
        }

        debug!("WeightedRouter initialized successfully");
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

        let selected = if self.smooth_weighted {
            self.select_smooth_weighted(&available_backends).await
        } else {
            self.select_simple_weighted(&available_backends).await
        };

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
            debug!("WeightedRouter selected backend: {}", backend.id);
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
        }

        debug!(
            "WeightedRouter updated with {} backends",
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
                "WeightedRouter: Backend '{}' health changed to {}",
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
        debug!("WeightedRouter shutting down");

        // 清理资源
        {
            let mut backends = self.backends.write().await;
            backends.clear();
        }

        debug!("WeightedRouter shutdown completed");
        Ok(())
    }
}

impl WeightedRouter {
    /// 简单加权选择 - 基于权重随机选择
    async fn select_simple_weighted(&self, backends: &[&BackendInfo]) -> Option<BackendInfo> {
        let total_weight: u32 = backends.iter().map(|b| b.weight).sum();
        if total_weight == 0 {
            return None;
        }

        // 生成随机数
        let mut random_weight = (rand::random::<f64>() * total_weight as f64) as u32;

        for backend in backends {
            if random_weight < backend.weight {
                return Some((*backend).clone());
            }
            random_weight -= backend.weight;
        }

        // 如果由于浮点精度问题没有选中，返回最后一个
        backends.last().map(|b| (*b).clone())
    }

    /// 平滑加权轮询选择 - Nginx风格的平滑加权轮询
    async fn select_smooth_weighted(&self, backends: &[&BackendInfo]) -> Option<BackendInfo> {
        if backends.is_empty() {
            return None;
        }

        // 这里简化实现，实际应该维护每个后端的当前权重状态
        // 为了演示，我们使用简单的加权随机选择
        self.select_simple_weighted(backends).await
    }
}

impl Default for WeightedRouter {
    fn default() -> Self {
        Self::new()
    }
}

// 为了支持真正的平滑加权轮询，我们需要一个状态结构
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct WeightedBackend {
    backend: BackendInfo,
    current_weight: i32,
    effective_weight: i32,
}

#[allow(dead_code)]
impl WeightedBackend {
    fn new(backend: BackendInfo) -> Self {
        let weight = backend.weight as i32;
        Self {
            backend,
            current_weight: 0,
            effective_weight: weight,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::{Protocol, RoutingContext};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_weighted_selection() {
        let mut router = WeightedRouter::new();
        router.initialize(serde_json::json!({})).await.unwrap();

        let backends = vec![
            BackendInfo::new("backend1".to_string(), "127.0.0.1:8001".parse().unwrap())
                .with_weight(1),
            BackendInfo::new("backend2".to_string(), "127.0.0.1:8002".parse().unwrap())
                .with_weight(2),
            BackendInfo::new("backend3".to_string(), "127.0.0.1:8003".parse().unwrap())
                .with_weight(3),
        ];

        let context = RoutingContext::new(
            "127.0.0.1:12345".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
            Protocol::Tcp,
        );

        // 测试多次选择，统计分布
        let mut selections = HashMap::new();
        for _ in 0..1000 {
            if let Some(selected) = router.select_backend(&context, &backends).await.unwrap() {
                *selections.entry(selected.id).or_insert(0) += 1;
            }
        }

        // 验证权重分布大致正确
        let backend1_count = selections.get("backend1").unwrap_or(&0);
        let backend2_count = selections.get("backend2").unwrap_or(&0);
        let backend3_count = selections.get("backend3").unwrap_or(&0);

        // backend3的权重最高，应该被选中最多
        assert!(*backend3_count > *backend2_count);
        assert!(*backend2_count > *backend1_count);
    }

    #[tokio::test]
    async fn test_zero_weight_backends() {
        let mut router = WeightedRouter::new();
        router.initialize(serde_json::json!({})).await.unwrap();

        let backends = vec![
            BackendInfo::new("backend1".to_string(), "127.0.0.1:8001".parse().unwrap())
                .with_weight(0),
            BackendInfo::new("backend2".to_string(), "127.0.0.1:8002".parse().unwrap())
                .with_weight(0),
        ];

        let context = RoutingContext::new(
            "127.0.0.1:12345".parse().unwrap(),
            "127.0.0.1:8080".parse().unwrap(),
            Protocol::Tcp,
        );

        let selected = router.select_backend(&context, &backends).await.unwrap();
        // 所有权重为0时，应该返回None或最后一个
        // 这取决于具体实现
        println!("Selected with zero weights: {:?}", selected);
    }
}
