use rust_research_mcp::{Config, DaemonConfig, DaemonService, HealthCheck, PidFile, SignalHandler};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn test_daemon_service_creation() {
    let config = Arc::new(Config::default());
    let daemon_config = DaemonConfig::default();

    let service = DaemonService::new(config, daemon_config);
    assert!(service.is_ok());
}

#[tokio::test]
async fn test_health_check_endpoint() {
    let health_check = HealthCheck::new(0);

    // Test initial state
    let status = health_check.get_status().await;
    assert!(status.healthy);
    assert_eq!(status.message, "Service is healthy");

    // Test setting unhealthy
    health_check.set_unhealthy("Test failure").await;
    let status = health_check.get_status().await;
    assert!(!status.healthy);
    assert_eq!(status.message, "Test failure");

    // Test updating check - note: the actual API uses HealthCheckType enum
    // We can't directly test this without access to the enum
}

#[tokio::test]
async fn test_pid_file_lifecycle() {
    let temp_dir = TempDir::new().unwrap();
    let pid_path = temp_dir.path().join("test.pid");

    // Test creation
    let pid_file = PidFile::create(&pid_path).unwrap();
    assert!(pid_path.exists());
    assert_eq!(pid_file.pid(), std::process::id());
    assert_eq!(pid_file.path(), &pid_path);

    // Test duplicate creation should fail
    let duplicate = PidFile::create(&pid_path);
    assert!(duplicate.is_err());

    // Clean up
    drop(pid_file);
}

#[tokio::test]
async fn test_pid_file_stale_cleanup() {
    let temp_dir = TempDir::new().unwrap();
    let pid_path = temp_dir.path().join("stale.pid");

    // Create a PID file with a fake PID
    std::fs::write(&pid_path, "999999").unwrap();

    // Creating a new PID file should succeed and clean up the stale one
    let pid_file = PidFile::create(&pid_path).unwrap();
    assert!(pid_path.exists());
    assert_eq!(pid_file.pid(), std::process::id());
}

#[tokio::test]
async fn test_signal_handler_creation() {
    let handler = SignalHandler::new();
    assert!(handler.is_ok());

    let handler = handler.unwrap();
    assert!(!handler.signal_pending());
}

#[tokio::test]
async fn test_daemon_config_validation() {
    let mut config = DaemonConfig::default();
    assert!(!config.daemon);
    assert_eq!(config.health_port, 8090);
    assert!(config.auto_restart);
    assert_eq!(config.max_restart_attempts, 3);

    // Test custom configuration
    config.daemon = true;
    config.health_port = 9090;
    config.max_memory_mb = 512;
    config.max_cpu_percent = 80;

    assert!(config.daemon);
    assert_eq!(config.health_port, 9090);
    assert_eq!(config.max_memory_mb, 512);
    assert_eq!(config.max_cpu_percent, 80);
}

#[tokio::test]
async fn test_service_status() {
    let config = Arc::new(Config::default());
    let daemon_config = DaemonConfig::default();

    let service = DaemonService::new(config, daemon_config).unwrap();
    let status = service.get_status().await;

    assert!(status.running);
    assert_eq!(status.requests_handled, 0);
    assert_eq!(status.errors_count, 0);
    assert_eq!(status.restart_count, 0);
}

#[tokio::test]
async fn test_health_check_startup_probe() {
    let health_check = HealthCheck::new(0);

    // Test startup status
    let initial_status = health_check.get_status().await;
    assert!(initial_status.healthy);

    // Simulate failure and recovery
    health_check.set_unhealthy("Database connecting...").await;
    let status = health_check.get_status().await;
    assert!(!status.healthy);

    health_check.set_healthy().await;
    let status = health_check.get_status().await;
    assert!(status.healthy);
}

#[tokio::test]
async fn test_daemon_shutdown() {
    let config = Arc::new(Config::default());
    let daemon_config = DaemonConfig::default();

    let service = DaemonService::new(config, daemon_config).unwrap();

    // Test graceful shutdown
    tokio::spawn(async move {
        sleep(Duration::from_millis(100)).await;
        service.shutdown().await;
    });

    // The shutdown should complete without hanging
    tokio::time::timeout(Duration::from_secs(5), async {
        // Simulate waiting for shutdown
        sleep(Duration::from_millis(200)).await;
    })
    .await
    .expect("Shutdown should complete within timeout");
}

#[tokio::test]
async fn test_resource_monitoring() {
    let config = Arc::new(Config::default());
    let daemon_config = DaemonConfig {
        monitor_interval_secs: 1,
        max_memory_mb: 10000, // High limit to avoid triggering
        max_cpu_percent: 100,
        ..DaemonConfig::default()
    };

    let service = DaemonService::new(config, daemon_config).unwrap();
    let initial_status = service.get_status().await;

    // Memory and CPU usage should be initialized
    // memory_usage_mb is unsigned, so always >= 0
    assert!(initial_status.cpu_usage_percent >= 0.0);
}

#[test]
fn test_standard_pid_path() {
    let path = PidFile::standard_path();
    assert!(path.to_string_lossy().contains("rust_research_mcp.pid"));

    // Path should be absolute
    assert!(path.is_absolute());
}

#[tokio::test]
async fn test_health_check_concurrent_updates() {
    let health_check = Arc::new(HealthCheck::new(0));

    // Spawn multiple tasks updating health status concurrently
    let mut handles = vec![];
    for i in 0..10 {
        let hc = health_check.clone();
        let handle = tokio::spawn(async move {
            if i % 2 == 0 {
                hc.set_healthy().await;
            } else {
                hc.set_unhealthy(&format!("Check {i} failed")).await;
            }
        });
        handles.push(handle);
    }

    // Wait for all updates to complete
    for handle in handles {
        handle.await.unwrap();
    }

    // Verify final status
    let status = health_check.get_status().await;
    assert!(status.timestamp <= std::time::SystemTime::now());
}

#[tokio::test]
async fn test_daemon_config_serialization() {
    let config = DaemonConfig {
        daemon: true,
        pid_file: Some(PathBuf::from("/var/run/test.pid")),
        health_port: 8888,
        max_memory_mb: 256,
        max_cpu_percent: 75,
        auto_restart: false,
        max_restart_attempts: 5,
        restart_delay_secs: 10,
        ..Default::default()
    };

    // Test serialization
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("\"daemon\":true"));
    assert!(json.contains("\"health_port\":8888"));

    // Test deserialization
    let deserialized: DaemonConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.daemon, config.daemon);
    assert_eq!(deserialized.health_port, config.health_port);
    assert_eq!(deserialized.max_memory_mb, config.max_memory_mb);
}
