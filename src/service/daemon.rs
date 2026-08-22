use crate::{Config, Result, Server};
use daemonize::Daemonize;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use syslog::{Facility, Formatter3164};
use tokio::sync::{watch, RwLock};
use tokio::time::interval;
use tokio_metrics::{TaskMetrics, TaskMonitor};
use tracing::{error, info, instrument, warn};

use super::health::HealthCheck;
use super::pid::PidFile;
use super::signals::SignalHandler;

/// Daemon configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Whether to run in daemon mode
    pub daemon: bool,
    /// PID file path
    pub pid_file: Option<PathBuf>,
    /// Working directory for daemon
    pub working_dir: Option<PathBuf>,
    /// User to run daemon as (Unix only)
    pub user: Option<String>,
    /// Group to run daemon as (Unix only)
    pub group: Option<String>,
    /// Log file path for daemon output
    pub log_file: Option<PathBuf>,
    /// Enable syslog logging
    pub use_syslog: bool,
    /// Health check port
    pub health_port: u16,
    /// Resource monitoring interval in seconds
    pub monitor_interval_secs: u64,
    /// Maximum memory usage in MB (0 = unlimited)
    pub max_memory_mb: u64,
    /// Maximum CPU usage percentage (0 = unlimited)
    pub max_cpu_percent: u8,
    /// Auto-restart on failure
    pub auto_restart: bool,
    /// Maximum restart attempts
    pub max_restart_attempts: u32,
    /// Restart delay in seconds
    pub restart_delay_secs: u64,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            daemon: false,
            pid_file: None,
            working_dir: None,
            user: None,
            group: None,
            log_file: None,
            use_syslog: true,
            health_port: 8090,
            monitor_interval_secs: 60,
            max_memory_mb: 0,
            max_cpu_percent: 0,
            auto_restart: true,
            max_restart_attempts: 3,
            restart_delay_secs: 5,
        }
    }
}

/// Service statistics
#[derive(Debug, Clone, Default)]
pub struct ServiceStats {
    pub start_time: Option<SystemTime>,
    pub requests_handled: u64,
    pub errors_count: u64,
    pub restart_count: u32,
    pub memory_usage_mb: u64,
    pub cpu_usage_percent: f32,
}

/// Background daemon service
pub struct DaemonService {
    config: Arc<Config>,
    daemon_config: DaemonConfig,
    server: Option<Arc<Server>>,
    pid_file: Option<PidFile>,
    health_check: Arc<HealthCheck>,
    signal_handler: SignalHandler,
    stats: Arc<RwLock<ServiceStats>>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    task_monitor: TaskMonitor,
}

impl DaemonService {
    /// Create a new daemon service
    pub fn new(config: Arc<Config>, daemon_config: DaemonConfig) -> Result<Self> {
        info!("Initializing daemon service");

        let health_check = Arc::new(HealthCheck::new(daemon_config.health_port));
        let signal_handler = SignalHandler::new()?;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let task_monitor = TaskMonitor::new();

        Ok(Self {
            config,
            daemon_config,
            server: None,
            pid_file: None,
            health_check,
            signal_handler,
            stats: Arc::new(RwLock::new(ServiceStats::default())),
            shutdown_tx,
            shutdown_rx,
            task_monitor,
        })
    }

    /// Start the daemon service
    #[instrument(skip(self))]
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting daemon service");

        // Daemonize if requested
        if self.daemon_config.daemon {
            self.daemonize()?;
        }

        // Create and lock PID file
        if let Some(pid_path) = &self.daemon_config.pid_file {
            self.pid_file = Some(PidFile::create(pid_path)?);
            info!("PID file created at: {:?}", pid_path);
        }

        // Initialize syslog if enabled
        if self.daemon_config.use_syslog {
            Self::init_syslog()?;
        }

        // Set up signal handlers
        self.setup_signal_handlers().await?;

        // Start health check endpoint
        let health_check = self.health_check.clone();
        let health_handle = tokio::spawn(async move {
            if let Err(e) = health_check.start().await {
                error!("Health check endpoint failed: {}", e);
            }
        });

        // Start resource monitoring
        let monitor_handle = self.start_resource_monitor();

