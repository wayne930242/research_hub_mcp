use rust_research_mcp::Result;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::TempDir;

/// Helper to send JSON-RPC request and get response (synchronous)
fn send_request_sync(
    stdin: &mut std::process::ChildStdin,
    stdout: &mut BufReader<std::process::ChildStdout>,
    request: &Value,
) -> Result<Value> {
    // Send request
    let request_str = serde_json::to_string(&request)?;
    writeln!(stdin, "{request_str}")?;
    stdin.flush()?;

    // Read response with timeout
    let mut response_line = String::new();

    // Use a simple timeout mechanism
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(5) {
            return Err(rust_research_mcp::Error::Timeout {
                timeout: Duration::from_secs(5),
            });
        }

        match stdout.read_line(&mut response_line) {
            Ok(0) => {
                // EOF reached
                return Err(rust_research_mcp::Error::Parse {
                    context: "response".to_string(),
                    message: "Server closed connection".to_string(),
                });
            }
            Ok(_) => {
                // Got a line, parse it
                if !response_line.trim().is_empty() {
                    let response: Value = serde_json::from_str(&response_line)?;
                    return Ok(response);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Not ready yet, continue
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(rust_research_mcp::Error::Io(e)),
        }
    }
}

/// Helper to perform complete MCP initialization handshake
fn initialize_mcp_server(
    stdin: &mut std::process::ChildStdin,
    stdout: &mut BufReader<std::process::ChildStdout>,
) -> Result<Value> {
    // Send initialize request
    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "clientInfo": {"name": "e2e-test", "version": "1.0.0"}
        }
    });

    let response = send_request_sync(stdin, stdout, &init_request)?;

    // Send initialized notification (required by MCP protocol)
    let initialized_notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    });

    let notification_str = serde_json::to_string(&initialized_notification)?;
    writeln!(stdin, "{notification_str}")?;
    stdin.flush()?;

    // Give the server a moment to process the notification
    std::thread::sleep(Duration::from_millis(100));

    Ok(response)
}

#[tokio::test]
async fn test_mcp_server_initialization() -> Result<()> {
    // Start MCP server process
    let mut child = Command::new("cargo")
        .args(["run", "--release", "--", "--log-level", "debug"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stdout_reader = BufReader::new(stdout);

    // Test initialization
    let response = initialize_mcp_server(&mut stdin, &mut stdout_reader)?;

    // Verify initialization response
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert!(response["result"]["protocolVersion"].is_string());
    assert_eq!(
        response["result"]["serverInfo"]["name"],
        "rust_research_mcp"
    );
    assert!(response["result"]["capabilities"]["tools"].is_object());

    // Clean up
    child.kill()?;

    Ok(())
}

#[tokio::test]
async fn test_tools_list() -> Result<()> {
    // Start MCP server process
    let mut child = Command::new("cargo")
        .args(["run", "--release", "--", "--log-level", "info"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stdout_reader = BufReader::new(stdout);

    // Initialize first
    let _ = initialize_mcp_server(&mut stdin, &mut stdout_reader)?;

    // List tools
    let list_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });

    let response = send_request_sync(&mut stdin, &mut stdout_reader, &list_request)?;

    // Verify tools list
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 2);

    let tools = response["result"]["tools"].as_array().unwrap();
    assert!(tools.len() >= 4); // Should have at least 4 tools

    // Check for expected tools
    let tool_names: Vec<String> = tools
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();

    assert!(tool_names.contains(&"debug_test".to_string()));
    assert!(tool_names.contains(&"search_papers".to_string()));
    assert!(tool_names.contains(&"download_paper".to_string()));
    assert!(tool_names.contains(&"extract_metadata".to_string()));

    // Clean up
    child.kill()?;

    Ok(())
}

#[tokio::test]
async fn test_debug_tool() -> Result<()> {
    // Start MCP server process
    let mut child = Command::new("cargo")
        .args(["run", "--release"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stdout_reader = BufReader::new(stdout);

    // Initialize
    let _ = initialize_mcp_server(&mut stdin, &mut stdout_reader)?;

    // Call debug tool
    let debug_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "debug_test",
            "arguments": {
                "message": "Hello from E2E test"
            }
        }
    });

    let response = send_request_sync(&mut stdin, &mut stdout_reader, &debug_request)?;

    // Verify debug tool response
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 2);

    let content = &response["result"]["content"][0];
    assert_eq!(content["type"], "text");
    assert!(content["text"]
        .as_str()
        .unwrap()
        .contains("Debug echo: Hello from E2E test"));

    // Clean up
    child.kill()?;

    Ok(())
}

#[tokio::test]
async fn test_search_papers_tool() -> Result<()> {
    // Start MCP server process
    let mut child = Command::new("cargo")
        .args(["run", "--release"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stdout_reader = BufReader::new(stdout);

    // Initialize
    let _ = initialize_mcp_server(&mut stdin, &mut stdout_reader)?;

    // Search for papers
    let search_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "search_papers",
            "arguments": {
                "query": "quantum computing",
                "limit": 2
            }
        }
    });

    let response = send_request_sync(&mut stdin, &mut stdout_reader, &search_request)?;

    // Verify search response
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 2);

    let content = &response["result"]["content"][0];
    assert_eq!(content["type"], "text");

    let text = content["text"].as_str().unwrap();
    assert!(text.contains("Found") || text.contains("papers") || text.contains("No results"));

    // Clean up
    child.kill()?;

    Ok(())
}

