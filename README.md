# Dynamic Proxy Server

一个功能完整的动态代理服务器，基于Rust实现，支持TCP/UDP流量代理、完整的TLS支持、Redis集成和插件化架构。

## ✨ 核心特性

### 🚀 代理功能
- **TCP/UDP代理** - 高性能的TCP和UDP流量代理
- **TLS支持** - 完整的TLS支持，包括SNI检测和多证书管理
- **负载均衡** - 支持权重和健康检查的负载均衡
- **连接管理** - 智能连接池和会话管理

### 🔒 TLS增强功能
- **SNI检测** - 支持SNI提取和证书选择
- **多证书支持** - 支持基于SNI的多证书配置
- **通配符证书** - 支持通配符域名证书匹配
- **动态证书管理** - 运行时证书加载和管理

### 🔄 Redis集成
- **Redis连接** - 完整的Redis连接池和异步操作
- **后端管理** - 动态后端服务器配置和管理
- **健康检查** - 自动健康检查并更新Redis状态
- **缓存策略** - 本地缓存与Redis的双层缓存

### 🔌 路由系统
- **插件化架构** - 可扩展的路由插件系统
- **智能路由** - 支持精确匹配、前缀匹配和通配符匹配
- **动态配置** - 支持运行时配置更新
- **规则引擎** - 灵活的路由规则配置

### 📊 监控和管理
- **Prometheus指标** - 完整的指标收集和暴露
- **结构化日志** - 基于tracing的高性能日志系统
- **健康检查API** - 后端服务健康状态监控
- **优雅关闭** - 支持信号处理和优雅关闭

### 🛡️ 优雅关机系统
- **多信号支持** - 支持SIGINT、SIGTERM、SIGQUIT、SIGHUP等信号
- **连接排空** - 优雅等待现有连接完成或超时
- **资源清理** - 自动清理所有资源和后台任务
- **状态管理** - 完整的关机状态跟踪和监控

## 🚀 快速开始

### 前置要求

- Rust 1.70+
- Redis服务器
- 可选：OpenSSL/TLS证书

### 安装和运行

#### 🎯 快速演示（推荐）

使用交互式演示菜单，一键体验所有功能：

```bash
bash scripts/demo.sh
```

#### 📋 手动步骤

1. **启动Redis服务器**
```bash
redis-server
```

2. **初始化Redis后端配置**
```bash
bash scripts/setup_redis_backends.sh
```

3. **构建项目**
```bash
cargo build --release
```

4. **运行代理服务器**
```bash
# 基础演示（无Redis/TLS）
bash scripts/demo/run_basic_demo.sh

# 或增强演示（完整功能）
bash scripts/demo/run_enhanced_demo.sh

# 或直接运行主程序
cargo run --release
```

### 🧪 测试和验证

#### 测试客户端
```bash
# 在代理运行时，在另一个终端测试
bash scripts/test/run_test_client.sh
```

#### 功能测试
```bash
# 验证代码修复和编译
bash scripts/test/test_fixes.sh

# 完整功能测试
bash scripts/test_enhanced_features.sh

# 优雅关机测试
bash scripts/test_graceful_shutdown.sh
```

#### 优雅关机演示
```bash
bash scripts/demo/run_graceful_shutdown_demo.sh
```

## 📋 配置

### 基本配置

项目包含一个完整的配置文件 `config.toml`，可以直接使用或根据需要修改：

```bash
# 编辑 config.toml 文件以适应您的环境
vim config.toml
```

> **注意**:
> - `config.toml` 包含详细的配置说明和示例，建议先阅读其中的注释
> - 如果不存在 `config.toml` 文件，程序会自动使用默认配置并创建该文件
> - 生产环境建议根据实际需求调整配置参数

### 服务器配置

```toml
[server]
tcp_bind = "0.0.0.0:8080"    # TCP监听地址
udp_bind = "0.0.0.0:8081"    # UDP监听地址
buffer_size = 8192           # 缓冲区大小
max_connections = 1000       # 最大连接数
```

### 多证书TLS配置