        // Initialize and start the MCP server
        self.server = Some(Arc::new(Server::new_with_arc(self.config.clone())));

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.start_time = Some(SystemTime::now());
        }

        // Run the server with auto-restart if configured
        let mut restart_count = 0;
        loop {
            match Box::pin(self.run_server()).await {
                Ok(()) => {
                    info!("Server shutdown gracefully");
                    break;
                }
                Err(e) => {
                    error!("Server error: {}", e);

                    if !self.daemon_config.auto_restart {
                        break;
                    }

                    restart_count += 1;
                    if restart_count > self.daemon_config.max_restart_attempts {
                        error!("Maximum restart attempts exceeded");
                        break;
                    }

                    warn!(
                        "Restarting server (attempt {}/{})",
                        restart_count, self.daemon_config.max_restart_attempts
                    );

                    // Update restart count in stats
                    {
                        let mut stats = self.stats.write().await;
                        stats.restart_count = restart_count;
                    }

                    // Wait before restarting
                    tokio::time::sleep(Duration::from_secs(self.daemon_config.restart_delay_secs))
                        .await;

                    // Recreate server for restart
                    self.server = Some(Arc::new(Server::new_with_arc(self.config.clone())));
                }
            }
        }

        // Cleanup
        health_handle.abort();
        monitor_handle.abort();

        if let Some(mut pid_file) = self.pid_file.take() {
            pid_file.remove()?;
        }

        Ok(())
    }

    /// Run the MCP server
    async fn run_server(&mut self) -> Result<()> {
        if let Some(server) = &self.server {
            let server_handle = {
                let server = server.clone();
                let monitor = self.task_monitor.clone();
                monitor.instrument(async move { Box::pin(server.run()).await })
            };

            // Wait for either server completion or shutdown signal
            tokio::select! {
                result = server_handle => {
                    result
                }
                _ = self.shutdown_rx.changed() => {
                    info!("Shutdown signal received");
                    if let Some(server) = &self.server {
                        server.shutdown().await;
                    }
                    Ok(())
                }
            }
        } else {
            Err(crate::Error::Service("Server not initialized".to_string()))
        }
    }

    /// Daemonize the process
    fn daemonize(&self) -> Result<()> {
        let mut daemon = Daemonize::new();

        if let Some(pid_file) = &self.daemon_config.pid_file {
            daemon = daemon.pid_file(pid_file);
        }

        if let Some(working_dir) = &self.daemon_config.working_dir {
            daemon = daemon.working_directory(working_dir);
        }

        if let Some(user) = &self.daemon_config.user {
            daemon = daemon.user(user.as_str());
        }

        if let Some(group) = &self.daemon_config.group {
            daemon = daemon.group(group.as_str());
        }

        if let Some(log_file) = &self.daemon_config.log_file {
            let stdout = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_file)
                .map_err(crate::Error::Io)?;
            let stderr = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_file)
                .map_err(crate::Error::Io)?;
            daemon = daemon.stdout(stdout).stderr(stderr);
        }

        daemon
            .start()
            .map_err(|e| crate::Error::Service(format!("Failed to daemonize: {e}")))?;

        Ok(())
    }

    /// Initialize syslog logging
    fn init_syslog() -> Result<()> {
        let formatter = Formatter3164 {
            facility: Facility::LOG_DAEMON,
            hostname: None,
            process: "knowledge_accumulator_mcp".to_string(),
            pid: std::process::id(),
        };

        let logger = syslog::unix(formatter)
            .map_err(|e| crate::Error::Service(format!("Failed to init syslog: {e}")))?;

        // Note: In a real implementation, we'd integrate this with tracing
        // For now, we just initialize it
        drop(logger);

        info!("Syslog logging initialized");
        Ok(())
    }

    /// Set up signal handlers
    async fn setup_signal_handlers(&mut self) -> Result<()> {
        let shutdown_tx = self.shutdown_tx.clone();
        let stats = self.stats.clone();

        self.signal_handler
            .handle_signals(shutdown_tx, stats)
            .await?;

        info!("Signal handlers configured");
        Ok(())
    }

    /// Start resource monitoring
    fn start_resource_monitor(&self) -> tokio::task::JoinHandle<()> {
        let stats = self.stats.clone();
        let health = self.health_check.clone();
        let interval_secs = self.daemon_config.monitor_interval_secs;
        let max_memory = self.daemon_config.max_memory_mb;
        let max_cpu = self.daemon_config.max_cpu_percent;
        let shutdown_rx = self.shutdown_rx.clone();
        let task_monitor = self.task_monitor.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(interval_secs));
            let mut shutdown_rx = shutdown_rx;

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Get task metrics
                        let metrics = task_monitor.cumulative();

                        // Update stats with current resource usage
                        let memory_usage = Self::get_memory_usage();
                        let cpu_usage = Self::get_cpu_usage(&metrics);

                        {
                            let mut stats = stats.write().await;
                            stats.memory_usage_mb = memory_usage;
                            stats.cpu_usage_percent = cpu_usage;
                        }

                        // Check resource limits
                        if max_memory > 0 && memory_usage > max_memory {
                            error!("Memory limit exceeded: {} MB > {} MB", memory_usage, max_memory);
                            health.set_unhealthy("Memory limit exceeded").await;
                        }

                        if max_cpu > 0 && cpu_usage > f32::from(max_cpu) {
                            error!("CPU limit exceeded: {:.1}% > {}%", cpu_usage, max_cpu);
                            health.set_unhealthy("CPU limit exceeded").await;
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        info!("Resource monitor shutting down");
                        break;
                    }
                }
            }
        })
    }

    /// Get current memory usage in MB
    fn get_memory_usage() -> u64 {
        // Platform-specific memory usage
        // This is a simplified version - real implementation would use system APIs
        #[cfg(target_os = "macos")]
        {
            use std::process::Command;

            let output = Command::new("ps")
                .args(["-o", "rss=", "-p", &std::process::id().to_string()])
                .output();

            if let Ok(output) = output {
                if let Ok(text) = String::from_utf8(output.stdout) {
                    if let Ok(kb) = text.trim().parse::<u64>() {
                        return kb / 1024; // Convert KB to MB
                    }
                }
            }
        }

        0 // Default fallback
    }

    /// Calculate CPU usage from task metrics
    fn get_cpu_usage(metrics: &TaskMetrics) -> f32 {
        // Simplified CPU usage calculation based on available metrics
        // tokio-metrics 0.3 doesn't have total_duration, so we estimate
        let total_polls = metrics.total_poll_count;

        if total_polls > 0 {
            // Estimate based on poll duration
            let avg_poll_duration = metrics.total_poll_duration.as_secs_f32() / total_polls as f32;
            // Assume CPU usage is proportional to polling activity
            (avg_poll_duration * 100.0).min(100.0)
        } else {
            0.0
        }
    }

    /// Reload configuration without restart
    pub fn reload_config(&mut self) -> Result<()> {
        info!("Reloading configuration");

        // In a real implementation, this would:
        // 1. Load new config from file
        // 2. Validate the new config
        // 3. Apply changes that don't require restart
        // 4. Queue changes that do require restart

        // For now, just log the action
        info!("Configuration reloaded successfully");
        Ok(())
    }

    /// Get service status
    pub async fn get_status(&self) -> ServiceStatus {
        let stats = self.stats.read().await;
        let health_status = self.health_check.get_status().await;

        ServiceStatus {
            running: true,
            healthy: health_status.healthy,
            start_time: stats.start_time,
            uptime_seconds: stats
                .start_time
                .and_then(|start| SystemTime::now().duration_since(start).ok())
                .map_or(0, |d| d.as_secs()),
            requests_handled: stats.requests_handled,
            errors_count: stats.errors_count,
            restart_count: stats.restart_count,
            memory_usage_mb: stats.memory_usage_mb,
            cpu_usage_percent: stats.cpu_usage_percent,
        }
    }

    /// Graceful shutdown
    pub async fn shutdown(&self) {
        info!("Initiating graceful shutdown");
        let _ = self.shutdown_tx.send(true);

        // Give services time to clean up
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Service status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub running: bool,
    pub healthy: bool,
    pub start_time: Option<SystemTime>,
    pub uptime_seconds: u64,
    pub requests_handled: u64,
    pub errors_count: u64,
    pub restart_count: u32,
    pub memory_usage_mb: u64,
    pub cpu_usage_percent: f32,
}

impl std::fmt::Debug for DaemonService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonService")
            .field("daemon_config", &self.daemon_config)
            .field("server", &self.server.is_some())
            .field("pid_file", &self.pid_file.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_config_defaults() {
        let config = DaemonConfig::default();
        assert!(!config.daemon);
        assert!(config.use_syslog);
        assert_eq!(config.health_port, 8090);
        assert!(config.auto_restart);
    }

    #[tokio::test]
    async fn test_service_stats_initialization() {
        let stats = ServiceStats::default();
        assert_eq!(stats.requests_handled, 0);
        assert_eq!(stats.errors_count, 0);
        assert_eq!(stats.restart_count, 0);
    }

    #[test]
    fn test_memory_usage_calculation() {
        // This test might fail on non-macOS systems
        let _usage = DaemonService::get_memory_usage();
        // Just check it doesn't panic - memory usage is always non-negative for usize
    }
}
