# Dynamic Proxy

TCP/UDP流量转发程序，具备插件化路由系统、动态配置加载、健康检查和监控指标功能。

## 特性

- **TCP/UDP流量转发**: 支持同时处理TCP和UDP流量
- **插件化路由系统**: 支持多种路由算法（轮询、加权、哈希、最少连接、随机）
- **动态配置加载**: 支持运行时配置热重载，无需停机
- **健康检查**: 自动监控后端服务器健康状态
- **监控指标**: 集成Prometheus指标收集
- **优雅关闭**: 支持信号处理和优雅关闭
- **日志记录**: 结构化日志输出，支持多种格式

## 架构设计

### 核心组件

1. **配置管理器 (ConfigManager)**: 负责配置文件的加载、验证和热重载
2. **路由管理器 (RouterManager)**: 管理所有路由器实例和路由规则
3. **代理服务器 (ProxyServer)**: 统一管理TCP和UDP代理服务
4. **健康检查器 (HealthChecker)**: 监控后端服务器健康状态
5. **监控指标收集器 (MetricsCollector)**: 收集和导出Prometheus指标

### 路由器插件

- **轮询路由器 (RoundRobinRouter)**: 按顺序轮流选择后端服务器
- **加权路由器 (WeightedRouter)**: 根据权重选择后端服务器
- **哈希路由器 (HashRouter)**: 基于客户端IP或其他属性进行一致性哈希
- **最少连接路由器 (LeastConnectionsRouter)**: 选择当前连接数最少的后端
- **随机路由器 (RandomRouter)**: 随机选择后端服务器

## 安装和使用

### 编译

```bash
# 克隆项目
git clone <repository-url>
cd dynamic-proxy

# 编译
cargo build --release
```

### 配置

复制并编辑配置文件：

```bash
cp config.toml config.local.toml
# 编辑 config.local.toml
```

### 运行

```bash
# 验证配置
./target/release/dynamic-proxy --validate

# 启动服务
./target/release/dynamic-proxy -c config.local.toml

# 查看帮助
./target/release/dynamic-proxy --help

# 查看版本信息
./target/release/dynamic-proxy --version-info
```

## 配置文件说明

### 服务器配置

```toml
[server]
tcp_bind = "0.0.0.0:8080"      # TCP监听地址
udp_bind = "0.0.0.0:8081"      # UDP监听地址
worker_threads = 4              # 工作线程数
max_connections = 10000         # 最大连接数
connection_timeout = 30         # 连接超时时间（秒）
buffer_size = 8192             # 缓冲区大小
```

### Redis配置

```toml
[redis]
url = "redis://localhost:6379"
password = "your_password"
db = 0
pool_size = 10
connection_timeout = 5
command_timeout = 3
```

### 路由器配置

```toml
[routers.round_robin]
plugin = "round_robin"
config = {}

[routers.weighted]
plugin = "weighted"
config = { smooth_weighted = true }

[routers.hash_ip]
plugin = "hash"
config = { hash_key = "client_ip" }
```

### 路由规则

```toml
[[rules]]
name = "web_traffic"
protocol = "tcp"
enabled = true
router = "round_robin"

[[rules.backends]]
id = "web1"
address = "192.168.1.10:80"
weight = 1
enabled = true

[rules.matcher]
type = "port"
port = 80
```

## 监控和指标

程序集成了Prometheus指标收集，包括：

- 连接指标：活跃连接数、总连接数、连接持续时间
- 请求指标：请求总数、请求持续时间、请求/响应大小
- 后端指标：后端连接数、后端请求数、后端响应时间、后端健康状态
- 路由器指标：路由器选择次数、路由器错误数
- 系统指标：内存使用、CPU使用等

## 健康检查

程序支持自动健康检查功能：

- 定期检查后端服务器可用性
- 支持TCP连接检查
- 可配置检查间隔、超时时间、重试次数
- 自动标记不健康的后端并从负载均衡中移除

## 信号处理

程序支持以下信号：

- `SIGTERM` / `SIGINT`: 优雅关闭
- `SIGHUP`: 重新加载配置（计划中）

## 开发

### 添加新的路由器插件

1. 在 `src/router/plugins/` 目录下创建新的路由器实现
2. 实现 `Router` trait
3. 在 `RouterManager::register_builtin_routers()` 中注册新路由器

### 运行测试

```bash
cargo test
```

### 代码检查

```bash
cargo clippy
cargo fmt
```

## 许可证

[MIT License](LICENSE)

## 贡献

欢迎提交Issue和Pull Request！

## 技术栈

- **Rust**: 主要编程语言
- **Tokio**: 异步运行时
- **Serde**: 序列化/反序列化
- **TOML**: 配置文件格式
- **Prometheus**: 指标收集
- **Redis**: 数据存储（可选）
- **Clap**: 命令行参数解析
- **Tracing**: 日志记录

## 性能特点

- 零拷贝数据转发
- 异步I/O处理
- 内存池管理
- 连接复用
- 高并发支持

## 部署建议

- 使用systemd管理服务
- 配置适当的文件描述符限制
- 监控内存和CPU使用情况
- 定期备份配置文件
- 设置日志轮转