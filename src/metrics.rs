use crate::config::MetricsConfig;
use anyhow::Result;
use prometheus::{
    Gauge, GaugeVec, Histogram, HistogramVec, IntCounter, IntCounterVec, IntGauge, IntGaugeVec,
    Registry,
};
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

/// 请求记录参数
#[derive(Debug)]
pub struct RequestMetrics {
    pub protocol: String,
    pub rule: String,
    pub backend: String,
    pub status: String,
    pub duration: std::time::Duration,
    pub request_size: u64,
    pub response_size: u64,
}

impl RequestMetrics {
    pub fn new(
        protocol: &str,
        rule: &str,
        backend: &str,
        status: &str,
        duration: std::time::Duration,
        request_size: u64,
        response_size: u64,
    ) -> Self {
        Self {
            protocol: protocol.to_string(),
            rule: rule.to_string(),
            backend: backend.to_string(),
            status: status.to_string(),
            duration,
            request_size,
            response_size,
        }
    }
}

/// 监控指标收集器
pub struct MetricsCollector {
    registry: Registry,

    // 连接指标
    pub active_connections: IntGauge,
    pub total_connections: IntCounter,
    pub connection_duration: Histogram,

    // 请求指标
    pub requests_total: IntCounterVec,
    pub request_duration: HistogramVec,
    pub request_size: HistogramVec,
    pub response_size: HistogramVec,

    // 后端指标
    pub backend_connections: IntGaugeVec,
    pub backend_requests: IntCounterVec,
    pub backend_response_time: HistogramVec,
    pub backend_health: GaugeVec,

    // 路由器指标
    pub router_selections: IntCounterVec,
    pub router_errors: IntCounterVec,

    // 系统指标
    pub memory_usage: Gauge,
    pub cpu_usage: Gauge,
    pub goroutines: IntGauge,

    // 错误指标
    pub errors_total: IntCounterVec,

    config: MetricsConfig,
    start_time: Instant,
}

