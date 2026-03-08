mod protocol;
mod transport;
mod config;
mod display;
mod commands;
mod repl;

use anyhow::Result;
use clap::Parser;

use config::{CompleterState, ReplState, DEFAULT_TIMEOUT_SECS};

#[derive(Parser, Debug)]
#[command(
    name = "mcpi",
    about = "MCP Inspector — interactive CLI for MCP server inspection",
    long_about = None,
    version
)]
struct Cli {
    /// Connect to a stdio MCP server on startup (e.g. "npx -y @mcp/server-filesystem /tmp")
    #[arg(long, value_name = "CMD")]
    connect: Option<String>,

    /// Connect to an HTTP MCP server on startup
    #[arg(long, value_name = "URL")]
    connect_http: Option<String>,

    /// Print server notifications live (instead of buffering)
    #[arg(long)]
    live: bool,

    /// Request timeout in seconds (default: 10)
    #[arg(long, value_name = "SECS", default_value_t = DEFAULT_TIMEOUT_SECS)]
    timeout: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let completer_state = CompleterState::new();
    let mut state = ReplState::new(completer_state);
    state.timeout_secs = cli.timeout;

    // Auto-connect if flags provided
    if let Some(cmd_str) = &cli.connect {
        let parts = shell_words::split(cmd_str)
            .unwrap_or_else(|_| cmd_str.split_whitespace().map(String::from).collect());
        if !parts.is_empty() {
            let line = format!("connect {cmd_str}");
            if let Err(e) = commands::handle_command(&mut state, &line).await {
                display::print_error(&e.to_string());
            }
        }
    } else if let Some(url) = &cli.connect_http {
        let line = format!("connect-http {url}");
        if let Err(e) = commands::handle_command(&mut state, &line).await {
            display::print_error(&e.to_string());
        }
    }

    repl::run_repl(&mut state, cli.live).await?;

    Ok(())
}
