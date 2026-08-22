use rust_research_mcp::{integrations::LibraryClient, Config};
use serde_json::json;
use wiremock::{
    matchers::{body_json, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn selected_entries_are_saved_through_the_library_api() {
    let server = MockServer::start().await;
    let entries = json!([{
        "key": "fine1994essence",
        "entry_type": "article",
        "fields": {"title": "Essence and Modality"}
    }]);
    Mock::given(method("POST"))
        .and(path("/api/entries/batch"))
        .and(body_json(json!({"entries": entries.clone()})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"saved": 1})))
        .expect(1)
        .mount(&server)
        .await;

    let mut config = Config::default();
    config.library.api_url = server.uri();
    let result = LibraryClient::new(&config)
        .save_entries(entries)
        .await
        .unwrap();

    assert_eq!(result["saved"], 1);
}

#[tokio::test]
async fn bibliography_export_is_requested_by_database_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/entries/export"))
        .and(body_json(json!({"keys": ["fine1994essence"]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": "@article{fine1994essence,}\n"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut config = Config::default();
    config.library.api_url = server.uri();
    let result = LibraryClient::new(&config)
        .export_bibtex(Some(vec!["fine1994essence".to_string()]))
        .await
        .unwrap();

    assert!(result["content"]
        .as_str()
        .unwrap()
        .contains("fine1994essence"));
}
