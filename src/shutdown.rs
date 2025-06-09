use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// 关机信号类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShutdownSignal {
    /// SIGINT (Ctrl+C)
    Interrupt,
    /// SIGTERM (优雅关闭)
    Terminate,
    /// SIGQUIT (强制退出)
    Quit,
    /// SIGHUP (重新加载配置)
    Hangup,
    /// 程序内部触发的关机
    Internal,
}

impl std::fmt::Display for ShutdownSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShutdownSignal::Interrupt => write!(f, "SIGINT"),
            ShutdownSignal::Terminate => write!(f, "SIGTERM"),
            ShutdownSignal::Quit => write!(f, "SIGQUIT"),
            ShutdownSignal::Hangup => write!(f, "SIGHUP"),
            ShutdownSignal::Internal => write!(f, "INTERNAL"),
        }
    }
}

/// 关机状态
#[derive(Debug, Clone, PartialEq)]
pub enum ShutdownState {
    /// 正常运行
    Running,
    /// 开始关机流程
    ShuttingDown,
    /// 等待连接排空
    Draining,
    /// 强制关闭
    ForceShutdown,
    /// 已关闭
    Shutdown,
}

/// 优雅关机管理器
pub struct GracefulShutdown {
    /// 关机信号广播器
    shutdown_tx: broadcast::Sender<ShutdownSignal>,
    /// 当前状态
    state: Arc<RwLock<ShutdownState>>,
    /// 活跃连接计数
    active_connections: Arc<Mutex<u64>>,
    /// 关机超时时间
    shutdown_timeout: Duration,
    /// 连接排空超时时间
    drain_timeout: Duration,
    /// 是否启用强制关闭
    force_shutdown_enabled: bool,
}

