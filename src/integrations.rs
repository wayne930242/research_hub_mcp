//! Maintained adapters around upstream discovery and the canonical library API.

use crate::{Config, Error, Result};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult},
    service::{RoleClient, RunningService},
    transport::TokioChildProcess,
    Peer, ServiceExt,
};
use serde_json::{Map, Value};
use std::{process::Stdio, sync::Arc};
use tokio::{process::Command, sync::Mutex};
use tracing::{info, warn};

#[derive(Debug)]
pub struct PaperSearchClient {
    config: Arc<Config>,
    service: Mutex<Option<RunningService<RoleClient, ()>>>,
}

impl PaperSearchClient {
    #[must_use]
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            service: Mutex::new(None),
        }
    }

    async fn connect(&self) -> Result<Peer<RoleClient>> {
        let mut guard = self.service.lock().await;
        if guard.as_ref().is_none_or(RunningService::is_closed) {
            let mut command = Command::new(&self.config.paper_search.command);
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            if let Some(project_dir) = &self.config.paper_search.project_dir {
                command
                    .arg("run")
                    .arg("--project")
                    .arg(project_dir)
                    .arg("paper-search-mcp")
                    .current_dir(project_dir);
            } else {
                command.args([
                    "tool",
                    "run",
                    "--from",
                    "paper-search-mcp==0.1.4",
                    "paper-search-mcp",
                ]);
            }
            info!("Starting paper-search-mcp upstream process");
            let transport = TokioChildProcess::new(command)
                .map_err(|error| Error::Service(format!("paper-search spawn failed: {error}")))?;
            let service = ().serve(transport).await.map_err(|error| {
                Error::Service(format!("paper-search initialization failed: {error}"))
            })?;
            *guard = Some(service);
        }
        Ok(guard.as_ref().expect("service initialized").peer().clone())
    }

    pub async fn call(
        &self,
        tool: &'static str,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult> {
        let peer = self.connect().await?;
        match peer
            .call_tool(CallToolRequestParams::new(tool).with_arguments(arguments))
            .await
        {
            Ok(result) => Ok(result),
            Err(error) => {
                warn!("paper-search call failed; resetting child process: {error}");
                self.service.lock().await.take();
                Err(Error::Service(format!("paper-search call failed: {error}")))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LibraryClient {
    http: reqwest::Client,
    api_url: String,
}

impl LibraryClient {
    #[must_use]
    pub fn new(config: &Config) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_url: config.library.api_url.trim_end_matches('/').to_string(),
        }
    }

    pub async fn save_entries(&self, entries: Value) -> Result<Value> {
        self.post(
            "/api/entries/batch",
            serde_json::json!({ "entries": entries }),
        )
        .await
    }

    pub async fn export_bibtex(&self, keys: Option<Vec<String>>) -> Result<Value> {
        self.post("/api/entries/export", serde_json::json!({ "keys": keys }))
            .await
    }

    async fn post(&self, path: &str, payload: Value) -> Result<Value> {
        let response = self
            .http
            .post(format!("{}{path}", self.api_url))
            .json(&payload)
            .send()
            .await
            .map_err(|error| Error::Service(format!("library API unavailable: {error}")))?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            Error::Service(format!("failed to read library API response: {error}"))
        })?;
        if !status.is_success() {
            return Err(Error::Service(format!(
                "library API returned {status}: {body}"
            )));
        }
        serde_json::from_str(&body)
            .map_err(|error| Error::Service(format!("invalid library API response: {error}")))
    }
}
