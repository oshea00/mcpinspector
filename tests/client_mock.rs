mod helpers;

use mcpi::protocol::{McpResourceTemplate, McpTool, Notification};
use serde_json::json;
use tokio::time::Duration;

// ── helpers ────────────────────────────────────────────────────────────────

/// Read one request from the mock server's incoming channel, parse the JSON id,
/// send `result` back with the same id, and return the full request value.
async fn respond_with(
    server: &mut helpers::MockMcpServer,
    result: serde_json::Value,
) -> serde_json::Value {
    let req_str = server.incoming.recv().await.expect("no request received");
    let req: serde_json::Value = serde_json::from_str(&req_str).unwrap();
    let id = req["id"].clone();
    let response = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    server
        .outgoing
        .send(serde_json::to_string(&response).unwrap())
        .await
        .unwrap();
    req
}

// ── tests ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn routes_response_to_pending_request() {
    let (client, mut server, _notif_rx) = helpers::make_client_with_mock();

    tokio::spawn(async move {
        respond_with(&mut server, json!({"tools": []})).await;
    });

    let result = client.list_tools().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);
}

#[tokio::test]
async fn routes_notification_to_channel() {
    let (client, server, mut notif_rx) = helpers::make_client_with_mock();
    let _client = client; // keep client alive so reader task runs

    let notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/tools/list_changed",
    });
    server
        .outgoing
        .send(serde_json::to_string(&notif).unwrap())
        .await
        .unwrap();

    let received = tokio::time::timeout(Duration::from_secs(1), notif_rx.recv())
        .await
        .expect("timed out waiting for notification")
        .expect("notification channel closed");

    assert!(matches!(received, Notification::ToolListChanged));
}

#[tokio::test]
async fn handles_server_request_with_handler() {
    let (client, mut server, mut notif_rx) = helpers::make_client_with_mock();

    // Pre-configure a capability handler
    {
        let mut caps = client.client_capabilities.lock().await;
        caps.insert("roots/list".to_string(), json!({"roots": []}));
    }

    let _client = client;

    // Server sends a server-initiated request (has both method and id)
    let server_req = json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "roots/list",
        "params": {},
    });
    server
        .outgoing
        .send(serde_json::to_string(&server_req).unwrap())
        .await
        .unwrap();

    // Server reads the response the client sent back
    let response_str = tokio::time::timeout(Duration::from_secs(1), server.incoming.recv())
        .await
        .expect("timed out waiting for response")
        .expect("server incoming channel closed");
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["id"], 42);
    assert!(response.get("result").is_some());
    assert_eq!(response["result"]["roots"], json!([]));

    // The notification channel should have a ServerRequest notification
    let notif = tokio::time::timeout(Duration::from_secs(1), notif_rx.recv())
        .await
        .expect("timed out waiting for ServerRequest notification")
        .expect("notification channel closed");
    assert!(matches!(
        notif,
        Notification::ServerRequest {
            responded: true,
            ..
        }
    ));
}

#[tokio::test]
async fn handles_server_request_no_handler() {
    let (client, mut server, _notif_rx) = helpers::make_client_with_mock();
    let _client = client;

    // No handler registered — client should reply with method-not-found
    let server_req = json!({
        "jsonrpc": "2.0",
        "id": "req-1",
        "method": "sampling/createMessage",
        "params": {},
    });
    server
        .outgoing
        .send(serde_json::to_string(&server_req).unwrap())
        .await
        .unwrap();

    let response_str = tokio::time::timeout(Duration::from_secs(1), server.incoming.recv())
        .await
        .expect("timed out waiting for error response")
        .expect("server incoming channel closed");
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["id"], "req-1");
    assert!(response.get("error").is_some());
    assert_eq!(response["error"]["code"], -32601);
}

#[tokio::test(start_paused = true)]
async fn timeout_on_no_response() {
    // 5 second timeout; time is paused so we advance manually
    let (client, _server, _notif_rx) = helpers::make_client_with_mock();

    let task = tokio::spawn(async move { client.list_tools().await });

    // Advance time past the 5-second timeout
    tokio::time::advance(Duration::from_secs(6)).await;

    let result = task.await.unwrap();
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("timed out") || msg.contains("timeout") || msg.contains("Timed out"),
        "unexpected error: {msg}"
    );
}

