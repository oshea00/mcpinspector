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
    /// May be a string or integer depending on the server implementation.
    pub id: Value,
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
pub struct McpResourceTemplate {
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
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
    Text {
        text: String,
    },
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
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
    Log {
        level: String,
        message: String,
    },
    ToolListChanged,
    ResourceListChanged,
    PromptListChanged,
    /// A request the server sent to us. `responded` is false when no handler was configured.
    ServerRequest {
        method: String,
        params: Option<Value>,
        responded: bool,
    },
    Unknown {
        method: String,
        params: Option<Value>,
    },
}

impl Notification {
    pub fn from_jsonrpc(notif: &JsonRpcNotification) -> Self {
        match notif.method.as_str() {
            "notifications/message" => {
                if let Some(params) = &notif.params {
                    let level = params
                        .get("level")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info")
                        .to_string();
                    let message = params
                        .get("data")
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    Notification::Log { level, message }
                } else {
                    Notification::Unknown {
                        method: notif.method.clone(),
                        params: notif.params.clone(),
                    }
                }
            }
            "notifications/tools/list_changed" => Notification::ToolListChanged,
            "notifications/resources/list_changed" => Notification::ResourceListChanged,
            "notifications/prompts/list_changed" => Notification::PromptListChanged,
            _ => Notification::Unknown {
                method: notif.method.clone(),
                params: notif.params.clone(),
            },
        }
    }
}

pub mod client;

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn mcp_resource_template_deserialization() {
        let json_str = r#"{"uriTemplate":"weather://{location}","name":"Weather","mimeType":"text/plain","description":"Weather data"}"#;
        let t: McpResourceTemplate = serde_json::from_str(json_str).unwrap();
        assert_eq!(t.uri_template, "weather://{location}");
        assert_eq!(t.name, "Weather");
        assert_eq!(t.mime_type, "text/plain");
    }

    #[test]
    fn mcp_resource_template_defaults() {
        let json_str = r#"{"uriTemplate":"foo://{id}"}"#;
        let t: McpResourceTemplate = serde_json::from_str(json_str).unwrap();
        assert_eq!(t.uri_template, "foo://{id}");
        assert!(t.name.is_empty());
        assert!(t.mime_type.is_empty());
        assert!(t.description.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn notification_log_message() {
        let notif = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "notifications/message".to_string(),
            params: Some(json!({"level": "error", "data": "something went wrong"})),
        };
        match Notification::from_jsonrpc(&notif) {
            Notification::Log { level, message } => {
                assert_eq!(level, "error");
                assert!(message.contains("something went wrong"));
            }
            _ => panic!("Expected Notification::Log"),
        }
    }

    #[test]
    fn notification_log_missing_params() {
        let notif = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "notifications/message".to_string(),
            params: None,
        };
        assert!(matches!(
            Notification::from_jsonrpc(&notif),
            Notification::Unknown { .. }
        ));
    }

    #[test]
    fn notification_tools_list_changed() {
        let notif = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "notifications/tools/list_changed".to_string(),
            params: None,
        };
        assert!(matches!(
            Notification::from_jsonrpc(&notif),
            Notification::ToolListChanged
        ));
    }

    #[test]
    fn notification_resources_list_changed() {
        let notif = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "notifications/resources/list_changed".to_string(),
            params: None,
        };
        assert!(matches!(
            Notification::from_jsonrpc(&notif),
            Notification::ResourceListChanged
        ));
    }

    #[test]
    fn notification_prompts_list_changed() {
        let notif = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "notifications/prompts/list_changed".to_string(),
            params: None,
        };
        assert!(matches!(
            Notification::from_jsonrpc(&notif),
            Notification::PromptListChanged
        ));
    }

    #[test]
    fn notification_unknown_method() {
        let notif = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "notifications/custom/event".to_string(),
            params: Some(json!({"key": "value"})),
        };
        match Notification::from_jsonrpc(&notif) {
            Notification::Unknown { method, .. } => {
                assert_eq!(method, "notifications/custom/event");
            }
            _ => panic!("Expected Notification::Unknown"),
        }
    }

    #[test]
    fn jsonrpc_request_serde_roundtrip() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "test-id".to_string(),
            method: "tools/list".to_string(),
            params: None,
        };
        let json_str = serde_json::to_string(&req).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(val["jsonrpc"], "2.0");
        assert_eq!(val["id"], "test-id");
        assert_eq!(val["method"], "tools/list");
        // params is skipped when None
        assert!(val.get("params").is_none());
    }

    #[test]
    fn jsonrpc_response_error_deserialization() {
        let json_str = r#"{"jsonrpc":"2.0","id":"1","result":null,"error":{"code":-32601,"message":"Method not found","data":null}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json_str).unwrap();
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");
    }

    #[test]
    fn mcp_tool_input_schema_rename() {
        let json_str = r#"{"name":"my_tool","description":"A tool","inputSchema":{"type":"object","properties":{"x":{}}}}"#;
        let tool: McpTool = serde_json::from_str(json_str).unwrap();
        assert_eq!(tool.name, "my_tool");
        assert_eq!(tool.input_schema["type"], "object");
        assert!(tool.input_schema["properties"].get("x").is_some());
    }

    #[test]
    fn server_capabilities_default_all_none() {
        let caps = ServerCapabilities::default();
        assert!(caps.tools.is_none());
        assert!(caps.resources.is_none());
        assert!(caps.prompts.is_none());
        assert!(caps.logging.is_none());
    }
}
