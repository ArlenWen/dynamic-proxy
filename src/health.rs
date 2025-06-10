use crate::config::{Backend, HealthCheckConfig};
use crate::router::RouterManager;
use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio::time::{interval, timeout};
use tracing::{debug, error, info, warn};

/// 健康检查器 - 负责监控后端服务器的健康状态
pub struct HealthChecker {
    config: HealthCheckConfig,
    router_manager: Arc<RouterManager>,
    backend_states: Arc<RwLock<HashMap<String, BackendHealthState>>>,
    running: Arc<RwLock<bool>>,
}

/// 后端健康状态
#[derive(Debug, Clone)]
struct BackendHealthState {
    backend_id: String,
    rule_name: String,
    address: SocketAddr,
    healthy: bool,
    consecutive_failures: u32,
    consecutive_successes: u32,
    last_check: Option<Instant>,
    last_success: Option<Instant>,
    last_failure: Option<Instant>,
    response_time: Option<Duration>,
}

impl HealthChecker {
    pub fn new(config: HealthCheckConfig, router_manager: Arc<RouterManager>) -> Self {
        Self {
            config,
            router_manager,
            backend_states: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// 启动健康检查
    pub async fn start(&self) -> Result<()> {
        if !self.config.enabled {
            info!("Health checker is disabled");
            return Ok(());
        }

        {
            let mut running = self.running.write().await;
            if *running {
                warn!("Health checker is already running");
                return Ok(());
            }
            *running = true;
        }

        let interval_secs = self.config.interval.unwrap_or(30);
        info!(
            "Starting health checker with interval: {}s",
            interval_secs
        );

        let config = self.config.clone();
        let router_manager = self.router_manager.clone();
        let backend_states = self.backend_states.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(interval_secs));

            loop {
                interval.tick().await;

                // 检查是否应该停止
                {
                    let running_guard = running.read().await;
                    if !*running_guard {
                        break;
                    }
                }

                // 执行健康检查
                if let Err(e) =
                    Self::perform_health_checks(&config, &router_manager, &backend_states).await
                {
                    error!("Health check failed: {}", e);
                }
            }

            info!("Health checker stopped");
        });

