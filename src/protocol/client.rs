use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

use crate::protocol::{
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, JsonRpcServerRequest, McpPrompt,
    McpPromptMessage, McpResource, McpResourceTemplate, McpTool, Notification, ServerCapabilities,
};

pub struct McpClient {
    pub transport_tx: mpsc::Sender<String>,
    pub pending: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>,
    #[allow(dead_code)]
    pub notification_tx: mpsc::Sender<Notification>,
    pub _reader_task: JoinHandle<()>,
    pub _writer_task: JoinHandle<()>,
    pub timeout_secs: u64,
    /// Map of server-request method → fixed JSON response payload.
    /// Shared with the reader task so it can be updated at runtime via cap-set/cap-remove.
    pub client_capabilities: Arc<Mutex<HashMap<String, Value>>>,
    #[allow(dead_code)]
    pub debug: bool,
}

impl McpClient {
    pub fn new(
        transport_tx: mpsc::Sender<String>,
        transport_rx: mpsc::Receiver<String>,
        notification_tx: mpsc::Sender<Notification>,
        timeout_secs: u64,
        client_capabilities: Arc<Mutex<HashMap<String, Value>>>,
        debug: bool,
    ) -> Self {
        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let pending_clone = pending.clone();
        let notif_tx_clone = notification_tx.clone();
        let caps_clone = client_capabilities.clone();

        // Writer task: read from our channel and forward to transport
        let (writer_tx, mut writer_rx) = mpsc::channel::<String>(64);
        let transport_tx_clone = transport_tx.clone();

        let _writer_task = tokio::spawn(async move {
            while let Some(msg) = writer_rx.recv().await {
                if debug {
                    eprintln!("[mcpi → server] {msg}");
                }
                if transport_tx_clone.send(msg).await.is_err() {
                    break;
                }
            }
        });

        // Clone writer_tx so the reader task can send responses back to the server.
        let response_tx = writer_tx.clone();

        // Reader task: parse incoming lines, route to pending responses, notifications,
        // or server-initiated requests.
        // When stdout closes (process exited), drain pending map so callers fail fast.
        let _reader_task = tokio::spawn(async move {
            let mut transport_rx = transport_rx;
            while let Some(line) = transport_rx.recv().await {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if debug {
                    eprintln!("[server → mcpi] {trimmed}");
                }

                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    let has_id = val.get("id").is_some();
                    let has_method = val.get("method").is_some();
                    let has_result = val.get("result").is_some();
                    let has_error = val.get("error").is_some();

                    if has_id && (has_result || has_error) {
                        // Client-bound response to one of our requests
                        if let Ok(resp) = serde_json::from_value::<JsonRpcResponse>(val) {
                            // Normalize id to string — servers may echo back a number or string
                            let id_key = match &resp.id {
                                serde_json::Value::String(s) => s.clone(),
                                v => v.to_string(),
                            };
                            let mut map = pending_clone.lock().await;
                            if let Some(tx) = map.remove(&id_key) {
                                let _ = tx.send(resp);
                            }
                        }
                    } else if has_method && !has_id {
                        // Server notification (no id)
                        if let Ok(notif) = serde_json::from_value::<JsonRpcNotification>(val) {
                            let n = Notification::from_jsonrpc(&notif);
                            let _ = notif_tx_clone.send(n).await;
                        }
                    } else if has_method && has_id {
                        // Server-initiated request — look up a configured handler
                        if let Ok(req) = serde_json::from_value::<JsonRpcServerRequest>(val) {
                            let handlers = caps_clone.lock().await;
                            let responded = if let Some(payload) = handlers.get(&req.method) {
                                let reply = json!({
                                    "jsonrpc": "2.0",
                                    "id": req.id,
                                    "result": payload
                                });
                                if let Ok(msg) = serde_json::to_string(&reply) {
                                    response_tx.send(msg).await.is_ok()
                                } else {
                                    false
                                }
                            } else {
                                // No handler — reply with method-not-found so the server
                                // doesn't time out silently.
                                let reply = json!({
                                    "jsonrpc": "2.0",
                                    "id": req.id,
                                    "error": {
                                        "code": -32601,
                                        "message": "Method not found"
                                    }
                                });
                                if let Ok(msg) = serde_json::to_string(&reply) {
                                    response_tx.send(msg).await.is_ok()
                                } else {
                                    false
                                }
                            };
                            drop(handlers);
                            let _ = notif_tx_clone
                                .send(Notification::ServerRequest {
                                    method: req.method,
                                    params: req.params,
                                    responded,
                                })
                                .await;
                        }
                    }
                }
            }
            // stdout closed — drop all pending senders so callers fail fast.
            let mut map = pending_clone.lock().await;
            map.clear();
        });