impl MetricsCollector {
    pub fn new(config: MetricsConfig) -> Result<Self> {
        let registry = Registry::new();

        // 连接指标
        let active_connections =
            IntGauge::new("proxy_active_connections", "Number of active connections")?;
        registry.register(Box::new(active_connections.clone()))?;

        let total_connections =
            IntCounter::new("proxy_connections_total", "Total number of connections")?;
        registry.register(Box::new(total_connections.clone()))?;

        let connection_duration = Histogram::with_opts(
            prometheus::HistogramOpts::new(
                "proxy_connection_duration_seconds",
                "Connection duration in seconds",
            )
            .buckets(vec![0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0]),
        )?;
        registry.register(Box::new(connection_duration.clone()))?;

        // 请求指标
        let requests_total = IntCounterVec::new(
            prometheus::Opts::new("proxy_requests_total", "Total number of requests"),
            &["protocol", "rule", "backend", "status"],
        )?;
        registry.register(Box::new(requests_total.clone()))?;

        let request_duration = HistogramVec::new(
            prometheus::HistogramOpts::new(
                "proxy_request_duration_seconds",
                "Request duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
            &["protocol", "rule", "backend"],
        )?;
        registry.register(Box::new(request_duration.clone()))?;

        let request_size = HistogramVec::new(
            prometheus::HistogramOpts::new("proxy_request_size_bytes", "Request size in bytes")
                .buckets(prometheus::exponential_buckets(64.0, 2.0, 16)?),
            &["protocol", "rule"],
        )?;
        registry.register(Box::new(request_size.clone()))?;

        let response_size = HistogramVec::new(
            prometheus::HistogramOpts::new("proxy_response_size_bytes", "Response size in bytes")
                .buckets(prometheus::exponential_buckets(64.0, 2.0, 16)?),
            &["protocol", "rule"],
        )?;
        registry.register(Box::new(response_size.clone()))?;

        // 后端指标
        let backend_connections = IntGaugeVec::new(
            prometheus::Opts::new(
                "proxy_backend_connections",
                "Number of connections to backend",
            ),
            &["backend", "rule"],
        )?;
        registry.register(Box::new(backend_connections.clone()))?;

        let backend_requests = IntCounterVec::new(
            prometheus::Opts::new("proxy_backend_requests_total", "Total requests to backend"),
            &["backend", "rule", "status"],
        )?;
        registry.register(Box::new(backend_requests.clone()))?;

        let backend_response_time = HistogramVec::new(
            prometheus::HistogramOpts::new(
                "proxy_backend_response_time_seconds",
                "Backend response time in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
            &["backend", "rule"],
        )?;
        registry.register(Box::new(backend_response_time.clone()))?;

        let backend_health = GaugeVec::new(
            prometheus::Opts::new(
                "proxy_backend_health",
                "Backend health status (1 = healthy, 0 = unhealthy)",
            ),
            &["backend", "rule"],
        )?;
        registry.register(Box::new(backend_health.clone()))?;

        // 路由器指标
        let router_selections = IntCounterVec::new(
            prometheus::Opts::new("proxy_router_selections_total", "Total router selections"),
            &["router", "rule"],
        )?;
        registry.register(Box::new(router_selections.clone()))?;

        let router_errors = IntCounterVec::new(
            prometheus::Opts::new("proxy_router_errors_total", "Total router errors"),
            &["router", "rule", "error_type"],
        )?;
        registry.register(Box::new(router_errors.clone()))?;

        // 系统指标
        let memory_usage = Gauge::new("proxy_memory_usage_bytes", "Memory usage in bytes")?;
        registry.register(Box::new(memory_usage.clone()))?;

        let cpu_usage = Gauge::new("proxy_cpu_usage_percent", "CPU usage percentage")?;
        registry.register(Box::new(cpu_usage.clone()))?;

        let goroutines = IntGauge::new("proxy_goroutines", "Number of goroutines")?;
        registry.register(Box::new(goroutines.clone()))?;

        // 错误指标
        let errors_total = IntCounterVec::new(
            prometheus::Opts::new("proxy_errors_total", "Total errors"),
            &["type", "component"],
        )?;
        registry.register(Box::new(errors_total.clone()))?;

        Ok(Self {
            registry,
            active_connections,
            total_connections,
            connection_duration,
            requests_total,
            request_duration,
            request_size,
            response_size,
            backend_connections,
            backend_requests,
            backend_response_time,
            backend_health,
            router_selections,
            router_errors,
            memory_usage,
            cpu_usage,
            goroutines,
            errors_total,
            config,
            start_time: Instant::now(),
        })
    }

    /// 获取Prometheus注册表
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// 记录新连接
    pub fn record_new_connection(&self) {
        self.active_connections.inc();
        self.total_connections.inc();
    }

    /// 记录连接关闭
    pub fn record_connection_closed(&self, duration: std::time::Duration) {
        self.active_connections.dec();
        self.connection_duration.observe(duration.as_secs_f64());
    }

    /// 记录请求
    pub fn record_request(&self, metrics: &RequestMetrics) {
        self.requests_total
            .with_label_values(&[
                &metrics.protocol,
                &metrics.rule,
                &metrics.backend,
                &metrics.status,
            ])
            .inc();

        self.request_duration
            .with_label_values(&[&metrics.protocol, &metrics.rule, &metrics.backend])
            .observe(metrics.duration.as_secs_f64());

        self.request_size
            .with_label_values(&[&metrics.protocol, &metrics.rule])
            .observe(metrics.request_size as f64);

        self.response_size
            .with_label_values(&[&metrics.protocol, &metrics.rule])
            .observe(metrics.response_size as f64);
    }

    /// 更新后端连接数
    pub fn update_backend_connections(&self, backend: &str, rule: &str, connections: i64) {
        self.backend_connections
            .with_label_values(&[backend, rule])
            .set(connections);
    }

    /// 记录后端请求
    pub fn record_backend_request(
        &self,
        backend: &str,
        rule: &str,
        status: &str,
        response_time: std::time::Duration,
    ) {
        self.backend_requests
            .with_label_values(&[backend, rule, status])
            .inc();

        self.backend_response_time
            .with_label_values(&[backend, rule])
            .observe(response_time.as_secs_f64());
    }

    /// 更新后端健康状态
    pub fn update_backend_health(&self, backend: &str, rule: &str, healthy: bool) {
        let value = if healthy { 1.0 } else { 0.0 };
        self.backend_health
            .with_label_values(&[backend, rule])
            .set(value);
    }

    /// 记录路由器选择
    pub fn record_router_selection(&self, router: &str, rule: &str) {
        self.router_selections
            .with_label_values(&[router, rule])
            .inc();
    }

    /// 记录路由器错误
    pub fn record_router_error(&self, router: &str, rule: &str, error_type: &str) {
        self.router_errors
            .with_label_values(&[router, rule, error_type])
            .inc();
    }

    /// 记录错误
    pub fn record_error(&self, error_type: &str, component: &str) {
        self.errors_total
            .with_label_values(&[error_type, component])
            .inc();
    }

    /// 更新系统指标
    pub fn update_system_metrics(&self) {
        // 这里可以实现系统指标的收集
        // 例如内存使用、CPU使用等
        // 由于这需要系统特定的实现，这里只是示例

        // 示例：更新运行时间
        let _uptime = self.start_time.elapsed().as_secs() as f64;
        // 可以添加一个运行时间指标
    }

    /// 导出指标为Prometheus格式
    pub fn export_metrics(&self) -> Result<String> {
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();
        encoder
            .encode_to_string(&metric_families)
            .map_err(|e| anyhow::anyhow!("Failed to encode metrics: {}", e))
    }

    /// 启动指标收集
    pub async fn start_collection(&self) -> Result<()> {
        if !self.config.enabled {
            info!("Metrics collection is disabled");
            return Ok(());
        }

        info!("Starting metrics collection on {}", self.config.bind);

        // 简化实现，暂时不启动HTTP服务器
        // 可以通过export_metrics()方法获取指标
        info!("Metrics collection enabled (use export_metrics() to get data)");

        Ok(())
    }
}

/// 指标收集器的简化接口
pub struct MetricsRecorder {
    collector: Arc<MetricsCollector>,
}

impl MetricsRecorder {
    pub fn new(collector: Arc<MetricsCollector>) -> Self {
        Self { collector }
    }

    pub fn connection_started(&self) {
        self.collector.record_new_connection();
    }

    pub fn connection_ended(&self, duration: std::time::Duration) {
        self.collector.record_connection_closed(duration);
    }

    pub fn request_completed(&self, metrics: &RequestMetrics) {
        self.collector.record_request(metrics);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn request_completed_simple(
        &self,
        protocol: &str,
        rule: &str,
        backend: &str,
        success: bool,
        duration: std::time::Duration,
        bytes_sent: u64,
        bytes_received: u64,
    ) {
        let status = if success { "success" } else { "error" };
        let metrics = RequestMetrics::new(
            protocol,
            rule,
            backend,
            status,
            duration,
            bytes_sent,
            bytes_received,
        );
        self.collector.record_request(&metrics);
    }

    pub fn backend_selected(&self, router: &str, rule: &str) {
        self.collector.record_router_selection(router, rule);
    }

    pub fn error_occurred(&self, error_type: &str, component: &str) {
        self.collector.record_error(error_type, component);
    }
}
