use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};


use crate::protocol::{Notification, ServerCapabilities};
use crate::protocol::client::McpClient;
use crate::transport::stdio::StdioTransport;

#[derive(Debug, Clone, PartialEq)]
pub enum TransportType {
    Stdio,
    Http,
}

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub transport_type: TransportType,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub url: String,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        ConnectionConfig {
            transport_type: TransportType::Stdio,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: String::new(),
        }
    }
}

pub struct CompleterState {
    pub tools: Mutex<Vec<String>>,
    pub resources: Mutex<Vec<String>>,
    pub prompts: Mutex<Vec<String>>,
}

impl CompleterState {
    pub fn new() -> Arc<Self> {
        Arc::new(CompleterState {
            tools: Mutex::new(Vec::new()),
            resources: Mutex::new(Vec::new()),
            prompts: Mutex::new(Vec::new()),
        })
    }
}

pub const DEFAULT_TIMEOUT_SECS: u64 = 10;

pub struct ReplState {
    pub client: Option<McpClient>,
    pub stdio_transport: Option<StdioTransport>,
    pub config: ConnectionConfig,
    pub server_name: String,
    pub capabilities: Option<ServerCapabilities>,
    pub notification_rx: Option<mpsc::Receiver<Notification>>,
    pub pending_notifications: Vec<Notification>,
    pub completer_state: Arc<CompleterState>,
    pub history: Vec<String>,
    pub timeout_secs: u64,
    pub debug: bool,
    /// Configured client capability handlers: method → fixed JSON response.
    /// Shared with McpClient so the reader task can respond to server requests at runtime.
    pub client_capabilities: Arc<Mutex<HashMap<String, Value>>>,
}

impl ReplState {
    pub fn new(completer_state: Arc<CompleterState>) -> Self {
        ReplState {
            client: None,
            stdio_transport: None,
            config: ConnectionConfig::default(),
            server_name: "mcp-server".to_string(),
            capabilities: None,
            notification_rx: None,
            pending_notifications: Vec::new(),
            completer_state,
            history: Vec::new(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            debug: false,
            client_capabilities: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.client.is_some()
    }
}

/// Export Claude Desktop-compatible JSON configuration
pub fn export_config(state: &ReplState) -> Value {
    let config = &state.config;
    let server_entry = match config.transport_type {
        TransportType::Http => {
            let mut entry = json!({ "url": config.url });
            if !config.env.is_empty() {
                entry["env"] = json!(config.env);
            }
            entry
        }
        TransportType::Stdio => {
            let mut entry = json!({
                "command": config.command,
                "args": config.args,
            });
            if !config.env.is_empty() {
                entry["env"] = json!(config.env);
            }
            entry
        }
    };

    json!({
        "mcpServers": {
            state.server_name.clone(): server_entry
        }
    })
}

pub fn write_config(state: &ReplState, filename: Option<&str>) -> Result<()> {
    let config_json = export_config(state);
    let pretty = serde_json::to_string_pretty(&config_json)?;

    match filename {
        Some(path) => {
            std::fs::write(path, &pretty)?;
            println!("Config written to {path}");
        }
        None => {
            println!("{pretty}");
        }
    }
    Ok(())
}
