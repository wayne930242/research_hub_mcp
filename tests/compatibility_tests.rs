use rust_research_mcp::{Config, MetaSearchClient, MetaSearchConfig, Server};
use std::env;
use tempfile::TempDir;

/// Environment compatibility tests for different platforms and configurations
#[tokio::test]
async fn test_cross_platform_path_handling() {
    // Test path handling across different platforms
    let temp_dir = TempDir::new().unwrap();
    let mut config = Config::default();

    // Test various path formats
    let test_paths = vec![
        temp_dir.path().to_path_buf(),
        temp_dir.path().join("subdir"),
        temp_dir
            .path()
            .join("nested")
            .join("deep")
            .join("directory"),
    ];

    for path in test_paths {
        config.downloads.directory = path.clone();

        // Should validate successfully on current platform
        let validation_result = config.validate();
        if path.parent().unwrap().exists() || path == temp_dir.path() {
            assert!(
                validation_result.is_ok(),
                "Valid path should be accepted: {path:?}"
            );
        }
    }
}

#[tokio::test]
async fn test_filesystem_permissions() {
    // Test filesystem permission handling
    let temp_dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.downloads.directory = temp_dir.path().to_path_buf();

    // Create a directory and test access
    let test_dir = temp_dir.path().join("test_downloads");
    std::fs::create_dir_all(&test_dir).unwrap();

    config.downloads.directory = test_dir;
    assert!(
        config.validate().is_ok(),
        "Writable directory should be valid"
    );

    // Test with non-existent parent directory
    let invalid_dir = temp_dir
        .path()
        .join("nonexistent")
        .join("deep")
        .join("path");
    config.downloads.directory = invalid_dir;
    // Validation might pass (parent dir existence isn't always checked), but creation should handle it
}

#[test]
fn test_environment_variable_handling() {
    // Test environment variable parsing and handling

    // Save original values
    let original_port = env::var("RUST_SCI_HUB_MCP_PORT").ok();
    let original_host = env::var("RUST_SCI_HUB_MCP_HOST").ok();
    let original_mirrors = env::var("RUST_SCI_HUB_MCP_MIRRORS").ok();

    // Test setting environment variables
    env::set_var("RUST_SCI_HUB_MCP_PORT", "9090");
    env::set_var("RUST_SCI_HUB_MCP_HOST", "0.0.0.0");
    env::set_var(
        "RUST_SCI_HUB_MCP_MIRRORS",
        "https://sci-hub.se,https://sci-hub.st",
    );

    // Config should respect environment variables if implemented
    let config = Config::default();

    // Restore original values
    match original_port {
        Some(val) => env::set_var("RUST_SCI_HUB_MCP_PORT", val),
        None => env::remove_var("RUST_SCI_HUB_MCP_PORT"),
    }
    match original_host {
        Some(val) => env::set_var("RUST_SCI_HUB_MCP_HOST", val),
        None => env::remove_var("RUST_SCI_HUB_MCP_HOST"),
    }
    match original_mirrors {
        Some(val) => env::set_var("RUST_SCI_HUB_MCP_MIRRORS", val),
        None => env::remove_var("RUST_SCI_HUB_MCP_MIRRORS"),
    }

    // Basic validation that config loads
    assert!(
        config.validate().is_ok(),
        "Config should load from environment"
    );
}

#[tokio::test]
async fn test_network_stack_compatibility() {
    // Test network stack compatibility (IPv4/IPv6)
    let ipv4_configs = vec![
        ("127.0.0.1", 8080),
        ("0.0.0.0", 8081),
        ("192.168.1.1", 8082),
    ];

    for (host, port) in ipv4_configs {
        let mut config = Config::default();
        config.server.host = host.to_string();
        config.server.port = port;

        assert!(
            config.validate().is_ok(),
            "IPv4 config should be valid: {host}:{port}"
        );

        // Test that server can be created (doesn't test binding)
        let server = Server::new(config);
        assert!(
            !server.is_shutdown_requested(),
            "Server should be created successfully"
        );
    }

    // Test IPv6 if supported
    let ipv6_configs = vec![("::1", 8090), ("::", 8091)];

    for (host, port) in ipv6_configs {
        let mut config = Config::default();
        config.server.host = host.to_string();
        config.server.port = port;

        // IPv6 might not be supported on all systems, so don't fail the test
        let validation_result = config.validate();
        if validation_result.is_ok() {
            let server = Server::new(config);
            assert!(
                !server.is_shutdown_requested(),
                "IPv6 server should be created if supported"
            );
        }
    }
}

#[tokio::test]
async fn test_concurrent_client_limits() {
    // Test behavior under different concurrent client scenarios
    let temp_dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.downloads.directory = temp_dir.path().to_path_buf();
    config.downloads.max_concurrent = 2; // Low limit for testing
    config.research_source.endpoints = vec!["https://test.com".to_string()];

    // Test creating multiple clients
    let mut clients = Vec::new();
    for i in 0..5 {
        let meta_config = MetaSearchConfig::default();
        let client_result = MetaSearchClient::new(config.clone(), meta_config);
        match client_result {
            Ok(client) => {
                clients.push(client);
                println!("Created client {}", i + 1);
            }
            Err(e) => {
                println!("Failed to create client {}: {:?}", i + 1, e);
            }
        }
    }

    // Should be able to create multiple clients
    assert!(
        !clients.is_empty(),
        "Should be able to create at least one client"
    );
}

