use anyhow::{anyhow, Result};
use clap::Parser;

use mcpi::config::{
    apply_server_entry, load_mcp_servers, CompleterState, ReplState, TransportType,
    DEFAULT_TIMEOUT_SECS,
};
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

    /// Load MCP server definitions from a JSON config file (mcpServers format)
    #[arg(long, value_name = "FILE")]
    mcp_config: Option<String>,

    /// Select a server from --mcp-config by key and auto-connect
    #[arg(long, value_name = "KEY")]
    server: Option<String>,

    /// Call a tool and exit (batch/scripting mode — requires a connection)
    #[arg(long, value_name = "TOOL")]
    tool: Option<String>,

    /// JSON arguments for --tool
    #[arg(long, value_name = "JSON")]
    args: Option<String>,

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

    // Load mcp.json and apply server entry if specified
    if let Some(config_path) = &cli.mcp_config {
        if let Some(key) = &cli.server {
            let servers = load_mcp_servers(config_path)?;
            let entry = servers
                .get(key)
                .ok_or_else(|| anyhow!("Server '{}' not found in '{}'", key, config_path))?;
            apply_server_entry(&mut state, key, &entry.clone())?;
        } else if cli.tool.is_some() {
            return Err(anyhow!(
                "--server is required when using --tool with --mcp-config"
            ));
        }
    } else if cli.server.is_some() {
        return Err(anyhow!("--server requires --mcp-config"));
    }

    // Auto-connect: explicit flags take priority, then preloaded config from --server
    if let Some(cmd_str) = &cli.connect {
        let parts = shell_words::split(cmd_str)
            .unwrap_or_else(|_| cmd_str.split_whitespace().map(String::from).collect());
        if !parts.is_empty() {
            let line = format!("connect {cmd_str}");
            if let Err(e) = commands::handle_command(&mut state, &line).await {
                display::print_error(&e.to_string());
                if cli.tool.is_some() {
                    return Err(e);
                }
            }
        }
    } else if let Some(url) = &cli.connect_http {
        let line = format!("connect-http {url}");
        if let Err(e) = commands::handle_command(&mut state, &line).await {
            display::print_error(&e.to_string());
            if cli.tool.is_some() {
                return Err(e);
            }
        }
    } else if cli.server.is_some() {
        // Auto-connect using the preloaded server config
        match state.config.transport_type {
            TransportType::Http => {
                let line = format!("connect-http {}", state.config.url);
                commands::handle_command(&mut state, &line).await?;
            }
            TransportType::Stdio => {
                let quoted = shell_words::join(
                    std::iter::once(state.config.command.as_str())
                        .chain(state.config.args.iter().map(|s| s.as_str())),
                );
                let line = format!("connect {quoted}");
                commands::handle_command(&mut state, &line).await?;
            }
        }
    }

    // Batch tool call: invoke tool and exit without entering the REPL
    if let Some(tool_name) = cli.tool {
        let json_args = cli
            .args
            .as_deref()
            .map(serde_json::from_str::<serde_json::Value>)
            .transpose()
            .map_err(|e| anyhow!("Invalid JSON in --args: {e}"))?;

        let client = state.client.as_ref().ok_or_else(|| {
            anyhow!("Not connected — use --connect, --connect-http, or --mcp-config with --server")
        })?;

        let result = client.call_tool(&tool_name, json_args).await?;
        display::print_tool_result(&result);
        return Ok(());
    }

    repl::run_repl(&mut state, cli.live).await?;

    Ok(())
}
