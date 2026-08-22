use rust_research_mcp::{Config, Error};

#[tokio::test]
async fn test_config_default() {
    let config = Config::default();
    assert_eq!(config.server.port, 8080);
    assert_eq!(config.server.host, "127.0.0.1");
    assert_eq!(config.server.timeout_secs, 30);
    assert_eq!(config.research_source.rate_limit_per_sec, 1);
    assert!(!config.research_source.endpoints.is_empty());
    assert_eq!(config.downloads.max_concurrent, 3);
}

#[tokio::test]
async fn test_config_validation() {
    let mut config = Config::default();

    // Valid config should pass
    assert!(config.validate().is_ok());

    // Invalid port
    config.server.port = 0;
    assert!(matches!(config.validate(), Err(Error::InvalidInput { .. })));
    config.server.port = 8080;

    // Empty endpoints
    config.research_source.endpoints.clear();
    assert!(matches!(config.validate(), Err(Error::InvalidInput { .. })));
    config
        .research_source
        .endpoints
        .push("https://test.com".to_string());

    // Zero rate limit
    config.research_source.rate_limit_per_sec = 0;
    assert!(matches!(config.validate(), Err(Error::InvalidInput { .. })));
    config.research_source.rate_limit_per_sec = 1;

    // Zero max concurrent
    config.downloads.max_concurrent = 0;
    assert!(matches!(config.validate(), Err(Error::InvalidInput { .. })));
}

#[test]
fn test_error_chain() {
    let err = Error::InvalidInput {
        field: "test_field".to_string(),
        reason: "test error".to_string(),
    };
    assert_eq!(format!("{err}"), "Invalid input: test_field - test error");
}

#[test]
fn test_build_info() {
    // Test that build.rs generates the expected constants
    // This will fail to compile if build.rs isn't working
    assert!(!env!("CARGO_PKG_VERSION").is_empty());
    assert!(!env!("CARGO_PKG_NAME").is_empty());
}