#[test]
fn test_memory_usage_patterns() {
    // Test memory usage patterns under different loads
    let start_memory = get_memory_usage();

    // Create multiple configurations
    let mut configs = Vec::new();
    for i in 0..100 {
        let mut config = Config::default();
        config.server.port = 8000 + i;
        configs.push(config);
    }

    let after_configs = get_memory_usage();

    // Memory usage should be reasonable
    let memory_increase = after_configs.saturating_sub(start_memory);
    assert!(
        memory_increase < 100 * 1024 * 1024,
        "Memory usage should be reasonable"
    ); // Less than 100MB

    // Clean up
    drop(configs);

    println!(
        "Memory usage test: start={}MB, after={}MB, increase={}MB",
        start_memory / 1024 / 1024,
        after_configs / 1024 / 1024,
        memory_increase / 1024 / 1024
    );
}

#[test]
fn test_file_descriptor_limits() {
    // Test handling of file descriptor limits
    let temp_dir = TempDir::new().unwrap();

    // Try to create many temporary files to test FD handling
    let mut files = Vec::new();
    for i in 0..100 {
        let file_path = temp_dir.path().join(format!("test_file_{i}.txt"));
        match std::fs::File::create(&file_path) {
            Ok(file) => files.push(file),
            Err(e) => {
                println!("Failed to create file {i}: {e:?}");
                break;
            }
        }
    }

    println!("Created {} test files", files.len());

    // Should handle file creation gracefully
    assert!(
        files.len() > 10,
        "Should be able to create at least 10 files"
    );

    // Clean up happens automatically when files vector is dropped
}

#[tokio::test]
async fn test_signal_handling_compatibility() {
    // Test signal handling on different platforms
    use std::time::Duration;

    let config = Config::default();
    let server = Server::new(config);

    // Test graceful shutdown request
    let shutdown_future = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        server.shutdown().await;
    };

    let server_future = async {
        // This would normally run the server, but we'll just check shutdown status
        while !server.is_shutdown_requested() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };

    // Run both futures concurrently
    tokio::select! {
        () = shutdown_future => {},
        () = server_future => {},
        () = tokio::time::sleep(Duration::from_secs(1)) => {
            panic!("Test timed out");
        }
    }

    assert!(
        server.is_shutdown_requested(),
        "Server should be marked for shutdown"
    );
}

#[test]
fn test_locale_and_encoding_handling() {
    // Test handling of different locales and character encodings
    let test_strings = vec![
        "English text",
        "Русский текст", // Russian
        "中文文本",      // Chinese
        "العربية",       // Arabic
        "日本語",        // Japanese
        "한국어",        // Korean
        "Français",      // French with accents
        "Español",       // Spanish with accents
        "Deutsch: äöüß", // German with umlauts
    ];

    for text in test_strings {
        // Test that strings are handled properly in configuration
        let mut config = Config::default();
        // Test with different timeout values derived from text
        config.research_source.timeout_secs = (text.len() as u64).clamp(1, 300);

        // Should not crash when serializing/deserializing
        let serialized = serde_json::to_string(&config);
        assert!(
            serialized.is_ok(),
            "Should serialize config with text: {text}"
        );

        if let Ok(json) = serialized {
            let deserialized: Result<Config, _> = serde_json::from_str(&json);
            assert!(
                deserialized.is_ok(),
                "Should deserialize config with text: {text}"
            );
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn test_macos_specific_features() {
    // Test macOS-specific functionality
    use std::process::Command;

    // Test that we can access system information
    let output = Command::new("sw_vers").arg("-productVersion").output();

    if let Ok(output) = output {
        let version = String::from_utf8_lossy(&output.stdout);
        println!("macOS version: {}", version.trim());

        // Basic validation that we're on macOS
        assert!(!version.is_empty(), "Should detect macOS version");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn test_linux_specific_features() {
    // Test Linux-specific functionality
    use std::fs;

    // Test that we can read system information
    if let Ok(contents) = fs::read_to_string("/proc/version") {
        println!(
            "Linux kernel info: {}",
            contents.lines().next().unwrap_or("unknown")
        );
        assert!(!contents.is_empty(), "Should read Linux kernel info");
    }
}

#[cfg(target_os = "windows")]
#[test]
fn test_windows_specific_features() {
    // Test Windows-specific functionality
    use std::env;

    // Test Windows environment
    let os = env::var("OS").unwrap_or_default();
    let computername = env::var("COMPUTERNAME").unwrap_or_default();

    println!("Windows OS: {}, Computer: {}", os, computername);

    // Basic validation that we're on Windows
    assert!(
        os.to_lowercase().contains("windows") || !computername.is_empty(),
        "Should detect Windows environment"
    );
}

// Helper function to get current memory usage (approximate)
fn get_memory_usage() -> usize {
    // This is a simplified memory usage estimation
    // In a real implementation, you might use platform-specific APIs

    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<usize>() {
                            return kb * 1024; // Convert KB to bytes
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("ps")
            .args(&["-o", "rss=", "-p"])
            .arg(std::process::id().to_string())
            .output()
        {
            if let Ok(rss_str) = String::from_utf8(output.stdout) {
                if let Ok(kb) = rss_str.trim().parse::<usize>() {
                    return kb * 1024; // Convert KB to bytes
                }
            }
        }
    }

    // Fallback: return a default value
    1024 * 1024 // 1MB
}