impl GracefulShutdown {
    /// 创建新的优雅关机管理器
    pub fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);
        
        Self {
            shutdown_tx,
            state: Arc::new(RwLock::new(ShutdownState::Running)),
            active_connections: Arc::new(Mutex::new(0)),
            shutdown_timeout: Duration::from_secs(30),
            drain_timeout: Duration::from_secs(10),
            force_shutdown_enabled: true,
        }
    }

    /// 配置关机超时时间
    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    /// 配置连接排空超时时间
    pub fn with_drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_timeout = timeout;
        self
    }

    /// 配置是否启用强制关闭
    pub fn with_force_shutdown(mut self, enabled: bool) -> Self {
        self.force_shutdown_enabled = enabled;
        self
    }

    /// 获取关机信号接收器
    pub fn subscribe(&self) -> broadcast::Receiver<ShutdownSignal> {
        self.shutdown_tx.subscribe()
    }

    /// 获取当前状态
    pub async fn get_state(&self) -> ShutdownState {
        self.state.read().await.clone()
    }

    /// 检查是否正在关机
    pub async fn is_shutting_down(&self) -> bool {
        let state = self.state.read().await;
        !matches!(*state, ShutdownState::Running)
    }

    /// 增加活跃连接计数
    pub async fn add_connection(&self) {
        let mut count = self.active_connections.lock().await;
        *count += 1;
        debug!("Active connections: {}", *count);
    }

    /// 减少活跃连接计数
    pub async fn remove_connection(&self) {
        let mut count = self.active_connections.lock().await;
        if *count > 0 {
            *count -= 1;
        }
        debug!("Active connections: {}", *count);
    }

    /// 获取活跃连接数
    pub async fn get_active_connections(&self) -> u64 {
        *self.active_connections.lock().await
    }

    /// 启动信号监听
    pub async fn start_signal_handling(&self) -> Result<()> {
        let shutdown_tx = self.shutdown_tx.clone();
        
        // 监听 SIGINT (Ctrl+C)
        let shutdown_tx_int = shutdown_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::signal::ctrl_c().await {
                error!("Failed to listen for SIGINT: {}", e);
                return;
            }
            info!("Received SIGINT signal");
            let _ = shutdown_tx_int.send(ShutdownSignal::Interrupt);
        });

        // Unix 信号处理
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};

            // 监听 SIGTERM
            let shutdown_tx_term = shutdown_tx.clone();
            tokio::spawn(async move {
                let mut sigterm = match signal(SignalKind::terminate()) {
                    Ok(signal) => signal,
                    Err(e) => {
                        error!("Failed to register SIGTERM handler: {}", e);
                        return;
                    }
                };

                if sigterm.recv().await.is_some() {
                    info!("Received SIGTERM signal");
                    let _ = shutdown_tx_term.send(ShutdownSignal::Terminate);
                }
            });

            // 监听 SIGQUIT
            let shutdown_tx_quit = shutdown_tx.clone();
            tokio::spawn(async move {
                let mut sigquit = match signal(SignalKind::quit()) {
                    Ok(signal) => signal,
                    Err(e) => {
                        error!("Failed to register SIGQUIT handler: {}", e);
                        return;
                    }
                };

                if sigquit.recv().await.is_some() {
                    info!("Received SIGQUIT signal");
                    let _ = shutdown_tx_quit.send(ShutdownSignal::Quit);
                }
            });

            // 监听 SIGHUP
            let shutdown_tx_hup = shutdown_tx.clone();
            tokio::spawn(async move {
                let mut sighup = match signal(SignalKind::hangup()) {
                    Ok(signal) => signal,
                    Err(e) => {
                        error!("Failed to register SIGHUP handler: {}", e);
                        return;
                    }
                };

                if sighup.recv().await.is_some() {
                    info!("Received SIGHUP signal");
                    let _ = shutdown_tx_hup.send(ShutdownSignal::Hangup);
                }
            });
        }

        info!("Signal handlers registered successfully");
        Ok(())
    }

    /// 触发关机
    pub async fn trigger_shutdown(&self, signal: ShutdownSignal) -> Result<()> {
        let mut state = self.state.write().await;
        
        match *state {
            ShutdownState::Running => {
                info!("Initiating graceful shutdown due to signal: {}", signal);
                *state = ShutdownState::ShuttingDown;
                
                // 发送关机信号
                if let Err(e) = self.shutdown_tx.send(signal) {
                    warn!("Failed to broadcast shutdown signal: {}", e);
                }
                
                Ok(())
            }
            _ => {
                warn!("Shutdown already in progress, ignoring signal: {}", signal);
                Ok(())
            }
        }
    }

    /// 等待关机信号
    pub async fn wait_for_shutdown(&self) -> ShutdownSignal {
        let mut receiver = self.subscribe();
        
        match receiver.recv().await {
            Ok(signal) => {
                info!("Received shutdown signal: {}", signal);
                signal
            }
            Err(e) => {
                error!("Error receiving shutdown signal: {}", e);
                ShutdownSignal::Internal
            }
        }
    }

    /// 执行优雅关机流程
    pub async fn execute_graceful_shutdown(&self) -> Result<()> {
        let signal = self.wait_for_shutdown().await;
        
        // 根据信号类型决定关机策略
        match signal {
            ShutdownSignal::Quit => {
                // SIGQUIT 立即强制关闭
                info!("Force shutdown requested");
                self.force_shutdown().await
            }
            ShutdownSignal::Hangup => {
                // SIGHUP 通常用于重新加载配置，这里暂时当作普通关机处理
                info!("Configuration reload requested, performing graceful shutdown");
                self.graceful_shutdown().await
            }
            _ => {
                // 其他信号执行优雅关机
                self.graceful_shutdown().await
            }
        }
    }

    /// 优雅关机流程
    async fn graceful_shutdown(&self) -> Result<()> {
        info!("Starting graceful shutdown process");
        
        // 设置状态为排空连接
        {
            let mut state = self.state.write().await;
            *state = ShutdownState::Draining;
        }
        
        // 等待连接排空或超时
        let drain_result = timeout(self.drain_timeout, self.wait_for_connections_drain()).await;
        
        match drain_result {
            Ok(_) => {
                info!("All connections drained successfully");
            }
            Err(_) => {
                let active = self.get_active_connections().await;
                warn!("Drain timeout reached with {} active connections", active);
                
                if self.force_shutdown_enabled {
                    warn!("Proceeding with force shutdown");
                    return self.force_shutdown().await;
                }
            }
        }
        
        // 设置最终状态
        {
            let mut state = self.state.write().await;
            *state = ShutdownState::Shutdown;
        }
        
        info!("Graceful shutdown completed");
        Ok(())
    }

    /// 强制关机
    async fn force_shutdown(&self) -> Result<()> {
        info!("Executing force shutdown");
        
        {
            let mut state = self.state.write().await;
            *state = ShutdownState::ForceShutdown;
        }
        
        // 等待短暂时间让正在进行的操作完成
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        {
            let mut state = self.state.write().await;
            *state = ShutdownState::Shutdown;
        }
        
        info!("Force shutdown completed");
        Ok(())
    }

    /// 等待所有连接排空
    async fn wait_for_connections_drain(&self) {
        loop {
            let active = self.get_active_connections().await;
            if active == 0 {
                break;
            }
            
            debug!("Waiting for {} connections to drain", active);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

impl Default for GracefulShutdown {
    fn default() -> Self {
        Self::new()
    }
}
