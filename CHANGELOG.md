# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.8] - 2026-10-02

### Security

- `save_papers_to_library` authenticates to the library API: it exchanges
  `RSH_LIBRARY_ADMIN_TOKEN` for an admin session and sends it as a bearer token.
  Without the variable, saving fails before any request.

## [0.6.6] - 2025-01-06

### Fixed
- **ArXiv URL Resolution**: Fixed "relative URL without a base" errors by implementing proper URL resolution logic
- **Sci-Hub Access Issues**: Enhanced 403 error handling with mirror rotation and user agent cycling
- **Connection Errors**: Improved network error handling and context for better user feedback
- **PDF Metadata Validation**: Added comprehensive PDF file validation before parsing attempts
- **Error Categorization**: Changed Sci-Hub 403 errors from permanent to transient for better retry behavior

### Enhanced
- **Mirror Management**: Updated Sci-Hub mirrors with more recent working URLs
- **User Agent Rotation**: Implemented realistic browser user agent rotation for better access
- **Error Messages**: Significantly improved error messages with actionable suggestions
- **Test Coverage**: Added comprehensive unit, integration, and E2E tests for error scenarios
- **Resilience**: Better handling of transient failures with appropriate retry logic

### Added
- New integration tests for download cascade behavior
- E2E tests for full download and metadata extraction flow
- Comprehensive error categorization tests
- PDF validation with magic byte checking

## [0.3.0] - 2025-08-17

### Added
- Multi-provider search system with plugin architecture
- arXiv provider for computer science and physics papers
- CrossRef provider for DOI and metadata search
- Configurable download directory via CLI and environment variables
- User-friendly error messages with helpful suggestions
- Graceful failure handling that doesn't interrupt Claude's workflow
- Enhanced search results with source, year, and DOI information
- E2E test suites for MCP protocol compliance

### Changed
- Simplified JSON schemas for better Claude Desktop compatibility
- Improved error handling to return informative messages instead of failures
- Updated search results format to be more informative
- Refactored from single Sci-Hub source to multi-provider architecture

### Fixed
- Claude Desktop tool invocation issues with complex schemas
- Download failures now provide helpful alternatives
- MCP protocol compliance for error responses

## [0.2.0] - 2025-08-16

### Added
- Model Context Protocol (MCP) server implementation
- Integration with Claude Desktop
- search_papers tool for academic paper discovery
- download_paper tool for PDF retrieval
- extract_metadata tool for bibliographic information
- Resilience features (circuit breakers, retries)
- Comprehensive test coverage

### Changed
- Migrated from standalone CLI to MCP server architecture
- Updated to use rmcp SDK for MCP protocol support

## [0.1.0] - 2025-08-15

### Added
- Initial release
- Basic Sci-Hub integration
- Command-line interface
- PDF download functionality
- Mirror discovery and failover
- Rate limiting for respectful access

---

For a complete list of changes, see the [commit history](https://github.com/yourusername/rust-research-mcp/commits/main).