#[tokio::test]
async fn test_download_paper_with_custom_directory() -> Result<()> {
    // Create temporary directory for downloads
    let temp_dir = TempDir::new()?;
    let download_dir = temp_dir.path().to_string_lossy().to_string();

    // Start MCP server with custom download directory
    let mut child = Command::new("cargo")
        .args([
            "run",
            "--release",
            "--",
            "--download-dir",
            &download_dir,
            "--log-level",
            "info",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stdout_reader = BufReader::new(stdout);

    // Initialize
    let _ = initialize_mcp_server(&mut stdin, &mut stdout_reader)?;

    // Try to download a paper (using a known test DOI)
    let download_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "download_paper",
            "arguments": {
                "doi": "10.1038/nature12373",
                "filename": "test_paper.pdf"
            }
        }
    });

    let response = send_request_sync(&mut stdin, &mut stdout_reader, &download_request)?;

    // Verify download response (may fail if paper not available, but should not error)
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 2);

    // Check if error or success
    if let Some(error) = response.get("error") {
        // If error, should be a service error (not a schema error)
        println!("Download failed (expected in test): {error:?}");
    } else {
        // If success, verify response structure
        let content = &response["result"]["content"][0];
        assert_eq!(content["type"], "text");

        let text = content["text"].as_str().unwrap();
        assert!(
            !text.trim().is_empty(),
            "download response must not be empty"
        );

        // The upstream wrapper returns the downloaded path directly on success.
        // Public providers may also return an explanatory failure message, so only
        // enforce the directory contract when a file was actually downloaded.
        let downloaded_path = std::path::Path::new(text.trim());
        if downloaded_path.is_file() {
            assert!(
                downloaded_path.starts_with(temp_dir.path()),
                "downloaded file must stay inside the configured directory: {text}"
            );
        }
    }

    // Clean up
    child.kill()?;

    Ok(())
}

#[tokio::test]
async fn test_simplified_schema_compatibility() -> Result<()> {
    // Start MCP server process
    let mut child = Command::new("cargo")
        .args(["run", "--release"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stdout_reader = BufReader::new(stdout);

    // Initialize
    let _ = initialize_mcp_server(&mut stdin, &mut stdout_reader)?;

    // List tools to get schemas
    let list_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });

    let response = send_request_sync(&mut stdin, &mut stdout_reader, &list_request)?;

    let tools = response["result"]["tools"].as_array().unwrap();

    // Verify simplified schemas don't have $defs or $ref
    for tool in tools {
        let schema = &tool["inputSchema"];
        let schema_str = serde_json::to_string(schema)?;

        // Check that complex schema elements are not present
        if tool["name"] == "search_papers" || tool["name"] == "download_paper" {
            assert!(
                !schema_str.contains("$defs"),
                "Schema should not contain $defs"
            );
            assert!(
                !schema_str.contains("$ref"),
                "Schema should not contain $ref"
            );
            assert!(
                schema_str.contains("\"type\":\"object\""),
                "Schema should be a simple object"
            );
            assert!(
                schema_str.contains("\"properties\""),
                "Schema should have properties"
            );
        }
    }

    // Clean up
    child.kill()?;

    Ok(())
}

#[tokio::test]
async fn test_multiple_sequential_calls() -> Result<()> {
    // Start MCP server process
    let mut child = Command::new("cargo")
        .args(["run", "--release"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stdout_reader = BufReader::new(stdout);

    // Initialize
    let _ = initialize_mcp_server(&mut stdin, &mut stdout_reader)?;

    // Make multiple sequential calls
    for i in 2..5 {
        let debug_request = json!({
            "jsonrpc": "2.0",
            "id": i,
            "method": "tools/call",
            "params": {
                "name": "debug_test",
                "arguments": {
                    "message": format!("Test message {}", i)
                }
            }
        });

        let response = send_request_sync(&mut stdin, &mut stdout_reader, &debug_request)?;

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], i);

        let content = &response["result"]["content"][0];
        let text = content["text"].as_str().unwrap();
        assert!(text.contains(&format!("Test message {i}")));
    }

    // Clean up
    child.kill()?;

    Ok(())
}

#[tokio::test]
async fn test_error_handling() -> Result<()> {
    // Start MCP server process
    let mut child = Command::new("cargo")
        .args(["run", "--release"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stdout_reader = BufReader::new(stdout);

    // Initialize
    let _ = initialize_mcp_server(&mut stdin, &mut stdout_reader)?;

    // Test missing required parameter
    let bad_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "search_papers",
            "arguments": {} // Missing required 'query' parameter
        }
    });

    let response = send_request_sync(&mut stdin, &mut stdout_reader, &bad_request)?;

    // Should return an error
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 2);
    assert!(response.get("error").is_some());

    let error = &response["error"];
    assert!(error["message"].as_str().unwrap().contains("query"));

    // Test unknown tool
    let unknown_tool = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "unknown_tool",
            "arguments": {}
        }
    });

    let response = send_request_sync(&mut stdin, &mut stdout_reader, &unknown_tool)?;

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 3);
    assert!(response.get("error").is_some());

    // Clean up
    child.kill()?;

    Ok(())
}
