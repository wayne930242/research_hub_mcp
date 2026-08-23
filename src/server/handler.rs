use crate::integrations::{LibraryClient, PaperSearchClient};
use crate::tools::{
    code_search::CodeSearchInput,
    download::{
        BatchDownloadInput as ActualBatchDownloadInput, DownloadInput as ActualDownloadInput,
    },
    metadata::MetadataInput as ActualMetadataInput,
    search::{SearchInput as ActualSearchInput, SearchResult},
};
use crate::{
    CodeSearchTool, Config, DownloadTool, MetaSearchClient, MetadataExtractor, Result, SearchTool,
};
use chrono::Utc;
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
    },
    service::{RequestContext, RoleServer},
    ErrorData, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    future::Future,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::sync::RwLock;
use tracing::{debug, info, instrument, warn};

// Tool input structures
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchInput {
    /// Query string - can be DOI, title, or author name
    pub query: String,
    /// Maximum number of results to return (default: 10)
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Offset for pagination (default: 0)
    #[serde(default)]
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DownloadInput {
    /// DOI or URL of the paper to download
    pub identifier: String,
    /// Optional output directory
    pub output_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MetadataInput {
    /// Path to the PDF file or DOI
    pub input: String,
}

/// Cache entry for paper categories from recent searches
#[derive(Debug, Clone)]
struct CategoryCacheEntry {
    category: Option<String>,
    timestamp: SystemTime,
}

/// Main MCP server handler implementing rmcp
#[derive(Debug)]
pub struct ResearchServerHandler {
    #[allow(dead_code)]
    config: Arc<Config>,
    search_tool: Arc<SearchTool>,
    download_tool: Arc<DownloadTool>,
    metadata_extractor: Arc<MetadataExtractor>,
    code_search_tool: Arc<CodeSearchTool>,
    paper_search_client: Option<Arc<PaperSearchClient>>,
    library_client: Arc<LibraryClient>,
    /// Cache of DOI -> Category mappings from recent searches
    category_cache: Arc<RwLock<HashMap<String, CategoryCacheEntry>>>,
}

impl ResearchServerHandler {
    pub fn new(config: Arc<Config>) -> Result<Self> {
        info!("Initializing Research MCP server handler");

        // Initialize MetaSearch client with config
        let meta_config = crate::client::MetaSearchConfig::from_config(&config);
        let client = Arc::new(MetaSearchClient::new((*config).clone(), meta_config)?);

        // Initialize search tool
        let search_tool = SearchTool::new(config.clone())?;

        // Initialize download tool
        let download_tool = DownloadTool::new(client, config.clone())?;

        // Initialize metadata extractor
        let metadata_extractor = MetadataExtractor::new(config.clone())?;

        // Initialize code search tool
        let code_search_tool = CodeSearchTool::new(config.clone())?;

        let paper_search_client = config
            .paper_search
            .enabled
            .then(|| Arc::new(PaperSearchClient::new(config.clone())));
        let library_client = Arc::new(LibraryClient::new(&config));

        Ok(Self {
            config,
            search_tool: Arc::new(search_tool),
            download_tool: Arc::new(download_tool),
            metadata_extractor: Arc::new(metadata_extractor),
            code_search_tool: Arc::new(code_search_tool),
            paper_search_client,
            library_client,
            category_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Health check for the server
    #[instrument(skip(self))]
    pub async fn ping(&self) -> Result<()> {
        debug!("Ping received - server is healthy");
        Ok(())
    }

    /// Cache category information from search results
    async fn cache_paper_categories(&self, results: &SearchResult) {
        let mut cache = self.category_cache.write().await;
        let now = SystemTime::now();

        for paper in &results.papers {
            if !paper.metadata.doi.is_empty() {
                if let Some(category) = &paper.category {
                    debug!(
                        "Caching category '{}' for DOI '{}'",
                        category, paper.metadata.doi
                    );
                    cache.insert(
                        paper.metadata.doi.clone(),
                        CategoryCacheEntry {
                            category: Some(category.clone()),
                            timestamp: now,
                        },
                    );
                }
            }
        }

        // Clean up old entries (older than 1 hour)
        let one_hour_ago = now - Duration::from_secs(3600);
        cache.retain(|doi, entry| {
            if entry.timestamp < one_hour_ago {
                debug!("Removing expired cache entry for DOI '{}'", doi);
                false
            } else {
                true
            }
        });
    }

    /// Get cached category for a DOI
    async fn get_cached_category(&self, doi: &str) -> Option<String> {
        let cache = self.category_cache.read().await;
        if let Some(entry) = cache.get(doi) {
            debug!(
                "Found cached category '{}' for DOI '{}'",
                entry.category.as_deref().unwrap_or("None"),
                doi
            );
            entry.category.clone()
        } else {
            debug!("No cached category found for DOI '{}'", doi);
            None
        }
    }

    fn tool_response(content: impl Into<String>, is_error: bool) -> CallToolResponse {
        let content = vec![ContentBlock::text(content)];
        if is_error {
            CallToolResult::error(content).into()
        } else {
            CallToolResult::success(content).into()
        }
    }
}

fn save_papers_to_library_schema() -> serde_json::Map<String, serde_json::Value> {
    serde_json::json!({
        "type": "object",
        "properties": {
            "entries": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "key": {"type": "string"},
                        "entry_type": {"type": "string", "default": "article"},
                        "academic_fields": {
                            "type": "array",
                            "minItems": 1,
                            "items": {
                                "type": "string",
                                "pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$"
                            },
                            "description": "One or more lowercase kebab-case academic field slugs, such as philosophy"
                        },
                        "fields": {"type": "object", "additionalProperties": {"type": "string"}},
                        "notes": {"type": "string"}
                    },
                    "required": ["key", "academic_fields", "fields"]
                }
            }
        },
        "required": ["entries"]
    })
    .as_object()
    .unwrap()
    .clone()
}

impl ServerHandler for ResearchServerHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "rust_research_mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(format!("Research Hub MCP v{} is the maintained policy wrapper around paper-search-mcp and the canonical bibliography database. Search is discovery-only. Save selected results explicitly with save_papers_to_library; generate_bibliography exports BibTeX from stored database records. Downloads use open-access fallback by default.", env!("CARGO_PKG_VERSION")))
    }

    #[instrument(skip(self, _request, _context))]
    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListToolsResult, ErrorData>> + Send + '_ {
        info!("Listing available tools");

        async move {
            let tools = vec![
                Tool::new(
                    "debug_test",
                    "Simple test tool for debugging - just echoes back what it receives",
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "message": {
                                "type": "string",
                                "description": "Test message to echo back"
                            }
                        },
                        "required": ["message"]
                    }).as_object().unwrap().clone(),
                ),
                Tool::new(
                    "search_papers",
                    "Search papers through the maintained paper-search-mcp wrapper. Results are discovery-only until explicitly saved.",
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Query string - can be DOI, title, or author name"
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum number of results to return",
                                "default": 10,
                                "minimum": 1,
                                "maximum": 100
                            },
                            "sources": {
                                "type": "string",
                                "description": "Comma-separated upstream sources, or 'all'",
                                "default": "all"
                            },
                            "year": {
                                "type": "string",
                                "description": "Optional year filter where supported"
                            }
                        },
                        "required": ["query"]
                    }).as_object().unwrap().clone(),
                ),
                Tool::new(
                    "download_paper",
                    "Download a paper through paper-search-mcp's OA-first fallback chain.",
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "doi": {
                                "type": "string",
                                "description": "DOI of the paper to download (e.g., '10.1038/nature12373')"
                            },
                            "filename": {
                                "type": "string", 
                                "description": "Optional custom filename for the downloaded PDF"
                            },
                            "source": {"type": "string", "default": "crossref"},
                            "paper_id": {"type": "string", "description": "Source-native identifier; defaults to DOI"},
                            "title": {"type": "string"},
                            "use_scihub": {"type": "boolean", "default": false}
                        },
                        "required": ["doi"]
                    }).as_object().unwrap().clone(),
                ),
                Tool::new(
                    "download_papers_batch",
                    "Download multiple papers concurrently (MAX 100 papers per batch, 1-20 concurrent). For >100 papers, split into multiple batches. 5-10x faster than individual downloads.",
                    serde_json::to_value(schemars::schema_for!(ActualBatchDownloadInput)).unwrap().as_object().unwrap().clone(),
                ),
                Tool::new(
                    "extract_metadata",
                    "Extract metadata from PDF files. Single file or batch processing (12 concurrent for batch_files array). Returns title, authors, DOI, abstract, etc.",
                    serde_json::to_value(schemars::schema_for!(ActualMetadataInput)).unwrap().as_object().unwrap().clone(),
                ),
                Tool::new(
                    "search_code",
                    "Search for code patterns within downloaded research papers using regex",
                    serde_json::to_value(schemars::schema_for!(CodeSearchInput)).unwrap().as_object().unwrap().clone(),
                ),
                Tool::new(
                    "generate_bibliography",
                    "Generate BibTeX on demand from selected keys already stored in the canonical bibliography database.",
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "keys": {
                                "type": "array",
                                "items": {"type": "string"},
                                "description": "Citation keys to export in order; omit for the full library"
                            }
                        }
                    }).as_object().unwrap().clone(),
                ),
                Tool::new(
                    "save_papers_to_library",
                    "Explicitly save selected discovery results, including their academic fields, to the canonical bibliography database.",
                    save_papers_to_library_schema(),
                ),
            ];

            Ok(ListToolsResult::with_all_items(tools))
        }
    }

    #[instrument(skip(self, request, _context))]
    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<CallToolResponse, ErrorData>> + Send + '_ {
        info!("Tool called: {}", request.name);

        let search_tool = Arc::clone(&self.search_tool);
        let download_tool = Arc::clone(&self.download_tool);
        let metadata_extractor = Arc::clone(&self.metadata_extractor);
        let code_search_tool = Arc::clone(&self.code_search_tool);
        let paper_search_client = self.paper_search_client.clone();
        let library_client = Arc::clone(&self.library_client);

        async move {
            match request.name.as_ref() {
                "debug_test" => {
                    info!("Debug tool called with arguments: {:?}", request.arguments);
                    let message = request
                        .arguments
                        .and_then(|args| {
                            args.get("message")
                                .and_then(|v| v.as_str())
                                .map(str::to_string)
                        })
                        .unwrap_or_else(|| "No message provided".to_string());

                    Ok(Self::tool_response(format!("Debug echo: {message}"), false))
                }
                "search_papers" => {
                    // Simple parsing for simplified schema
                    let args = request.arguments.unwrap_or_default();
                    let query = args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
                        ErrorData::invalid_params(
                            "Missing required 'query' parameter".to_string(),
                            None,
                        )
                    })?;
                    let limit = args
                        .get("limit")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(10) as u32;

                    if let Some(client) = &paper_search_client {
                        let sources = args
                            .get("sources")
                            .and_then(|value| value.as_str())
                            .unwrap_or("all");
                        let mut upstream_args = serde_json::json!({
                            "query": query,
                            "max_results_per_source": limit.clamp(1, 20),
                            "sources": sources,
                        })
                        .as_object()
                        .expect("object")
                        .clone();
                        if let Some(year) = args.get("year").and_then(|value| value.as_str()) {
                            upstream_args.insert("year".to_string(), year.into());
                        }
                        match client.call("search_papers", upstream_args).await {
                            Ok(result) => return Ok(result.into()),
                            Err(error) => warn!(
                                "paper-search wrapper unavailable, using legacy provider fallback: {error}"
                            ),
                        }
                    }

                    let input = ActualSearchInput {
                        query: query.to_string(),
                        search_type: crate::tools::search::SearchType::Auto,
                        limit,
                        offset: 0,
                    };

                    let results = search_tool.search_papers(input).await.map_err(|e| {
                        ErrorData::internal_error(format!("Search failed: {e}"), None)
                    })?;

                    // Cache the category information for each paper
                    self.cache_paper_categories(&results).await;

                    Ok(Self::tool_response(format!("📚 Found {} papers for '{}'\n\n{}\n\n💡 Tip: Papers from {} may be available for download. Very recent papers (2024-2025) might not be available yet.",
                            results.returned_count,
                            results.query,
                            results.papers.iter().enumerate().map(|(i, p)| {
                                let doi_info = if p.metadata.doi.is_empty() {
                                    "\n  ⚠️ No DOI available (cannot download)".to_string()
                                } else {
                                    format!("\n  📖 DOI: {doi}", doi = p.metadata.doi)
                                };
                                let source_info = format!("\n  🔍 Source: {source}", source = p.source);
                                let year = p.metadata.year.filter(|y| *y > 0)
                                    .map(|y| format!("\n  📅 Year: {y}"))
                                    .unwrap_or_default();
                                format!("{}. {} (Relevance: {:.0}%){}{}{}",
                                    i + 1,
                                    p.metadata.title.as_deref().unwrap_or("No title"),
                                    p.relevance_score * 100.0,
                                    doi_info,
                                    source_info,
                                    year
                                )
                            }).collect::<Vec<_>>().join("\n\n"),
                            results.papers.iter().filter(|p| !p.metadata.doi.is_empty()).count()
                        ), false))
                }
                "download_paper" => {
                    // Simple parsing for simplified schema
                    let args = request.arguments.unwrap_or_default();
                    let doi = args.get("doi").and_then(|v| v.as_str()).ok_or_else(|| {
                        ErrorData::invalid_params(
                            "Missing required 'doi' parameter".to_string(),
                            None,
                        )
                    })?;
                    let filename = args
                        .get("filename")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);

                    if let Some(client) = &paper_search_client {
                        let source = args
                            .get("source")
                            .and_then(|value| value.as_str())
                            .unwrap_or("crossref");
                        let paper_id = args
                            .get("paper_id")
                            .and_then(|value| value.as_str())
                            .unwrap_or(doi);
                        let title = args
                            .get("title")
                            .and_then(|value| value.as_str())
                            .unwrap_or("");
                        let use_scihub = args
                            .get("use_scihub")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false);
                        let upstream_args = serde_json::json!({
                            "source": source,
                            "paper_id": paper_id,
                            "doi": doi,
                            "title": title,
                            "save_path": self.config.downloads.directory.to_string_lossy(),
                            "use_scihub": use_scihub,
                        })
                        .as_object()
                        .expect("object")
                        .clone();
                        match client.call("download_with_fallback", upstream_args).await {
                            Ok(result) => return Ok(result.into()),
                            Err(error) => warn!(
                                "paper-search download unavailable, using legacy fallback: {error}"
                            ),
                        }
                    }

                    // Look up category from recent search results
                    let category = self.get_cached_category(doi).await;

                    let input = ActualDownloadInput {
                        doi: Some(doi.to_string()),
                        url: None,
                        filename,
                        directory: None,
                        category,
                        overwrite: false,
                        verify_integrity: true,
                    };

                    debug!("Attempting download with input: {:?}", input);
                    match download_tool.download_paper(input).await {
                        Ok(result) => {
                            debug!("Download result received: {:?}", result.status);
                            debug!(
                                "File size: {:?}, file path: {:?}",
                                result.file_size, result.file_path
                            );

                            // Validate that the file actually has content
                            let file_size = result.file_size.unwrap_or(0);
                            if file_size == 0 {
                                debug!("Download succeeded but file size is 0 - cleaning up");
                                // Clean up zero-byte file if it exists
                                if let Some(file_path) = &result.file_path {
                                    if file_path.exists() {
                                        debug!("Removing zero-byte file: {:?}", file_path);
                                        let _ = std::fs::remove_file(file_path);
                                    }
                                }
                                Ok(Self::tool_response(format!("⚠️ Download failed - no content received\n\nDOI: {doi}\n\n🔍 Debug Info:\n• Download ID: {}\n• Duration: {:.2}s\n• Status: {:?}\n• File created but empty\n\nThe paper was found but no downloadable content is available. This could be because:\n• The paper is too new or recently published\n• It's behind a paywall not covered by available sources\n• The DOI might be incorrect\n• Network issues during download\n\nTry checking the publisher's website or your institutional access.",
                                    result.download_id, result.duration_seconds, result.status), true))
                            } else {
                                debug!("Download successful - file size: {} bytes", file_size);
                                let duration_info = if result.duration_seconds > 0.0 {
                                    format!(
                                        "\n⏱️ Time: {:.1}s\n🚀 Speed: {:.1} KB/s",
                                        result.duration_seconds,
                                        result.average_speed as f64 / 1024.0
                                    )
                                } else {
                                    String::new()
                                };

                                let hash_info = result
                                    .sha256_hash
                                    .map(|h| format!("\n🔐 SHA256: {}...", &h[..16]))
                                    .unwrap_or_default();

                                Ok(Self::tool_response(format!("✅ Download successful!\n\n📄 File: {}\n📦 Size: {} KB{}{}",
                                        result.file_path.as_ref().map_or("Unknown".to_string(), |p| p.display().to_string()),
                                        file_size / 1024, duration_info, hash_info), false))
                            }
                        }
                        Err(e) => {
                            debug!("Download failed with error: {}", e);
                            debug!("Error type: {:?}", std::any::type_name_of_val(&e));

                            // Generate timestamp for debugging
                            let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");

                            // Return a helpful error message with debug information
                            let error_msg = match e.to_string().as_str() {
                                msg if msg.contains("No PDF available")
                                    || msg.contains("not found in any provider") =>
                                {
                                    format!("⚠️ Paper not available for download\n\n\
                                            DOI: {doi}\n\n\
                                            🔍 Debug Info:\n\
                                            • Time: {}\n\
                                            • Error: {}\n\
                                            • All available sources checked\n\n\
                                            This paper is not currently available through any source. This could be because:\n\
                                            • The paper is too new (published recently)\n\
                                            • It's from a publisher not covered by available sources\n\
                                            • The DOI might be incorrect or malformed\n\
                                            • Temporary service unavailability\n\n\
                                            💡 Alternatives:\n\
                                            • Try searching for the paper on Google Scholar\n\
                                            • Check if your institution has access\n\
                                            • Try arXiv or other preprint servers\n\
                                            • Contact the authors directly\n\
                                            • Verify the DOI is correct", timestamp, msg)
                                }
                                msg if msg.contains("Network")
                                    || msg.contains("timeout")
                                    || msg.contains("Connection") =>
                                {
                                    format!(
                                        "⚠️ Network error while downloading\n\n\
                                            DOI: {doi}\n\n\
                                            🔍 Debug Info:\n\
                                            • Time: {}\n\
                                            • Error: {}\n\
                                            • Network connectivity issue detected\n\n\
                                            Please check your internet connection and try again.\n\
                                            If the problem persists, the source servers may be temporarily unavailable.",
                                        timestamp, msg
                                    )
                                }
                                msg if msg.contains("Permission")
                                    || msg.contains("Claude Desktop") =>
                                {
                                    format!(
                                        "⚠️ File system permission error\n\n\
                                            DOI: {doi}\n\n\
                                            🔍 Debug Info:\n\
                                            • Time: {}\n\
                                            • Error: {}\n\n\
                                            This appears to be a permission issue with accessing the download directory.\n\
                                            Please check the error message for specific instructions to resolve.",
                                        timestamp, msg
                                    )
                                }
                                _ => {
                                    format!(
                                        "⚠️ Download failed\n\n\
                                            DOI: {doi}\n\n\
                                            🔍 Debug Info:\n\
                                            • Time: {}\n\
                                            • Error Type: {}\n\
                                            • Error: {}\n\n\
                                            Please try again or use a different DOI. If this error persists,\n\
                                            it may indicate an issue with the paper source or network connectivity.",
                                        timestamp, std::any::type_name_of_val(&e), e
                                    )
                                }
                            };
                            Ok(Self::tool_response(error_msg, true))
                        }
                    }
                }
                "download_papers_batch" => {
                    let input: ActualBatchDownloadInput = serde_json::from_value(
                        serde_json::Value::Object(request.arguments.unwrap_or_default()),
                    )
                    .map_err(|e| {
                        ErrorData::invalid_params(
                            format!("Invalid batch download input: {e}"),
                            None,
                        )
                    })?;

                    debug!("Starting batch download with {} papers", input.papers.len());
                    match download_tool.download_papers_batch(input).await {
                        Ok(result) => {
                            debug!(
                                "Batch download completed: {}/{} successful",
                                result.summary.successful, result.summary.total_requested
                            );

                            let success_rate = if result.summary.total_requested > 0 {
                                (result.summary.successful as f64
                                    / result.summary.total_requested as f64)
                                    * 100.0
                            } else {
                                0.0
                            };

                            let mut content = format!(
                                "✅ Batch Download Complete!\n\n\
                                📊 Summary:\n\
                                • Total requested: {}\n\
                                • Successful: {} ({:.1}%)\n\
                                • Failed: {}\n\
                                • Skipped: {}\n\
                                • Total time: {:.1}s\n\
                                • Total data: {:.1} MB\n\
                                • Average speed: {:.1} KB/s\n",
                                result.summary.total_requested,
                                result.summary.successful,
                                success_rate,
                                result.summary.failed,
                                result.summary.skipped,
                                result.total_duration_seconds,
                                result.summary.total_bytes as f64 / 1_048_576.0, // Convert to MB
                                result.summary.average_speed as f64 / 1024.0     // Convert to KB/s
                            );

                            // Add details about successful downloads
                            let successful_downloads: Vec<_> = result
                                .results
                                .iter()
                                .filter(|r| r.result.is_some())
                                .collect();

                            if !successful_downloads.is_empty() {
                                content.push_str("\n📁 Downloaded Papers:\n");
                                for item in successful_downloads.iter().take(10) {
                                    // Limit to first 10 for readability
                                    if let Some(ref download_result) = item.result {
                                        if let Some(ref file_path) = download_result.file_path {
                                            let file_name = file_path
                                                .file_name()
                                                .and_then(|name| name.to_str())
                                                .unwrap_or("unknown");
                                            let size_mb = download_result.file_size.unwrap_or(0)
                                                as f64
                                                / 1_048_576.0;
                                            content.push_str(&format!(
                                                "• {} ({:.1} MB)\n",
                                                file_name, size_mb
                                            ));
                                        }
                                    }
                                }
                                if successful_downloads.len() > 10 {
                                    content.push_str(&format!(
                                        "• ... and {} more files\n",
                                        successful_downloads.len() - 10
                                    ));
                                }
                            }

                            // Add error details if there were failures
                            if result.summary.failed > 0 && !result.summary.failed_items.is_empty()
                            {
                                content.push_str("\n❌ Failed Downloads:\n");
                                for failed_item in result.summary.failed_items.iter().take(5) {
                                    // Limit to first 5
                                    content.push_str(&format!("• {}\n", failed_item));
                                }
                                if result.summary.failed_items.len() > 5 {
                                    content.push_str(&format!(
                                        "• ... and {} more failures\n",
                                        result.summary.failed_items.len() - 5
                                    ));
                                }
                            }

                            Ok(Self::tool_response(
                                content,
                                result.summary.failed > result.summary.successful,
                            ))
                        }
                        Err(e) => {
                            debug!("Batch download failed: {}", e);
                            let error_msg = format!(
                                "⚠️ Batch download failed\n\n\
                                Error: {}\n\n\
                                This could be due to:\n\
                                • Invalid input parameters\n\
                                • Network connectivity issues\n\
                                • Resource constraints\n\
                                • Provider limitations\n\n\
                                Please check your input and try again.",
                                e
                            );

                            Ok(Self::tool_response(error_msg, true))
                        }
                    }
                }
                "extract_metadata" => {
                    let input: ActualMetadataInput = serde_json::from_value(
                        serde_json::Value::Object(request.arguments.unwrap_or_default()),
                    )
                    .map_err(|e| {
                        ErrorData::invalid_params(format!("Invalid metadata input: {e}"), None)
                    })?;

                    let result = metadata_extractor
                        .extract_metadata(input)
                        .await
                        .map_err(|e| {
                            ErrorData::internal_error(
                                format!("Metadata extraction failed: {e}"),
                                None,
                            )
                        })?;

                    Ok(Self::tool_response(
                        serde_json::to_string_pretty(&result).map_err(|e| {
                            ErrorData::internal_error(format!("Serialization failed: {e}"), None)
                        })?,
                        false,
                    ))
                }
                "search_code" => {
                    let input: CodeSearchInput = serde_json::from_value(serde_json::Value::Object(
                        request.arguments.unwrap_or_default(),
                    ))
                    .map_err(|e| {
                        ErrorData::invalid_params(format!("Invalid code search input: {e}"), None)
                    })?;

                    let results = code_search_tool.search(input).await.map_err(|e| {
                        ErrorData::internal_error(format!("Code search failed: {e}"), None)
                    })?;

                    if results.is_empty() {
                        Ok(Self::tool_response(
                            "🔍 No code patterns found matching your search criteria.",
                            false,
                        ))
                    } else {
                        let formatted_results = results
                            .iter()
                            .map(|result| {
                                let matches_text = result
                                    .matches
                                    .iter()
                                    .take(5) // Limit to first 5 matches per file
                                    .map(|m| {
                                        let context_before = if m.context_before.is_empty() {
                                            String::new()
                                        } else {
                                            format!("  {}\n", m.context_before.join("\n  "))
                                        };

                                        let context_after = if m.context_after.is_empty() {
                                            String::new()
                                        } else {
                                            format!("\n  {}", m.context_after.join("\n  "))
                                        };

                                        let lang_info = m
                                            .language
                                            .as_ref()
                                            .map(|l| format!(" [{l}]"))
                                            .unwrap_or_default();

                                        format!(
                                            "{}► Line {}{}: {}{}",
                                            context_before,
                                            m.line_number,
                                            lang_info,
                                            m.line,
                                            context_after
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n\n");

                                let title_info = result
                                    .paper_title
                                    .as_ref()
                                    .map(|t| format!("📄 Paper: {t}\n"))
                                    .unwrap_or_default();

                                format!(
                                    "📁 File: {}\n{}🎯 {} matches found:\n\n{}",
                                    result.file_path,
                                    title_info,
                                    result.total_matches,
                                    matches_text
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(&format!("\n\n{}\n\n", "─".repeat(60)));

                        Ok(Self::tool_response(
                            format!(
                                "🔍 Found {} files with matching code patterns:\n\n{}",
                                results.len(),
                                formatted_results
                            ),
                            false,
                        ))
                    }
                }
                "generate_bibliography" => {
                    let args = request.arguments.unwrap_or_default();
                    let keys = args
                        .get("keys")
                        .cloned()
                        .map(serde_json::from_value::<Vec<String>>)
                        .transpose()
                        .map_err(|error| {
                            ErrorData::invalid_params(
                                format!("Invalid citation keys: {error}"),
                                None,
                            )
                        })?;
                    let result = library_client.export_bibtex(keys).await.map_err(|error| {
                        ErrorData::internal_error(
                            format!("Database-derived bibliography export failed: {error}"),
                            None,
                        )
                    })?;
                    Ok(Self::tool_response(
                        serde_json::to_string_pretty(&result).map_err(|error| {
                            ErrorData::internal_error(
                                format!("Bibliography response serialization failed: {error}"),
                                None,
                            )
                        })?,
                        false,
                    ))
                }
                "save_papers_to_library" => {
                    let args = request.arguments.unwrap_or_default();
                    let entries = args.get("entries").cloned().ok_or_else(|| {
                        ErrorData::invalid_params(
                            "Missing required 'entries' parameter".to_string(),
                            None,
                        )
                    })?;
                    let result = library_client
                        .save_entries(entries)
                        .await
                        .map_err(|error| {
                            ErrorData::internal_error(
                                format!("Saving selected papers failed: {error}"),
                                None,
                            )
                        })?;
                    Ok(Self::tool_response(
                        serde_json::to_string_pretty(&result).map_err(|error| {
                            ErrorData::internal_error(
                                format!("Library response serialization failed: {error}"),
                                None,
                            )
                        })?,
                        false,
                    ))
                }
                _ => Err(ErrorData::invalid_request(
                    format!("Unknown tool: {}", request.name),
                    None,
                )),
            }
        }
    }
}

/// Default limit for search results
const fn default_limit() -> u32 {
    10
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_handler() -> ResearchServerHandler {
        let config = Config::default();
        ResearchServerHandler::new(Arc::new(config)).unwrap()
    }

    #[tokio::test]
    async fn test_handler_creation() {
        let handler = create_test_handler();
        assert!(!handler.config.research_source.endpoints.is_empty());
    }

    #[tokio::test]
    async fn test_ping() {
        let handler = create_test_handler();
        let result = handler.ping().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_search_input_validation() {
        let input = SearchInput {
            query: "test".to_string(),
            limit: 10,
            offset: 0,
        };
        assert_eq!(input.query, "test");
        assert_eq!(input.limit, 10);
        assert_eq!(input.offset, 0);
    }

    #[test]
    fn save_papers_schema_requires_academic_fields() {
        let schema = serde_json::Value::Object(save_papers_to_library_schema());
        let item = &schema["properties"]["entries"]["items"];

        assert_eq!(
            item["required"],
            serde_json::json!(["key", "academic_fields", "fields"])
        );
        assert_eq!(
            item["properties"]["academic_fields"]["items"]["pattern"],
            "^[a-z0-9]+(?:-[a-z0-9]+)*$"
        );
    }
}