```toml
[tls]
cert_path = "certs/server.crt"
key_path = "certs/server.key"
sni_routing = true

[[tls.certificates]]
hostname = "api.example.com"
cert_path = "certs/api.crt"
key_path = "certs/api.key"

[[tls.certificates]]
hostname = "*.example.com"
cert_path = "certs/wildcard.crt"
key_path = "certs/wildcard.key"
```

### Redis配置

```toml
[redis]
url = "redis://127.0.0.1:6379"
pool_size = 10
timeout_ms = 5000
cache_ttl_secs = 300
max_retries = 3
```

### 监控配置

```toml
[monitoring]
metrics_bind = "0.0.0.0:9090"  # 指标服务器监听地址
log_level = "info"             # 日志级别
enable_metrics = true          # 启用指标收集
```

## 🔧 Redis后端管理

### 添加后端服务器

```bash
redis-cli SET "proxy:backends:my_service" '{
  "address": "127.0.0.1:8080",
  "weight": 100,
  "healthy": true,
  "last_check": 1234567890,
  "metadata": {}
}'
```

### 查看所有后端

```bash
redis-cli KEYS "proxy:backends:*"
```

### 删除后端

```bash
redis-cli DEL "proxy:backends:service_name"
```

## 🔄 路由规则

路由规则支持多种条件和动作：

### 条件类型

- `SniPrefix`: SNI前缀匹配
- `SniSuffix`: SNI后缀匹配
- `SniExact`: SNI精确匹配
- `SourceIp`: 源IP匹配
- `DestinationPort`: 目标端口匹配
- `Protocol`: 协议匹配（tcp/udp）

### 动作类型

- `Forward`: 转发到指定后端
- `LoadBalance`: 负载均衡到多个后端
- `Reject`: 拒绝连接
- `TlsTerminate`: TLS终止并转发

## 📊 监控

### 指标端点

访问 `http://localhost:9090/metrics` 查看Prometheus格式的指标。

### 主要指标

- `proxy_connections_total` - 总连接数
- `proxy_bytes_transferred_total` - 传输字节数
- `proxy_requests_total` - 请求总数
- `proxy_backend_health` - 后端健康状态
- `proxy_tls_handshakes_total` - TLS握手次数
- `proxy_active_connections` - 当前活跃连接数
- `proxy_errors_total` - 错误总数
- `proxy_request_duration_seconds` - 请求处理时间
- `proxy_backend_response_time_seconds` - 后端响应时间
- `proxy_routing_decisions_total` - 路由决策总数

### 日志

结构化日志输出，支持多种日志级别：

```bash
RUST_LOG=debug cargo run --example enhanced_demo
```

## 🏗️ 架构设计

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Client        │    │  Dynamic Proxy  │    │   Backend       │
│                 │    │                 │    │   Services      │
├─────────────────┤    ├─────────────────┤    ├─────────────────┤
│ TCP/UDP/TLS     │───▶│ TLS Handler     │───▶│ Service A       │
│ Requests        │    │ SNI Detection   │    │ Service B       │
│                 │    │ Multi-Cert      │    │ Service C       │
└─────────────────┘    ├─────────────────┤    └─────────────────┘
                       │ Routing Engine  │              ▲
┌─────────────────┐    │ - Plugin System │              │
│   Redis         │◀───│ - Load Balancer │──────────────┘
│   Backend       │    │ - Health Check  │
│   Storage       │    ├─────────────────┤
├─────────────────┤    │ Monitoring      │    ┌─────────────────┐
│ Backend Config  │    │ - Metrics       │───▶│ Prometheus      │
│ Health Status   │    │ - Logging       │    │ Monitoring      │
│ Routing Rules   │    │ - Tracing       │    └─────────────────┘
└─────────────────┘    └─────────────────┘
```

## 🛡️ 优雅关机

### 支持的信号

- **SIGINT (Ctrl+C)** - 优雅关机
- **SIGTERM** - 优雅关机（适用于系统服务）
- **SIGQUIT** - 强制关机
- **SIGHUP** - 配置重载（当前实现为优雅关机）

### 关机流程

1. **信号接收** → 关机管理器 → 广播关机信号
2. **服务停止** → 停止接受新连接 → 各服务组件停止
3. **连接排空** → 等待活跃连接完成 → 超时处理
4. **资源清理** → 关闭插件 → 清理缓存 → 释放资源

### 测试优雅关机

```bash
# 启动演示程序
cargo run --example graceful_shutdown_demo

