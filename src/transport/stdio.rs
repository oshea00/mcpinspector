use anyhow::{Context, Result};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};
use tokio_util::codec::{FramedRead, LinesCodec};
use futures::StreamExt;

use crate::transport::TransportChannels;

pub struct StdioTransport {
    pub child: Child,
    pub stderr_buf: Arc<Mutex<String>>,
}

impl StdioTransport {
    pub fn spawn(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        debug: bool,
    ) -> Result<(Self, TransportChannels)> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn()
            .with_context(|| format!("Failed to spawn '{command}'"))?;

        let stdin = child.stdin.take().expect("stdin should be piped");
        let stdout = child.stdout.take().expect("stdout should be piped");
        let stderr = child.stderr.take().expect("stderr should be piped");

        // Collect stderr into a shared buffer so callers can inspect it on failure.
        // In debug mode, also stream each line to our own stderr in real time.
        let stderr_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let stderr_buf_writer = stderr_buf.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            let mut buf = String::new();
            while let Ok(Some(line)) = lines.next_line().await {
                if debug {
                    eprintln!("[server stderr] {line}");
                }
                buf.push_str(&line);
                buf.push('\n');
            }
            *stderr_buf_writer.lock().await = buf;
        });

        // Outgoing: our code sends strings, writer task appends \n and writes to stdin
        let (out_tx, mut out_rx) = mpsc::channel::<String>(64);

        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(mut msg) = out_rx.recv().await {
                msg.push('\n');
                if stdin.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });

        // Incoming: reader task reads stdout line by line, sends to channel
        let (in_tx, in_rx) = mpsc::channel::<String>(256);

        tokio::spawn(async move {
            let mut reader = FramedRead::new(stdout, LinesCodec::new_with_max_length(1024 * 1024));
            while let Some(Ok(line)) = reader.next().await {
                if in_tx.send(line).await.is_err() {
                    break;
                }
            }
        });

        Ok((
            StdioTransport { child, stderr_buf },
            TransportChannels { tx: out_tx, rx: in_rx },
        ))
    }

    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }
}
