use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::protocol::client::McpClient;
use crate::protocol::{Notification, ServerCapabilities};
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
    pub bearer_token: Option<String>,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        ConnectionConfig {
            transport_type: TransportType::Stdio,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: String::new(),
            bearer_token: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::client::McpClient;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{mpsc, Mutex};

    fn make_state() -> ReplState {
        ReplState::new(CompleterState::new())
    }

    #[test]
    fn export_config_stdio_no_env() {
        let state = make_state();
        let v = export_config(&state);
        let entry = &v["mcpServers"]["mcp-server"];
        assert!(entry.get("command").is_some());
        assert!(entry.get("args").is_some());
        assert!(entry.get("env").is_none());
    }

    #[test]
    fn export_config_stdio_with_env() {
        let mut state = make_state();
        state
            .config
            .env
            .insert("MY_VAR".to_string(), "val".to_string());
        let v = export_config(&state);
        let entry = &v["mcpServers"]["mcp-server"];
        assert!(entry.get("env").is_some());
        assert_eq!(entry["env"]["MY_VAR"], "val");
    }

    #[test]
    fn export_config_http_no_env() {
        let mut state = make_state();
        state.config.transport_type = TransportType::Http;
        state.config.url = "http://localhost:3000".to_string();
        let v = export_config(&state);
        let entry = &v["mcpServers"]["mcp-server"];
        assert!(entry.get("url").is_some());
        assert!(entry.get("command").is_none());
        assert!(entry.get("env").is_none());
    }

    #[test]
    fn export_config_http_with_env() {
        let mut state = make_state();
        state.config.transport_type = TransportType::Http;
        state.config.url = "http://localhost:3000".to_string();
        state
            .config
            .env
            .insert("API_KEY".to_string(), "secret".to_string());
        let v = export_config(&state);
        let entry = &v["mcpServers"]["mcp-server"];
        assert!(entry.get("env").is_some());
        assert_eq!(entry["env"]["API_KEY"], "secret");
    }

    #[test]
    fn export_config_uses_server_name_as_key() {
        let mut state = make_state();
        state.server_name = "my-custom-server".to_string();
        let v = export_config(&state);
        assert!(v["mcpServers"].get("my-custom-server").is_some());
        assert!(v["mcpServers"].get("mcp-server").is_none());
    }

    #[test]
    fn connection_config_default_values() {
        let cfg = ConnectionConfig::default();
        assert_eq!(cfg.transport_type, TransportType::Stdio);
        assert!(cfg.command.is_empty());
        assert!(cfg.args.is_empty());
        assert!(cfg.env.is_empty());
        assert!(cfg.url.is_empty());
        assert!(cfg.bearer_token.is_none());
    }

    #[test]
    fn repl_state_is_connected_false() {
        let state = make_state();
        assert!(!state.is_connected());
    }

    #[tokio::test]
    async fn repl_state_is_connected_true() {
        let (tx, rx) = mpsc::channel::<String>(1);
        let (notif_tx, _notif_rx) = mpsc::channel(1);
        let caps = Arc::new(Mutex::new(HashMap::new()));
        let client = McpClient::new(tx, rx, notif_tx, 10, caps, false);
        let mut state = make_state();
        state.client = Some(client);
        assert!(state.is_connected());
    }

    #[tokio::test]
    async fn completer_state_new_empty() {
        let cs = CompleterState::new();
        assert!(cs.tools.lock().await.is_empty());
        assert!(cs.resources.lock().await.is_empty());
        assert!(cs.prompts.lock().await.is_empty());
    }

    #[test]
    fn write_config_to_file() {
        let state = make_state();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        drop(tmp);
        write_config(&state, Some(&path)).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(val.get("mcpServers").is_some());
    }

    #[test]
    fn write_config_no_panic_stdout() {
        let state = make_state();
        assert!(write_config(&state, None).is_ok());
    }
}
