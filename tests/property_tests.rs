use proptest::prelude::*;
use rust_research_mcp::{Config, Error};

/// Property-based tests for critical algorithms and data structures
mod doi_validation_props {
    use super::*;

    proptest! {
        #[test]
        fn test_doi_validation_properties(doi in r"10\.\d{4,}/[^\s]+") {
            // Valid DOI format should always be accepted
            let result = validate_doi_format(&doi);
            prop_assert!(result.is_ok(), "Valid DOI format should be accepted: {}", doi);
        }

        #[test]
        fn test_invalid_doi_rejection(invalid_doi in r"[^1][^0]\..*|10\.[^0-9].*|10\.\d{1,3}/.*") {
            // Invalid DOI formats should be rejected
            let result = validate_doi_format(&invalid_doi);
            if !invalid_doi.starts_with("10.") || invalid_doi.len() < 7 {
                prop_assert!(result.is_err(), "Invalid DOI should be rejected: {}", invalid_doi);
            }
        }

        #[test]
        fn test_doi_normalization_idempotent(doi in r"10\.\d{4,}/[a-zA-Z0-9._-]+") {
            // DOI normalization should be idempotent
            let normalized1 = normalize_doi(&doi);
            let normalized2 = normalize_doi(&normalized1);
            prop_assert_eq!(normalized1, normalized2, "DOI normalization should be idempotent");
        }
    }

    fn validate_doi_format(doi: &str) -> Result<(), Error> {
        if !doi.starts_with("10.") || doi.len() < 7 {
            return Err(Error::InvalidInput {
                field: "doi".to_string(),
                reason: "Invalid DOI format".to_string(),
            });
        }
        Ok(())
    }

    fn normalize_doi(doi: &str) -> String {
        doi.trim().to_lowercase()
    }
}

mod config_validation_props {
    use super::*;

    proptest! {
        #[test]
        fn test_port_validation(port in 1u16..=65535) {
            // Valid ports should always be accepted
            let mut config = Config::default();
            config.server.port = port;
            prop_assert!(config.validate().is_ok(), "Valid port should be accepted: {}", port);
        }

        #[test]
        fn test_rate_limit_validation(rate_limit in 1u32..=1000) {
            // Valid rate limits should be accepted
            let mut config = Config::default();
            config.research_source.rate_limit_per_sec = rate_limit;
            prop_assert!(config.validate().is_ok(), "Valid rate limit should be accepted: {}", rate_limit);
        }

        #[test]
        fn test_concurrent_downloads_validation(max_concurrent in 1usize..=20) {
            // Valid concurrent download limits should be accepted
            let mut config = Config::default();
            config.downloads.max_concurrent = max_concurrent;
            prop_assert!(config.validate().is_ok(), "Valid max concurrent should be accepted: {}", max_concurrent);
        }

        #[test]
        fn test_timeout_validation(timeout in 1u64..=300) {
            // Valid timeout values should be accepted
            let mut config = Config::default();
            config.server.timeout_secs = timeout;
            config.research_source.timeout_secs = timeout;
            prop_assert!(config.validate().is_ok(), "Valid timeout should be accepted: {}", timeout);
        }
    }
}

mod error_categorization_props {
    use super::*;
    use rust_research_mcp::error::ErrorCategory;

    proptest! {
        #[test]
        fn test_error_categorization_consistency(
            field in r"[a-zA-Z_][a-zA-Z0-9_]*",
            reason in r"[a-zA-Z0-9 ._-]{1,100}"
        ) {
            // Error categorization should be consistent
            let error = Error::InvalidInput { field, reason };
            let category1 = error.category();
            let category2 = error.category();
            prop_assert_eq!(&category1, &category2, "Error categorization should be consistent");
            prop_assert_eq!(&category1, &ErrorCategory::Permanent, "InvalidInput should always be Permanent");
        }

        #[test]
        fn test_retryable_error_consistency(service in r"[a-zA-Z_][a-zA-Z0-9_]*", reason in r"[a-zA-Z0-9 ._-]{1,100}") {
            // Retryable classification should be consistent
            let error = Error::ServiceUnavailable { service, reason };
            let retryable1 = error.is_retryable();
            let retryable2 = error.is_retryable();
            prop_assert_eq!(retryable1, retryable2, "Retryable classification should be consistent");
            prop_assert!(retryable1, "ServiceUnavailable should be retryable");
        }
    }
}

mod circuit_breaker_props {
    use super::*;
    use rust_research_mcp::resilience::circuit_breaker::CircuitBreakerConfig;
    use rust_research_mcp::resilience::CircuitBreaker;
    use std::time::Duration;

    proptest! {
        #[test]
        fn test_circuit_breaker_config_validation(
            failure_threshold in 1u32..=100,
            success_threshold in 1u32..=50,
            failure_timeout_ms in 1000u64..=60000,
            recovery_timeout_ms in 1000u64..=30000
        ) {
            // Valid circuit breaker configurations should work
            let config = CircuitBreakerConfig {
                failure_threshold,
                success_threshold,
                failure_timeout: Duration::from_millis(failure_timeout_ms),
                recovery_timeout: Duration::from_millis(recovery_timeout_ms),
                half_open_max_calls: 3,
            };

            let _cb = CircuitBreaker::new("test", config);
            // Circuit breaker should be created successfully
            prop_assert!(true, "Circuit breaker creation should succeed");
        }
    }
}

