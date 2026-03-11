use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use mcpi::protocol::{client::McpClient, Notification};

/// Simulates the server side of the transport: reads what the client sends,
/// and can send responses/notifications back to the client.
pub struct MockMcpServer {
    /// Messages the client sent to the "server"
    pub incoming: mpsc::Receiver<String>,
    /// Send responses or notifications to the client
    pub outgoing: mpsc::Sender<String>,
}

/// Create a McpClient wired to a MockMcpServer via in-process channels.
/// Returns (client, mock_server, notification_receiver).
pub fn make_client_with_mock() -> (McpClient, MockMcpServer, mpsc::Receiver<Notification>) {
    make_client_with_mock_timeout(5)
}

/// Same as `make_client_with_mock` but with a configurable timeout.
pub fn make_client_with_mock_timeout(
    timeout_secs: u64,
) -> (McpClient, MockMcpServer, mpsc::Receiver<Notification>) {
    // client_sends_tx → client writes outgoing requests here
    // client_sends_rx → mock server reads requests from here
    let (client_sends_tx, client_sends_rx) = mpsc::channel::<String>(64);

    // server_sends_tx → mock server writes responses here
    // server_sends_rx → client reads responses from here
    let (server_sends_tx, server_sends_rx) = mpsc::channel::<String>(64);

    let (notif_tx, notif_rx) = mpsc::channel::<Notification>(64);
    let caps = Arc::new(Mutex::new(HashMap::new()));

    let client = McpClient::new(
        client_sends_tx,
        server_sends_rx,
        notif_tx,
        timeout_secs,
        caps,
        false,
    );

    let server = MockMcpServer {
        incoming: client_sends_rx,
        outgoing: server_sends_tx,
    };

    (client, server, notif_rx)
}
