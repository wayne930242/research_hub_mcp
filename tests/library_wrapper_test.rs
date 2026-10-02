use rust_research_mcp::{integrations::LibraryClient, Config};
use serde_json::json;
use wiremock::{
    matchers::{body_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn selected_entries_are_saved_through_the_library_api() {
    let server = MockServer::start().await;
    let entries = json!([{
        "key": "fine1994essence",
        "entry_type": "article",
        "academic_fields": ["philosophy"],
        "fields": {"title": "Essence and Modality"}
    }]);
    Mock::given(method("POST"))
        .and(path("/api/admin/session"))
        .and(body_json(json!({"credential": "admin-credential"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token": "session-token",
            "expires_at": "2026-10-02T12:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/entries/batch"))
        .and(header("authorization", "Bearer session-token"))
        .and(body_json(json!({"entries": entries.clone()})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"saved": 1})))
        .expect(1)
        .mount(&server)
        .await;

    let mut config = Config::default();
    config.library.api_url = server.uri();
    config.library.admin_token = Some("admin-credential".to_string());
    let result = LibraryClient::new(&config)
        .save_entries(entries)
        .await
        .unwrap();

    assert_eq!(result["saved"], 1);
}

#[tokio::test]
async fn saving_without_an_admin_token_fails_before_any_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let mut config = Config::default();
    config.library.api_url = server.uri();
    let error = LibraryClient::new(&config)
        .save_entries(json!([]))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("RSH_LIBRARY_ADMIN_TOKEN"));
}

#[test]
fn library_admin_token_is_not_exposed_by_debug_or_serialization() {
    let mut config = Config::default();
    config.library.admin_token = Some("admin-credential".to_string());

    assert!(!format!("{:?}", config.library).contains("admin-credential"));
    assert!(!serde_json::to_string(&config.library)
        .unwrap()
        .contains("admin-credential"));
    let client = LibraryClient::new(&config);
    assert!(!format!("{client:?}").contains("admin-credential"));
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
