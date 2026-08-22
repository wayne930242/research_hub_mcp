use rust_research_mcp::Result;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use tempfile::TempDir;

/// Test all MCP functionality in a single connection
#[test]
#[allow(clippy::too_many_lines)]
fn test_mcp_server_full_flow() -> Result<()> {
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

    // Helper to send and receive
    let send_and_receive = |stdin: &mut std::process::ChildStdin,
                            stdout: &mut BufReader<std::process::ChildStdout>,
                            request: Value|
     -> Result<Value> {
        let request_str = serde_json::to_string(&request)?;
        writeln!(stdin, "{request_str}")?;
        stdin.flush()?;

        let mut response_line = String::new();
        stdout.read_line(&mut response_line)?;

        if response_line.trim().is_empty() {
            return Err(rust_research_mcp::Error::Parse {
                context: "response".to_string(),
                message: "Empty response".to_string(),
            });
        }

        Ok(serde_json::from_str(&response_line)?)
    };

    // Test 1: Initialize
    println!("Test 1: Initialize");
    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "clientInfo": {
                "name": "e2e-test",
                "version": "1.0.0"
            }
        }
    });

    let response = send_and_receive(&mut stdin, &mut stdout_reader, init_request)?;
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(
        response["result"]["serverInfo"]["name"],
        "rust_research_mcp"
    );
    println!("✓ Initialization successful");

    // Send initialized notification (required by MCP protocol)
    let initialized_notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    });

    let notification_str = serde_json::to_string(&initialized_notification)?;
    writeln!(stdin, "{notification_str}")?;
    stdin.flush()?;

    // Test 2: List tools
    println!("\nTest 2: List tools");
    let list_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });

    let response = send_and_receive(&mut stdin, &mut stdout_reader, list_request)?;
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 2);

    let tools = response["result"]["tools"].as_array().unwrap();
    assert!(tools.len() >= 4);

    let tool_names: Vec<String> = tools
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();

    assert!(tool_names.contains(&"debug_test".to_string()));
    assert!(tool_names.contains(&"search_papers".to_string()));
    assert!(tool_names.contains(&"download_paper".to_string()));
    println!("✓ Tools listed: {tool_names:?}");

    // Test 3: Debug tool
    println!("\nTest 3: Debug tool");
    let debug_request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "debug_test",
            "arguments": {
                "message": "Hello from E2E test"
            }
        }
    });

    let response = send_and_receive(&mut stdin, &mut stdout_reader, debug_request)?;
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 3);

    let content = &response["result"]["content"][0];
    assert_eq!(content["type"], "text");
    let text = content["text"].as_str().unwrap();
    assert!(text.contains("Debug echo: Hello from E2E test"));
    println!("✓ Debug tool works: {text}");

    // Test 4: Search papers
    println!("\nTest 4: Search papers");
    let search_request = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "search_papers",
            "arguments": {
                "query": "machine learning",
                "limit": 2
            }
        }
    });

    let response = send_and_receive(&mut stdin, &mut stdout_reader, search_request)?;
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 4);

    if let Some(error) = response.get("error") {
        println!("⚠ Search returned error (network issues acceptable): {error:?}");
    } else {
        let content = &response["result"]["content"][0];
        let text = content["text"].as_str().unwrap();
        println!("✓ Search completed: {}", text.lines().next().unwrap_or(""));
    }

    // Test 5: Simplified schemas
    println!("\nTest 5: Verify simplified schemas");
    let list_request = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/list",
        "params": {}
    });

    let response = send_and_receive(&mut stdin, &mut stdout_reader, list_request)?;
    let tools = response["result"]["tools"].as_array().unwrap();

    for tool in tools {
        let schema = &tool["inputSchema"];
        let schema_str = serde_json::to_string(schema)?;

        if tool["name"] == "search_papers" || tool["name"] == "download_paper" {
            assert!(
                !schema_str.contains("$defs"),
                "Schema should not contain $defs"
            );
            assert!(
                !schema_str.contains("$ref"),
                "Schema should not contain $ref"
            );
        }
    }
    println!("✓ Schemas are simplified (no $ref or $defs)");

    // Test 6: Error handling
    println!("\nTest 6: Error handling");
    let bad_request = json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "tools/call",
        "params": {
            "name": "search_papers",
            "arguments": {} // Missing required 'query'
        }
    });

    let response = send_and_receive(&mut stdin, &mut stdout_reader, bad_request)?;
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 6);
    assert!(response.get("error").is_some());
    println!("✓ Error handling works");

    // Clean up
    child.kill()?;

    println!("\n=== All E2E tests passed! ===");
    Ok(())
}

/// Test with custom download directory
#[test]
fn test_custom_download_directory() -> Result<()> {
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
    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "clientInfo": {"name": "test", "version": "1.0"}
        }
    });

    let request_str = serde_json::to_string(&init_request)?;
    writeln!(stdin, "{request_str}")?;
    stdin.flush()?;

    let mut response_line = String::new();
    stdout_reader.read_line(&mut response_line)?;

    let response: Value = serde_json::from_str(&response_line)?;
    assert_eq!(
        response["result"]["serverInfo"]["name"],
        "rust_research_mcp"
    );

    println!("✓ Server started with custom download directory: {download_dir}");

    // Clean up
    child.kill()?;

    Ok(())
}

/// Test multiple sequential calls
#[test]
fn test_sequential_operations() -> Result<()> {
    let mut child = Command::new("cargo")
        .args(["run", "--release", "--", "--log-level", "error"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stdout_reader = BufReader::new(stdout);

    // Initialize
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "clientInfo": {"name": "test", "version": "1.0"}
        }
    });

    writeln!(stdin, "{}", serde_json::to_string(&init)?)?;
    stdin.flush()?;

    let mut line = String::new();
    stdout_reader.read_line(&mut line)?;
    assert!(line.contains("rust_research_mcp"));

    // Send initialized notification (required by MCP protocol)
    let initialized_notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    });
    writeln!(
        stdin,
        "{}",
        serde_json::to_string(&initialized_notification)?
    )?;
    stdin.flush()?;

    // Make multiple sequential debug calls
    for i in 2..5 {
        let debug_req = json!({
            "jsonrpc": "2.0",
            "id": i,
            "method": "tools/call",
            "params": {
                "name": "debug_test",
                "arguments": {
                    "message": format!("Test {}", i)
                }
            }
        });

        writeln!(stdin, "{}", serde_json::to_string(&debug_req)?)?;
        stdin.flush()?;

        let mut response_line = String::new();
        stdout_reader.read_line(&mut response_line)?;

        let response: Value = serde_json::from_str(&response_line)?;
        assert_eq!(response["id"], i);

        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains(&format!("Test {i}")));
    }

    println!("✓ Sequential operations completed successfully");

    // Clean up
    child.kill()?;

    Ok(())
}
