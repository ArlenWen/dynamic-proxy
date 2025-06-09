use prometheus::{
    Counter, Histogram, HistogramOpts, IntCounter, IntGauge, Opts, Registry,
};
use std::sync::Arc;
use anyhow::Result;

#[derive(Clone)]
pub struct ProxyMetrics {
    pub registry: Arc<Registry>,
    
    // 连接指标
    pub tcp_connections_total: IntCounter,
    pub udp_packets_total: IntCounter,
    pub active_connections: IntGauge,
    
    // 流量指标
    pub bytes_transferred_total: Counter,
    pub requests_total: IntCounter,
    pub errors_total: IntCounter,
    
    // 延迟指标
    pub request_duration: Histogram,
    pub backend_response_time: Histogram,
    
    // TLS指标
    pub tls_handshakes_total: IntCounter,
    pub tls_errors_total: IntCounter,
    
    // 路由指标
    pub routing_decisions_total: IntCounter,
    pub plugin_executions_total: IntCounter,
}

impl ProxyMetrics {
    pub fn new() -> Result<Self> {
        let registry = Arc::new(Registry::new());

        let tcp_connections_total = IntCounter::with_opts(Opts::new(
            "proxy_tcp_connections_total",
            "Total number of TCP connections handled",
        ))?;

        let udp_packets_total = IntCounter::with_opts(Opts::new(
            "proxy_udp_packets_total",
            "Total number of UDP packets handled",
        ))?;

        let active_connections = IntGauge::with_opts(Opts::new(
            "proxy_active_connections",
            "Current number of active connections",
        ))?;

        let bytes_transferred_total = Counter::with_opts(Opts::new(
            "proxy_bytes_transferred_total",
            "Total bytes transferred through proxy",
        ))?;

        let requests_total = IntCounter::with_opts(Opts::new(
            "proxy_requests_total",
            "Total number of requests processed",
        ))?;

        let errors_total = IntCounter::with_opts(Opts::new(
            "proxy_errors_total",
            "Total number of errors encountered",
        ))?;

        let request_duration = Histogram::with_opts(HistogramOpts::new(
            "proxy_request_duration_seconds",
            "Request processing duration in seconds",
        ))?;

        let backend_response_time = Histogram::with_opts(HistogramOpts::new(
            "proxy_backend_response_time_seconds",
            "Backend response time in seconds",
        ))?;

        let tls_handshakes_total = IntCounter::with_opts(Opts::new(
            "proxy_tls_handshakes_total",
            "Total number of TLS handshakes",
        ))?;

        let tls_errors_total = IntCounter::with_opts(Opts::new(
            "proxy_tls_errors_total",
            "Total number of TLS errors",
        ))?;

        let routing_decisions_total = IntCounter::with_opts(Opts::new(
            "proxy_routing_decisions_total",
            "Total number of routing decisions made",
        ))?;

        let plugin_executions_total = IntCounter::with_opts(Opts::new(
            "proxy_plugin_executions_total",
            "Total number of plugin executions",
        ))?;

        // 注册所有指标
        registry.register(Box::new(tcp_connections_total.clone()))?;
        registry.register(Box::new(udp_packets_total.clone()))?;
        registry.register(Box::new(active_connections.clone()))?;
        registry.register(Box::new(bytes_transferred_total.clone()))?;
        registry.register(Box::new(requests_total.clone()))?;
        registry.register(Box::new(errors_total.clone()))?;
        registry.register(Box::new(request_duration.clone()))?;
        registry.register(Box::new(backend_response_time.clone()))?;
        registry.register(Box::new(tls_handshakes_total.clone()))?;
        registry.register(Box::new(tls_errors_total.clone()))?;
        registry.register(Box::new(routing_decisions_total.clone()))?;
        registry.register(Box::new(plugin_executions_total.clone()))?;

        Ok(Self {
            registry,
            tcp_connections_total,
            udp_packets_total,
            active_connections,
            bytes_transferred_total,
            requests_total,
            errors_total,
            request_duration,
            backend_response_time,
            tls_handshakes_total,
            tls_errors_total,
            routing_decisions_total,
            plugin_executions_total,
        })
    }

    pub fn export_metrics(&self) -> String {
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();
        encoder.encode_to_string(&metric_families).unwrap_or_default()
    }
}