        McpClient {
            transport_tx: writer_tx,
            pending,
            notification_tx,
            _reader_task,
            _writer_task,
            timeout_secs,
            client_capabilities,
            debug,
        }
    }

    async fn send_request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = Uuid::new_v4().to_string();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method: method.to_string(),
            params,
        };
        let msg = serde_json::to_string(&req)?;

        let (tx, rx) = oneshot::channel::<JsonRpcResponse>();
        {
            let mut map = self.pending.lock().await;
            map.insert(id.clone(), tx);
        }

        self.transport_tx
            .send(msg)
            .await
            .map_err(|_| anyhow!("Transport send failed"))?;

        let resp = timeout(Duration::from_secs(self.timeout_secs), rx)
            .await
            .map_err(|_| anyhow!("Request timed out after {}s", self.timeout_secs))?
            .map_err(|_| anyhow!("Server process exited without responding"))?;

        if let Some(err) = resp.error {
            return Err(anyhow!("Server error {}: {}", err.code, err.message));
        }

        resp.result.ok_or_else(|| anyhow!("Empty result"))
    }

    pub async fn initialize(&self) -> Result<ServerCapabilities> {
        // Derive the advertised client capabilities from registered handlers.
        // e.g. "roots/list" → advertise { "roots": {} }
        let mut client_caps = serde_json::Map::new();
        {
            let handlers = self.client_capabilities.lock().await;
            for method in handlers.keys() {
                if let Some(ns) = method.split('/').next() {
                    client_caps.entry(ns.to_string()).or_insert(json!({}));
                }
            }
        }

        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": client_caps,
            "clientInfo": {
                "name": "mcpinspector",
                "version": "0.1.0"
            }
        });

        let result = self.send_request("initialize", Some(params)).await?;
        let caps: ServerCapabilities =
            serde_json::from_value(result.get("capabilities").cloned().unwrap_or_default())
                .unwrap_or_default();

        // Send initialized notification
        let notif = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "notifications/initialized".to_string(),
            params: None,
        };
        let msg = serde_json::to_string(&notif)?;
        let _ = self.transport_tx.send(msg).await;

        Ok(caps)
    }

    pub async fn list_tools(&self) -> Result<Vec<McpTool>> {
        let result = self.send_request("tools/list", None).await?;
        let tools = result.get("tools").cloned().unwrap_or(json!([]));
        Ok(serde_json::from_value(tools)?)
    }

    pub async fn call_tool(&self, name: &str, args: Option<Value>) -> Result<Value> {
        let params = json!({
            "name": name,
            "arguments": args.unwrap_or(json!({}))
        });
        self.send_request("tools/call", Some(params)).await
    }

    pub async fn list_resources(&self) -> Result<Vec<McpResource>> {
        let result = self.send_request("resources/list", None).await?;
        let resources = result.get("resources").cloned().unwrap_or(json!([]));
        Ok(serde_json::from_value(resources)?)
    }

    pub async fn list_resource_templates(&self) -> Result<Vec<McpResourceTemplate>> {
        let result = self.send_request("resources/templates/list", None).await?;
        let templates = result
            .get("resourceTemplates")
            .cloned()
            .unwrap_or(json!([]));
        Ok(serde_json::from_value(templates)?)
    }

    pub async fn read_resource(&self, uri: &str) -> Result<Value> {
        let params = json!({ "uri": uri });
        self.send_request("resources/read", Some(params)).await
    }

    pub async fn list_prompts(&self) -> Result<Vec<McpPrompt>> {
        let result = self.send_request("prompts/list", None).await?;
        let prompts = result.get("prompts").cloned().unwrap_or(json!([]));
        Ok(serde_json::from_value(prompts)?)
    }

    pub async fn get_prompt(
        &self,
        name: &str,
        args: Option<Value>,
    ) -> Result<Vec<McpPromptMessage>> {
        let mut params = json!({ "name": name });
        if let Some(a) = args {
            params["arguments"] = a;
        }
        let result = self.send_request("prompts/get", Some(params)).await?;
        let messages = result.get("messages").cloned().unwrap_or(json!([]));
        Ok(serde_json::from_value(messages)?)
    }
}
