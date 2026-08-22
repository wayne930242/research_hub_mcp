// pub mod command_integration;
pub mod handler;
pub mod transport;

use crate::{Config, Error, Result};
use rmcp::{service::ServiceExt, transport::stdio};
use std::sync::Arc;
use tokio::signal;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

// pub use command_integration::CommandIntegratedHandler;
pub use handler::ResearchServerHandler;

pub struct Server {
    config: Arc<Config>,
    cancellation_token: CancellationToken,
}

impl Server {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            cancellation_token: CancellationToken::new(),
        }
    }

    #[must_use]
    pub fn new_with_arc(config: Arc<Config>) -> Self {
        Self {
            config,
            cancellation_token: CancellationToken::new(),
        }
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting MCP server infrastructure");

        // Create server handler with dependency injection
        let handler = ResearchServerHandler::new(Arc::clone(&self.config))?;

        // Validate transport setup
        transport::validate_stdio_transport()
            .map_err(|e| Error::Service(format!("Transport validation failed: {e}")))?;

        info!("MCP server handler initialized successfully");

        // Setup signal handlers
        let shutdown_token = self.cancellation_token.clone();
        tokio::spawn(async move {
            let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to setup SIGTERM handler");
            let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
                .expect("Failed to setup SIGINT handler");

            tokio::select! {
                _ = sigterm.recv() => {
                    info!("Received SIGTERM, initiating graceful shutdown");
                }
                _ = sigint.recv() => {
                    info!("Received SIGINT, initiating graceful shutdown");
                }
            }

            shutdown_token.cancel();
        });

        // Start MCP server with stdio transport
        info!("Starting MCP server on stdio transport");

        let server_result = tokio::select! {
            result = self.run_mcp_server(handler) => {
                result
            }
            () = self.cancellation_token.cancelled() => {
                info!("Shutdown signal received, stopping MCP server");
                Ok(())
            }
        };

        // Graceful shutdown with timeout
        let shutdown_timeout =
            tokio::time::Duration::from_secs(self.config.server.graceful_shutdown_timeout_secs);
        if tokio::time::timeout(shutdown_timeout, self.graceful_shutdown())
            .await
            .is_err()
        {
            warn!("Graceful shutdown timeout exceeded, forcing shutdown");
        }

        info!("MCP server shutdown complete");
        server_result
    }

    async fn run_mcp_server(&self, handler: ResearchServerHandler) -> Result<()> {
        info!("Connecting MCP server to stdio transport");

        // Create stdio transport
        let transport = stdio();

        // Serve the MCP server
        let server = Box::pin(handler.serve(transport))
            .await
            .map_err(|e| Error::Service(format!("Failed to start MCP server: {e}")))?;

        // Wait for the server to complete
        let quit_reason = server
            .waiting()
            .await
            .map_err(|e| Error::Service(format!("MCP server error: {e}")))?;

        info!("MCP server completed with reason: {:?}", quit_reason);
        Ok(())
    }

    async fn graceful_shutdown(&self) -> Result<()> {
        info!("Performing graceful shutdown");

        // Here we would:
        // 1. Stop accepting new requests
        // 2. Wait for existing requests to complete
        // 3. Close connections cleanly
        // 4. Save any necessary state

        // For now, just a placeholder
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        info!("Graceful shutdown completed");
        Ok(())
    }

    pub async fn shutdown(&self) {
        warn!("Initiating server shutdown");
        self.cancellation_token.cancel();

        // Give a moment for cleanup to begin
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    /// Check if the server has been requested to shutdown
    #[must_use]
    pub fn is_shutdown_requested(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }

    /// Get the server configuration
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let config = Config::default();
        let server = Server::new(config);
        assert!(!server.cancellation_token.is_cancelled());
    }

    #[tokio::test]
    async fn test_server_shutdown() {
        let config = Config::default();
        let server = Server::new(config);

        server.shutdown().await;
        assert!(server.cancellation_token.is_cancelled());
    }
}