#[tokio::test]
async fn fails_fast_on_channel_close() {
    let (client, server, _notif_rx) = helpers::make_client_with_mock();

    // Spawn the request first
    let task = tokio::spawn(async move { client.list_tools().await });

    // Give the spawned task a moment to register the pending request
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Drop server — closes both transport channels; reader task will drain pending map
    drop(server);

    let result = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("test timed out")
        .expect("task panicked");

    assert!(result.is_err());
}

#[tokio::test]
async fn list_tools_parses_response() {
    let (client, mut server, _notif_rx) = helpers::make_client_with_mock();

    tokio::spawn(async move {
        respond_with(
            &mut server,
            json!({
                "tools": [{
                    "name": "echo",
                    "description": "Echo a message back",
                    "inputSchema": {"type": "object", "properties": {"text": {}}}
                }]
            }),
        )
        .await;
    });

    let tools: Vec<McpTool> = client.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");
    assert_eq!(tools[0].description, "Echo a message back");
}

#[tokio::test]
async fn call_tool_sends_correct_params() {
    let (client, mut server, _notif_rx) = helpers::make_client_with_mock();

    let capture = tokio::spawn(async move {
        respond_with(
            &mut server,
            json!({"content": [{"type": "text", "text": "echo: hello"}]}),
        )
        .await
    });

    client
        .call_tool("echo", Some(json!({"x": 1})))
        .await
        .unwrap();

    let request = capture.await.unwrap();
    assert_eq!(request["method"], "tools/call");
    assert_eq!(request["params"]["name"], "echo");
    assert_eq!(request["params"]["arguments"]["x"], 1);
}

#[tokio::test]
async fn list_resources_parses_response() {
    let (client, mut server, _notif_rx) = helpers::make_client_with_mock();

    tokio::spawn(async move {
        respond_with(
            &mut server,
            json!({
                "resources": [{
                    "uri": "file:///tmp/test.txt",
                    "name": "test",
                    "mimeType": "text/plain",
                    "description": ""
                }]
            }),
        )
        .await;
    });

    let resources = client.list_resources().await.unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].uri, "file:///tmp/test.txt");
    assert_eq!(resources[0].mime_type, "text/plain");
}

#[tokio::test]
async fn list_resource_templates_parses_response() {
    let (client, mut server, _notif_rx) = helpers::make_client_with_mock();

    tokio::spawn(async move {
        respond_with(
            &mut server,
            json!({
                "resourceTemplates": [{
                    "uriTemplate": "weather://{location}",
                    "name": "Weather",
                    "mimeType": "text/plain",
                    "description": "Weather data for a location"
                }]
            }),
        )
        .await;
    });

    let templates: Vec<McpResourceTemplate> = client.list_resource_templates().await.unwrap();
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].uri_template, "weather://{location}");
    assert_eq!(templates[0].name, "Weather");
    assert_eq!(templates[0].mime_type, "text/plain");
}

#[tokio::test]
async fn list_resource_templates_empty_response() {
    let (client, mut server, _notif_rx) = helpers::make_client_with_mock();

    tokio::spawn(async move {
        respond_with(&mut server, json!({"resourceTemplates": []})).await;
    });

    let templates = client.list_resource_templates().await.unwrap();
    assert!(templates.is_empty());
}

#[tokio::test]
async fn list_resource_templates_sends_correct_method() {
    let (client, mut server, _notif_rx) = helpers::make_client_with_mock();

    let capture =
        tokio::spawn(
            async move { respond_with(&mut server, json!({"resourceTemplates": []})).await },
        );

    client.list_resource_templates().await.unwrap();

    let request = capture.await.unwrap();
    assert_eq!(request["method"], "resources/templates/list");
}

#[tokio::test]
async fn initialize_sends_initialized_notification() {
    let (client, mut server, _notif_rx) = helpers::make_client_with_mock();

    let server_task = tokio::spawn(async move {
        // 1. Read the initialize request and respond
        let req = respond_with(
            &mut server,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "serverInfo": {"name": "test-server", "version": "1.0"},
            }),
        )
        .await;
        assert_eq!(req["method"], "initialize");

        // 2. Read the notifications/initialized notification the client sends after
        let notif_str = server.incoming.recv().await.expect("no initialized notif");
        let notif: serde_json::Value = serde_json::from_str(&notif_str).unwrap();
        notif
    });

    client.initialize().await.unwrap();

    let notif = server_task.await.unwrap();
    assert_eq!(notif["method"], "notifications/initialized");
    assert!(
        notif.get("id").is_none(),
        "notifications should not have an id"
    );
}
