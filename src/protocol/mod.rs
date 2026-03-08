use serde::{Deserialize, Serialize};
use serde_json::Value;

// JSON-RPC 2.0 wire types

/// A request sent *from the server* to the client (e.g. roots/list, sampling/createMessage).
/// Uses `Value` for `id` because servers may send integer or string ids.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonRpcServerRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: String,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

// MCP domain types
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpResource {
    pub uri: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "mimeType", default)]
    pub mime_type: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpPrompt {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub arguments: Vec<McpPromptArgument>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpContent {
    Text { text: String },
    Image { data: String, #[serde(rename = "mimeType")] mime_type: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpPromptMessage {
    pub role: String,
    pub content: McpContent,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ServerCapabilities {
    pub tools: Option<Value>,
    pub resources: Option<Value>,
    pub prompts: Option<Value>,
    pub logging: Option<Value>,
}

// Internal notification enum
#[derive(Debug, Clone)]
pub enum Notification {
    Log { level: String, message: String },
    ToolListChanged,
    ResourceListChanged,
    PromptListChanged,
    /// A request the server sent to us. `responded` is false when no handler was configured.
    ServerRequest { method: String, params: Option<Value>, responded: bool },
    Unknown { method: String, params: Option<Value> },
}

impl Notification {
    pub fn from_jsonrpc(notif: &JsonRpcNotification) -> Self {
        match notif.method.as_str() {
            "notifications/message" => {
                if let Some(params) = &notif.params {
                    let level = params.get("level")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info")
                        .to_string();
                    let message = params.get("data")
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    Notification::Log { level, message }
                } else {
                    Notification::Unknown { method: notif.method.clone(), params: notif.params.clone() }
                }
            }
            "notifications/tools/list_changed" => Notification::ToolListChanged,
            "notifications/resources/list_changed" => Notification::ResourceListChanged,
            "notifications/prompts/list_changed" => Notification::PromptListChanged,
            _ => Notification::Unknown { method: notif.method.clone(), params: notif.params.clone() },
        }
    }
}

pub mod client;