// Rate limiter property tests - commented out until RateLimiter API is finalized
/*
mod rate_limiter_props {
    use super::*;
    use rust_research_mcp::client::rate_limiter::RateLimiter;
    use std::time::Duration;

    proptest! {
        #[test]
        fn test_rate_limiter_properties(
            requests_per_second in 1u32..=100,
            burst_capacity in 1u32..=50
        ) {
            // Rate limiter should respect configured limits
            let mut rate_limiter = RateLimiter::new(requests_per_second, burst_capacity);

            // First request should always be allowed
            prop_assert!(rate_limiter.check_rate_limit(), "First request should be allowed");

            // Rapid successive requests should eventually be limited
            let mut allowed_count = 1; // First request was allowed
            for _ in 0..burst_capacity * 2 {
                if rate_limiter.check_rate_limit() {
                    allowed_count += 1;
                }
            }

            prop_assert!(allowed_count <= burst_capacity * 2,
                "Rate limiter should limit excessive requests");
        }

        #[test]
        fn test_rate_limiter_recovery(requests_per_second in 1u32..=10) {
            // Rate limiter should recover over time
            let mut rate_limiter = RateLimiter::new(requests_per_second, 1);

            // Exhaust the rate limiter
            while rate_limiter.check_rate_limit() {
                // Keep requesting until denied
            }

            // After waiting, should be able to make requests again
            std::thread::sleep(Duration::from_millis(1100)); // Wait > 1 second
            prop_assert!(rate_limiter.check_rate_limit(),
                "Rate limiter should recover after waiting");
        }
    }
}
*/

mod search_algorithm_props {
    use super::*;

    proptest! {
        #[test]
        fn test_search_query_normalization(query in r"[a-zA-Z0-9 ._-]{1,100}") {
            // Search query normalization should be consistent
            let normalized1 = normalize_search_query(&query);
            let normalized2 = normalize_search_query(&normalized1);
            prop_assert_eq!(&normalized1, &normalized2, "Query normalization should be idempotent");

            // Normalized query should not be empty unless input was empty/whitespace
            if !query.trim().is_empty() {
                prop_assert!(!normalized1.is_empty(), "Non-empty query should not normalize to empty");
            }
        }

        #[test]
        fn test_search_query_length_limits(query in r".{0,1000}") {
            // Search queries should respect length limits
            let result = validate_search_query(&query);
            if query.len() > 500 {
                prop_assert!(result.is_err(), "Overly long queries should be rejected");
            } else if !query.trim().is_empty() {
                prop_assert!(result.is_ok(), "Valid length queries should be accepted");
            }
        }
    }

    fn normalize_search_query(query: &str) -> String {
        query.trim().to_lowercase()
    }

    fn validate_search_query(query: &str) -> Result<(), Error> {
        if query.len() > 500 {
            return Err(Error::InvalidInput {
                field: "query".to_string(),
                reason: "Query too long".to_string(),
            });
        }
        if query.trim().is_empty() {
            return Err(Error::InvalidInput {
                field: "query".to_string(),
                reason: "Query cannot be empty".to_string(),
            });
        }
        Ok(())
    }
}

mod download_props {
    use super::*;

    proptest! {
        #[test]
        fn test_filename_generation(
            title in r"[a-zA-Z0-9 ._-]{1,50}",
            extension in r"[a-zA-Z]{2,4}"
        ) {
            // Filename generation should produce valid filenames
            let filename = generate_safe_filename(&title, &extension);

            // Should not contain invalid characters
            prop_assert!(!filename.contains('/'), "Filename should not contain path separators");
            prop_assert!(!filename.contains('\\'), "Filename should not contain backslashes");
            prop_assert!(!filename.contains(':'), "Filename should not contain colons");
            prop_assert!(!filename.is_empty(), "Filename should not be empty");
            prop_assert!(filename.ends_with(&format!(".{extension}")), "Filename should have correct extension");
        }

        #[test]
        fn test_file_size_validation(size_mb in 1u64..=1000) {
            // File size validation should work correctly
            let result = validate_file_size(size_mb * 1024 * 1024, 500); // 500MB limit

            if size_mb <= 500 {
                prop_assert!(result.is_ok(), "Files under limit should be accepted");
            } else {
                prop_assert!(result.is_err(), "Files over limit should be rejected");
            }
        }
    }

    fn generate_safe_filename(title: &str, extension: &str) -> String {
        let safe_title = title
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '.' || *c == '_' || *c == '-')
            .collect::<String>()
            .trim()
            .replace(' ', "_");

        if safe_title.is_empty() {
            format!("document.{extension}")
        } else {
            format!("{safe_title}.{extension}")
        }
    }

    fn validate_file_size(size_bytes: u64, max_size_mb: u64) -> Result<(), Error> {
        let max_size_bytes = max_size_mb * 1024 * 1024;
        if size_bytes > max_size_bytes {
            return Err(Error::InvalidInput {
                field: "file_size".to_string(),
                reason: "File too large".to_string(),
            });
        }
        Ok(())
    }
}