        Ok(())
    }

    /// 停止健康检查
    pub async fn stop(&self) -> Result<()> {
        let mut running = self.running.write().await;
        *running = false;
        info!("Health checker stop requested");
        Ok(())
    }

    /// 添加后端进行健康检查
    pub async fn add_backend(&self, rule_name: String, backend: &Backend) -> Result<()> {
        let mut states = self.backend_states.write().await;
        let key = format!("{}:{}", rule_name, backend.id);

        states.insert(
            key,
            BackendHealthState {
                backend_id: backend.id.clone(),
                rule_name,
                address: backend.address,
                healthy: true, // 初始假设健康
                consecutive_failures: 0,
                consecutive_successes: 0,
                last_check: None,
                last_success: None,
                last_failure: None,
                response_time: None,
            },
        );

        debug!("Added backend '{}' for health checking", backend.id);
        Ok(())
    }

    /// 移除后端健康检查
    pub async fn remove_backend(&self, rule_name: &str, backend_id: &str) -> Result<()> {
        let mut states = self.backend_states.write().await;
        let key = format!("{}:{}", rule_name, backend_id);
        states.remove(&key);
        debug!("Removed backend '{}' from health checking", backend_id);
        Ok(())
    }

    /// 获取后端健康状态
    pub async fn get_backend_health(&self, rule_name: &str, backend_id: &str) -> Option<bool> {
        let states = self.backend_states.read().await;
        let key = format!("{}:{}", rule_name, backend_id);
        states.get(&key).map(|state| state.healthy)
    }

    /// 获取所有后端健康状态
    pub async fn get_all_health_states(&self) -> HashMap<String, bool> {
        let states = self.backend_states.read().await;
        states
            .iter()
            .map(|(key, state)| (key.clone(), state.healthy))
            .collect()
    }

    /// 执行健康检查
    async fn perform_health_checks(
        config: &HealthCheckConfig,
        router_manager: &Arc<RouterManager>,
        backend_states: &Arc<RwLock<HashMap<String, BackendHealthState>>>,
    ) -> Result<()> {
        let states = {
            let states_guard = backend_states.read().await;
            states_guard.clone()
        };

        let mut check_tasks = Vec::new();

        for (key, state) in states {
            let config = config.clone();
            let router_manager = router_manager.clone();
            let backend_states = backend_states.clone();

            let task = tokio::spawn(async move {
                Self::check_single_backend(&config, &router_manager, &backend_states, key, state)
                    .await
            });

            check_tasks.push(task);
        }

        // 等待所有检查完成
        for task in check_tasks {
            if let Err(e) = task.await {
                error!("Health check task failed: {}", e);
            }
        }

        Ok(())
    }

    /// 检查单个后端
    async fn check_single_backend(
        config: &HealthCheckConfig,
        router_manager: &Arc<RouterManager>,
        backend_states: &Arc<RwLock<HashMap<String, BackendHealthState>>>,
        key: String,
        mut state: BackendHealthState,
    ) -> Result<()> {
        let start_time = Instant::now();
        let mut check_success = false;

        // 执行多次重试
        let retries = config.retries.unwrap_or(3);
        let timeout = config.timeout.unwrap_or(5);
        for attempt in 1..=retries {
            match Self::tcp_health_check(state.address, timeout).await {
                Ok(response_time) => {
                    check_success = true;
                    state.response_time = Some(response_time);
                    debug!(
                        "Health check succeeded for {} (attempt {}, response time: {:?})",
                        state.backend_id, attempt, response_time
                    );
                    break;
                }
                Err(e) => {
                    debug!(
                        "Health check failed for {} (attempt {}): {}",
                        state.backend_id, attempt, e
                    );
                    if attempt == retries {
                        error!(
                            "Health check failed for {} after {} retries",
                            state.backend_id, retries
                        );
                    }
                }
            }
        }

        // 更新状态
        state.last_check = Some(start_time);

        if check_success {
            state.consecutive_successes += 1;
            state.consecutive_failures = 0;
            state.last_success = Some(start_time);

            // 检查是否应该标记为健康
            let success_threshold = config.success_threshold.unwrap_or(2);
            if !state.healthy && state.consecutive_successes >= success_threshold {
                state.healthy = true;
                info!("Backend '{}' marked as healthy", state.backend_id);

                // 通知路由管理器
                if let Err(e) = router_manager
                    .update_backend_health(&state.rule_name, &state.backend_id, true)
                    .await
                {
                    error!("Failed to update backend health in router manager: {}", e);
                }
            }
        } else {
            state.consecutive_failures += 1;
            state.consecutive_successes = 0;
            state.last_failure = Some(start_time);

            // 检查是否应该标记为不健康
            let failure_threshold = config.failure_threshold.unwrap_or(3);
            if state.healthy && state.consecutive_failures >= failure_threshold {
                state.healthy = false;
                warn!("Backend '{}' marked as unhealthy", state.backend_id);

                // 通知路由管理器
                if let Err(e) = router_manager
                    .update_backend_health(&state.rule_name, &state.backend_id, false)
                    .await
                {
                    error!("Failed to update backend health in router manager: {}", e);
                }
            }
        }

        // 更新状态
        {
            let mut states = backend_states.write().await;
            states.insert(key, state);
        }

        Ok(())
    }

    /// TCP健康检查
    async fn tcp_health_check(address: SocketAddr, timeout_secs: u64) -> Result<Duration> {
        let start_time = Instant::now();

        let connect_future = TcpStream::connect(address);
        let timeout_duration = Duration::from_secs(timeout_secs);

        match timeout(timeout_duration, connect_future).await {
            Ok(Ok(_stream)) => {
                let response_time = start_time.elapsed();
                Ok(response_time)
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("Connection failed: {}", e)),
            Err(_) => Err(anyhow::anyhow!(
                "Connection timeout after {}s",
                timeout_secs
            )),
        }
    }

    /// 获取健康检查统计信息
    pub async fn get_stats(&self) -> HealthCheckStats {
        let states = self.backend_states.read().await;

        let total_backends = states.len();
        let healthy_backends = states.values().filter(|s| s.healthy).count();
        let unhealthy_backends = total_backends - healthy_backends;

        let avg_response_time = {
            let response_times: Vec<Duration> =
                states.values().filter_map(|s| s.response_time).collect();

            if response_times.is_empty() {
                Duration::from_millis(0)
            } else {
                let total_ms: u64 = response_times.iter().map(|d| d.as_millis() as u64).sum();
                Duration::from_millis(total_ms / response_times.len() as u64)
            }
        };

        HealthCheckStats {
            total_backends,
            healthy_backends,
            unhealthy_backends,
            avg_response_time,
            last_check: states.values().filter_map(|s| s.last_check).max(),
        }
    }
}

/// 健康检查统计信息
#[derive(Debug, Clone)]
pub struct HealthCheckStats {
    pub total_backends: usize,
    pub healthy_backends: usize,
    pub unhealthy_backends: usize,
    pub avg_response_time: Duration,
    pub last_check: Option<Instant>,
}

#[allow(dead_code)]
impl BackendHealthState {
    pub fn is_healthy(&self) -> bool {
        self.healthy
    }

    pub fn get_response_time(&self) -> Option<Duration> {
        self.response_time
    }

    pub fn get_consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    pub fn get_consecutive_successes(&self) -> u32 {
        self.consecutive_successes
    }
}
