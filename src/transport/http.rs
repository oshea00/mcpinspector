use std::collections::HashMap;

use anyhow::Result;
use futures::StreamExt;
use reqwest::Client;
use tokio::sync::mpsc;

use crate::transport::TransportChannels;

pub struct HttpTransport;

impl HttpTransport {
    pub fn connect(url: String, headers: HashMap<String, String>) -> Result<TransportChannels> {
        let client = Client::new();

        // Outgoing: POST each JSON-RPC message to the endpoint
        let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
        let (in_tx, in_rx) = mpsc::channel::<String>(256);

        let in_tx_clone = in_tx.clone();
        let client_clone = client.clone();
        let post_url = url.clone();

        // Writer task: send messages via POST, collect responses
        tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                let body: serde_json::Value = match serde_json::from_str(&msg) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let mut req = client_clone.post(&post_url).json(&body);
                for (key, value) in &headers {
                    req = req.header(key.as_str(), value.as_str());
                }
                match req.send().await {
                    Ok(resp) => {
                        let content_type = resp
                            .headers()
                            .get("content-type")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                            .to_string();

                        if content_type.contains("text/event-stream") {
                            // SSE stream
                            let mut stream = resp.bytes_stream();
                            let mut buffer = String::new();

                            while let Some(Ok(chunk)) = stream.next().await {
                                if let Ok(text) = std::str::from_utf8(&chunk) {
                                    buffer.push_str(text);
                                    // Parse SSE events
                                    while let Some(pos) = buffer.find("\n\n") {
                                        let event_text = buffer[..pos].to_string();
                                        buffer = buffer[pos + 2..].to_string();

                                        for line in event_text.lines() {
                                            if let Some(data) = line.strip_prefix("data: ") {
                                                let _ = in_tx_clone.send(data.to_string()).await;
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            // Regular JSON response
                            if let Ok(text) = resp.text().await {
                                let _ = in_tx_clone.send(text).await;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("HTTP error: {e}");
                    }
                }
            }
        });

        Ok(TransportChannels {
            tx: out_tx,
            rx: in_rx,
        })
    }
}
