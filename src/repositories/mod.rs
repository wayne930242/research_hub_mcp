//! # Repository Pattern Implementation
//!
//! This module implements the repository pattern to provide data access abstraction.
//! It separates data access logic from business logic, making the codebase more
//! maintainable and testable.
//!
//! ## Architecture
//!
//! The repository pattern consists of:
//! - **Traits**: Define the interface for data access operations
//! - **Implementations**: Concrete implementations for different storage backends
//! - **In-Memory**: Fast implementations for testing and caching
//! - **Persistent**: Implementations that persist data to disk or databases
//!
//! ## Repository Types
//!
//! - [`PaperRepository`]: Manages paper metadata and content
//! - [`CacheRepository`]: Provides caching for frequently accessed data
//! - [`ConfigRepository`]: Handles configuration storage and retrieval
//!
//! ## Usage Example
//!
//! ```no_run
//! use rust_research_mcp::repositories::{PaperRepository, InMemoryPaperRepository};
//! use rust_research_mcp::client::PaperMetadata;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let repository = InMemoryPaperRepository::new();
//!
//! let paper = PaperMetadata::new("10.1000/example".to_string());
//! repository.store(&paper).await?;
//!
//! let retrieved = repository.find_by_doi("10.1000/example").await?;
//! assert!(retrieved.is_some());
//! # Ok(())
//! # }
//! ```

pub mod cache;
pub mod config;
pub mod paper;

// Re-export the main traits and types
pub use cache::{CacheEntry, CacheRepository, InMemoryCacheRepository};
pub use config::{ConfigRepository, InMemoryConfigRepository};
pub use paper::{InMemoryPaperRepository, PaperFilter, PaperQuery, PaperRepository};

use crate::Result;
use async_trait::async_trait;
use std::fmt::Debug;

/// Common repository error types
#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Entity not found: {entity_type} with id {id}")]
    NotFound { entity_type: String, id: String },

    #[error("Storage error: {message}")]
    Storage { message: String },

    #[error("Serialization error: {message}")]
    Serialization { message: String },

    #[error("Validation error: {field} - {message}")]
    Validation { field: String, message: String },

    #[error("Constraint violation: {message}")]
    ConstraintViolation { message: String },
}

/// Repository result type
pub type RepositoryResult<T> = std::result::Result<T, RepositoryError>;

/// Base trait for all repositories providing common functionality
#[async_trait]
pub trait Repository: Send + Sync + Debug {
    /// Returns the name of the repository for logging and debugging
    fn name(&self) -> &'static str;

    /// Performs health check on the repository
    async fn health_check(&self) -> RepositoryResult<bool>;

    /// Clears all data from the repository (primarily for testing)
    async fn clear(&self) -> RepositoryResult<()>;

    /// Returns statistics about the repository
    async fn stats(&self) -> RepositoryResult<RepositoryStats>;
}

/// Statistics about repository usage and performance
#[derive(Debug, Clone, Default)]
pub struct RepositoryStats {
    /// Total number of entities stored
    pub total_entities: u64,
    /// Number of successful operations
    pub successful_operations: u64,
    /// Number of failed operations
    pub failed_operations: u64,
    /// Average operation time in milliseconds
    pub avg_operation_time_ms: f64,
    /// Memory usage in bytes (for in-memory repositories)
    pub memory_usage_bytes: Option<u64>,
    /// Storage size on disk in bytes (for persistent repositories)
    pub storage_size_bytes: Option<u64>,
}

impl RepositoryStats {
    /// Create new empty stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a successful operation with timing
    pub fn record_success(&mut self, duration_ms: f64) {
        self.successful_operations += 1;
        self.update_avg_time(duration_ms);
    }

    /// Record a failed operation with timing
    pub fn record_failure(&mut self, duration_ms: f64) {
        self.failed_operations += 1;
        self.update_avg_time(duration_ms);
    }

    /// Update average operation time using exponential moving average
    fn update_avg_time(&mut self, new_time_ms: f64) {
        let total_ops = self.successful_operations + self.failed_operations;
        if total_ops == 1 {
            self.avg_operation_time_ms = new_time_ms;
        } else {
            // Use exponential moving average with alpha = 0.1
            let alpha = 0.1;
            self.avg_operation_time_ms =
                alpha * new_time_ms + (1.0 - alpha) * self.avg_operation_time_ms;
        }
    }

    /// Calculate success rate as a percentage
    pub fn success_rate(&self) -> f64 {
        let total = self.successful_operations + self.failed_operations;
        if total == 0 {
            0.0
        } else {
            (self.successful_operations as f64 / total as f64) * 100.0
        }
    }
}
