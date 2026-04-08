use anyhow::{anyhow, Result};
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
    pub headers: HashMap<String, String>,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        ConnectionConfig {
            transport_type: TransportType::Stdio,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: String::new(),
            headers: HashMap::new(),
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
            if !config.headers.is_empty() {
                entry["headers"] = json!(config.headers);
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

/// Parse the `mcpServers` map from a Claude Desktop-compatible mcp.json file.
pub fn load_mcp_servers(path: &str) -> Result<HashMap<String, Value>> {
    let content =
        std::fs::read_to_string(path).map_err(|e| anyhow!("Cannot read '{}': {}", path, e))?;
    let json: Value =
        serde_json::from_str(&content).map_err(|e| anyhow!("Invalid JSON in '{}': {}", path, e))?;
    let servers = json
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow!("'{}' has no 'mcpServers' object", path))?;
    Ok(servers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect())
}

/// Apply a server entry from an mcp.json file to the ReplState connection config.
/// Supports both stdio (`command`/`args`) and HTTP (`url`) entries.
pub fn apply_server_entry(state: &mut ReplState, key: &str, entry: &Value) -> Result<()> {
    if let Some(env_map) = entry.get("env").and_then(|v| v.as_object()) {
        for (k, v) in env_map {
            if let Some(vs) = v.as_str() {
                state.config.env.insert(k.clone(), vs.to_string());
            }
        }
    }
    state.server_name = key.to_string();
    if let Some(url) = entry.get("url").and_then(|v| v.as_str()) {
        state.config.transport_type = TransportType::Http;
        state.config.url = url.to_string();
        if let Some(headers) = entry.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in headers {
                if let Some(vs) = v.as_str() {
                    state.config.headers.insert(k.clone(), vs.to_string());
                }
            }
        }
    } else if let Some(cmd) = entry.get("command").and_then(|v| v.as_str()) {
        state.config.transport_type = TransportType::Stdio;
        state.config.command = cmd.to_string();
        state.config.args = entry
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
    } else {
        return Err(anyhow!(
            "Server entry '{}' must have either 'command' or 'url'",
            key
        ));
    }
    Ok(())
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
        assert!(entry.get("headers").is_none());
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
    fn export_config_http_with_headers() {
        let mut state = make_state();
        state.config.transport_type = TransportType::Http;
        state.config.url = "http://localhost:3000".to_string();
        state
            .config
            .headers
            .insert("X-Custom".to_string(), "value".to_string());
        let v = export_config(&state);
        let entry = &v["mcpServers"]["mcp-server"];
        assert!(entry.get("headers").is_some());
        assert_eq!(entry["headers"]["X-Custom"], "value");
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
        assert!(cfg.headers.is_empty());
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
    fn load_mcp_servers_valid() {
        let json = r#"{"mcpServers":{"fs":{"command":"npx","args":["-y","server-fs","/tmp"]}}}"#;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), json).unwrap();
        let servers = load_mcp_servers(tmp.path().to_str().unwrap()).unwrap();
        assert!(servers.contains_key("fs"));
    }

    #[test]
    fn load_mcp_servers_missing_key() {
        let json = r#"{"other":{}}"#;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), json).unwrap();
        assert!(load_mcp_servers(tmp.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn load_mcp_servers_bad_json() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "not-json").unwrap();
        assert!(load_mcp_servers(tmp.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn load_mcp_servers_missing_file() {
        assert!(load_mcp_servers("/nonexistent/path/mcp.json").is_err());
    }

    #[test]
    fn apply_server_entry_stdio() {
        let mut state = make_state();
        let entry = serde_json::json!({
            "command": "npx",
            "args": ["-y", "server-fs", "/tmp"],
            "env": {"KEY": "val"}
        });
        apply_server_entry(&mut state, "myserver", &entry).unwrap();
        assert_eq!(state.server_name, "myserver");
        assert_eq!(state.config.transport_type, TransportType::Stdio);
        assert_eq!(state.config.command, "npx");
        assert_eq!(state.config.args, vec!["-y", "server-fs", "/tmp"]);
        assert_eq!(state.config.env.get("KEY").map(|s| s.as_str()), Some("val"));
    }

    #[test]
    fn apply_server_entry_http() {
        let mut state = make_state();
        let entry = serde_json::json!({
            "url": "http://localhost:3000/mcp",
            "headers": {"Authorization": "Bearer tok"}
        });
        apply_server_entry(&mut state, "httpserver", &entry).unwrap();
        assert_eq!(state.server_name, "httpserver");
        assert_eq!(state.config.transport_type, TransportType::Http);
        assert_eq!(state.config.url, "http://localhost:3000/mcp");
        assert_eq!(
            state
                .config
                .headers
                .get("Authorization")
                .map(|s| s.as_str()),
            Some("Bearer tok")
        );
    }

    #[test]
    fn apply_server_entry_missing_command_and_url() {
        let mut state = make_state();
        let entry = serde_json::json!({"env": {"X": "1"}});
        assert!(apply_server_entry(&mut state, "bad", &entry).is_err());
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
