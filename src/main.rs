use anyhow::Result;
use clap::Parser;

use mcpi::config::{CompleterState, ReplState, DEFAULT_TIMEOUT_SECS};
use mcpi::{commands, display, repl};

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

    /// Environment variable to pass to the MCP server (KEY=VALUE, repeatable)
    #[arg(short = 'e', value_name = "KEY=VALUE", action = clap::ArgAction::Append)]
    env: Vec<String>,

    /// HTTP header to send with requests (\"Name: Value\", repeatable)
    #[arg(short = 'H', value_name = "Name: Value", action = clap::ArgAction::Append)]
    header: Vec<String>,

    /// Bearer token for HTTP Authorization header (use $VARNAME to pass from env)
    #[arg(long, value_name = "TOKEN")]
    bearer: Option<String>,

    /// Print raw protocol messages to stderr for debugging
    #[arg(long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let completer_state = CompleterState::new();
    let mut state = ReplState::new(completer_state);
    state.timeout_secs = cli.timeout;
    state.debug = cli.debug;

    // Populate env vars from -e KEY=VALUE flags
    for kv in &cli.env {
        if let Some((k, v)) = kv.split_once('=') {
            state.config.env.insert(k.to_string(), v.to_string());
        } else {
            eprintln!("Warning: ignoring malformed -e value (expected KEY=VALUE): {kv}");
        }
    }

    // Populate HTTP headers from -H "Name: Value" flags
    for h in &cli.header {
        if let Some((k, v)) = h.split_once(':') {
            state
                .config
                .headers
                .insert(k.trim().to_string(), v.trim().to_string());
        } else {
            eprintln!("Warning: ignoring malformed -H value (expected \"Name: Value\"): {h}");
        }
    }

    // --bearer is shorthand for -H "Authorization: Bearer <token>"
    if let Some(token) = cli.bearer {
        state
            .config
            .headers
            .insert("Authorization".to_string(), format!("Bearer {token}"));
    }

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