# 在另一个终端中测试信号
kill -INT <pid>    # 优雅关机
kill -TERM <pid>   # 优雅关机
kill -QUIT <pid>   # 强制关机
```

## 🔧 开发

### 项目结构

```
dynamic-proxy/
├── src/
│   ├── config/          # 配置管理
│   ├── proxy/           # 代理核心
│   │   ├── tcp_proxy.rs # TCP代理
│   │   ├── udp_proxy.rs # UDP代理
│   │   └── tls_handler.rs # TLS处理
│   ├── routing/         # 路由系统
│   │   ├── plugin.rs    # 插件接口
│   │   ├── rules.rs     # 路由规则
│   │   └── redis_backend.rs # Redis后端
│   ├── monitoring/      # 监控系统
│   └── shutdown.rs      # 优雅关机
├── examples/            # 示例代码
│   ├── basic_usage.rs   # 基础使用示例
│   ├── enhanced_demo.rs # 增强功能演示
│   ├── graceful_shutdown_demo.rs # 优雅关机演示
│   └── test_client.rs   # 测试客户端
├── scripts/             # 脚本工具
│   ├── demo.sh          # 交互式演示菜单
│   ├── demo/            # 演示脚本
│   │   ├── run_basic_demo.sh
│   │   ├── run_enhanced_demo.sh
│   │   └── run_graceful_shutdown_demo.sh
│   ├── test/            # 测试脚本
│   │   ├── run_test_client.sh
│   │   └── test_fixes.sh
│   ├── setup_redis_backends.sh # Redis初始化
│   ├── test_enhanced_features.sh # 功能测试
│   └── test_graceful_shutdown.sh # 关机测试
├── config.toml          # 主配置文件
└── docs/                # 项目文档
    └── FIXES_SUMMARY.md # 修复总结文档
```

### 技术栈

- **语言**: Rust
- **异步运行时**: Tokio
- **TLS**: rustls + tokio-rustls
- **序列化**: serde + serde_json + toml
- **日志**: tracing + tracing-subscriber
- **指标**: prometheus
- **Redis**: redis (aio功能)
- **错误处理**: anyhow
- **异步trait**: async-trait

### 自定义插件

```rust
use async_trait::async_trait;
use dynamic_proxy::routing::*;

struct CustomPlugin;

#[async_trait]
impl RoutingPlugin for CustomPlugin {
    fn name(&self) -> &str {
        "custom_plugin"
    }

    async fn route(&self, context: &RoutingContext) -> Result<Option<RoutingDecision>> {
        // 实现自定义路由逻辑
        Ok(None)
    }
}
```

## 📈 性能

- **高并发**: 基于Tokio异步运行时
- **零拷贝**: 优化的数据传输
- **连接复用**: 智能连接池管理
- **内存效率**: 最小化内存分配

## 🛡️ 安全

- **TLS 1.3支持**: 最新的TLS协议
- **证书验证**: 完整的证书链验证
- **安全配置**: 默认安全的TLS配置
- **访问控制**: 基于规则的访问控制

## 📋 项目状态

### ✅ 已实现功能

- **核心代理** - TCP/UDP流量代理
- **完整TLS支持** - 真实的SNI检测和多证书管理
- **真实Redis集成** - 完整的Redis连接池和后端管理
- **高级路由功能** - 支持通配符、权重、健康检查
- **优雅关机系统** - 完整的信号处理和连接排空
- **生产就绪** - 完整的监控、日志、错误处理

### 🔄 后续改进方向

1. **高级负载均衡** - 实现一致性哈希、最少连接等算法
2. **证书管理** - 自动证书续期和Let's Encrypt集成
3. **性能优化** - 内存池、连接复用、零拷贝等优化
4. **管理界面** - Web管理界面和REST API
5. **分布式部署** - 支持多实例部署和配置同步

## 📝 许可证

MIT License

## 🤝 贡献

欢迎提交Issue和Pull Request！

1. Fork项目
2. 创建功能分支
3. 提交更改
4. 推送到分支
5. 创建Pull Request


