use tokio::sync::mpsc;

pub mod http;
pub mod stdio;

/// Result of establishing a transport: channels for sending/receiving raw lines
pub struct TransportChannels {
    pub tx: mpsc::Sender<String>,
    pub rx: mpsc::Receiver<String>,
}
