use rust_research_mcp::{integrations::PaperSearchClient, Config};
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires the paper-search Python environment and internet access"]
async fn wrapper_calls_pinned_paper_search_submodule() {
    let config = Arc::new(Config::default());
    assert!(config.paper_search.project_dir.is_some());
    let client = PaperSearchClient::new(config);
    let arguments = serde_json::json!({
        "query": "modal logic",
        "max_results_per_source": 1,
        "sources": "crossref"
    })
    .as_object()
    .unwrap()
    .clone();

    let result = client.call("search_papers", arguments).await.unwrap();
    assert!(!result.content.is_empty());
    assert!(!result.is_error.unwrap_or(false));
}
