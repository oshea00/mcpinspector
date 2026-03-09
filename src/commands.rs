use anyhow::{anyhow, Result};
use colored::Colorize;
use comfy_table::{Table, Cell, Color, Attribute};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::config::{ReplState, TransportType, write_config};
use crate::display;
use crate::protocol::{Notification, McpTool, McpResource, McpPrompt};
use crate::protocol::client::McpClient;
use crate::transport::{stdio::StdioTransport, http::HttpTransport};

pub async fn handle_command(state: &mut ReplState, line: &str) -> Result<bool> {
    let parts = shell_words::split(line.trim())
        .unwrap_or_else(|_| line.split_whitespace().map(String::from).collect());

    if parts.is_empty() {
        return Ok(false);
    }

    let cmd = parts[0].as_str();
    let args = &parts[1..];

    match cmd {
        "connect" => cmd_connect(state, args).await?,
        "connect-http" => cmd_connect_http(state, args).await?,
        "disconnect" => cmd_disconnect(state).await?,
        "reconnect" => cmd_reconnect(state).await?,
        "status" => cmd_status(state).await?,
        "tools" => cmd_tools(state).await?,
        "call" => cmd_call(state, args, line).await?,
        "resources" => cmd_resources(state).await?,
        "read" => cmd_read(state, args).await?,
        "prompts" => cmd_prompts(state).await?,
        "prompt" => cmd_prompt(state, args, line).await?,
        "export" => cmd_export(state, args)?,
        "set-name" => cmd_set_name(state, args)?,
        "set-env" => cmd_set_env(state, args)?,
        "set-timeout" => cmd_set_timeout(state, args)?,
        "cap-set" => cmd_cap_set(state, args, line).await?,
        "cap-list" => cmd_cap_list(state).await,
        "cap-remove" => cmd_cap_remove(state, args).await?,
        "log" => cmd_log(state),
        "help" => cmd_help(args),
        "history" => cmd_history(state),
        "clear" => cmd_clear(),
        "quit" | "exit" => return Ok(true),
        _ => {
            display::print_error(&format!("Unknown command: '{cmd}'. Type 'help' for available commands."));
        }
    }

    Ok(false)
}

async fn cmd_connect(state: &mut ReplState, args: &[String]) -> Result<()> {
    if args.is_empty() {
        display::print_error("Usage: connect <command> [args...]");
        return Ok(());
    }

    if state.is_connected() {
        cmd_disconnect(state).await?;
    }

    let command = args[0].clone();
    let cmd_args: Vec<String> = args[1..].to_vec();
    let env = state.config.env.clone();

    display::print_info(&format!("Connecting to '{command}'..."));

    let (mut transport, channels) = StdioTransport::spawn(&command, &cmd_args, &env, state.debug)?;

    let (notif_tx, notif_rx) = mpsc::channel::<Notification>(256);
    let client = McpClient::new(channels.tx, channels.rx, notif_tx, state.timeout_secs, state.client_capabilities.clone(), state.debug);

    match client.initialize().await {
        Ok(caps) => {
            // Update completer state
            {
                let tools: Vec<McpTool> = client.list_tools().await.unwrap_or_default();
                let mut t = state.completer_state.tools.lock().await;
                *t = tools.iter().map(|t| t.name.clone()).collect();
            }
            {
                let resources: Vec<McpResource> = client.list_resources().await.unwrap_or_default();
                let mut r = state.completer_state.resources.lock().await;
                *r = resources.iter().map(|r| r.uri.clone()).collect();
            }
            {
                let prompts: Vec<McpPrompt> = client.list_prompts().await.unwrap_or_default();
                let mut p = state.completer_state.prompts.lock().await;
                *p = prompts.iter().map(|p| p.name.clone()).collect();
            }

            state.config.transport_type = TransportType::Stdio;
            state.config.command = command;
            state.config.args = cmd_args;
            state.client = Some(client);
            state.stdio_transport = Some(transport);
            state.notification_rx = Some(notif_rx);

            display::print_success("Connected!");
            display::print_capabilities(&caps);
            state.capabilities = Some(caps);
        }
        Err(e) => {
            transport.kill().await;
            // Give the stderr collector task a moment to flush
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let stderr = transport.stderr_buf.lock().await;
            let stderr_msg = stderr.trim();
            if !stderr_msg.is_empty() {
                return Err(anyhow!("{e}\n\nServer stderr:\n{stderr_msg}"));
            }
            return Err(anyhow!("{e}"));
        }
    }

    Ok(())
}

async fn cmd_connect_http(state: &mut ReplState, args: &[String]) -> Result<()> {
    if args.is_empty() {
        display::print_error("Usage: connect-http <url>");
        return Ok(());
    }

    if state.is_connected() {
        cmd_disconnect(state).await?;
    }

    let url = args[0].clone();
    display::print_info(&format!("Connecting to '{url}'..."));

    let channels = HttpTransport::connect(url.clone(), state.config.bearer_token.clone())?;
    let (notif_tx, notif_rx) = mpsc::channel::<Notification>(256);
    let client = McpClient::new(channels.tx, channels.rx, notif_tx, state.timeout_secs, state.client_capabilities.clone(), state.debug);

    match client.initialize().await {
        Ok(caps) => {
            state.config.transport_type = TransportType::Http;
            state.config.url = url;
            state.client = Some(client);
            state.notification_rx = Some(notif_rx);

            display::print_success("Connected!");
            display::print_capabilities(&caps);
            state.capabilities = Some(caps);
        }
        Err(e) => {
            return Err(anyhow!("Initialization failed: {e}"));
        }
    }

    Ok(())
}

async fn cmd_disconnect(state: &mut ReplState) -> Result<()> {
    if !state.is_connected() {
        display::print_info("Not connected.");
        return Ok(());
    }
    state.client = None;
    state.notification_rx = None;
    state.capabilities = None;
    if let Some(mut t) = state.stdio_transport.take() {
        t.kill().await;
    }
    {
        let mut t = state.completer_state.tools.lock().await;
        t.clear();
    }
    {
        let mut r = state.completer_state.resources.lock().await;
        r.clear();
    }
    {
        let mut p = state.completer_state.prompts.lock().await;
        p.clear();
    }
    display::print_success("Disconnected.");
    Ok(())
}

async fn cmd_reconnect(state: &mut ReplState) -> Result<()> {
    let command = state.config.command.clone();
    let args = state.config.args.clone();
    let url = state.config.url.clone();
    let transport_type = state.config.transport_type.clone();

    match transport_type {
        TransportType::Stdio => {
            if command.is_empty() {
                display::print_error("No previous stdio connection to reconnect to.");
                return Ok(());
            }
            let all_args: Vec<String> = std::iter::once(command).chain(args).collect();
            cmd_connect(state, &all_args).await
        }
        TransportType::Http => {
            if url.is_empty() {
                display::print_error("No previous HTTP connection to reconnect to.");
                return Ok(());
            }
            cmd_connect_http(state, &[url]).await
        }
    }
}

async fn cmd_status(state: &mut ReplState) -> Result<()> {
    if !state.is_connected() {
        println!("Status: {}", "Disconnected".red());
        return Ok(());
    }
    println!("Status: {}", "Connected".green());
    println!("Server: {}", state.server_name.cyan());
    match state.config.transport_type {
        TransportType::Stdio => {
            println!("Transport: stdio");
            println!("Command: {} {}", state.config.command, state.config.args.join(" "));
        }
        TransportType::Http => {
            println!("Transport: http");
            println!("URL: {}", state.config.url);
        }
    }
    if !state.config.env.is_empty() {
        println!("Env vars: {}", state.config.env.keys().cloned().collect::<Vec<_>>().join(", "));
    }
    if let Some(caps) = &state.capabilities {
        println!("\nCapabilities:");
        display::print_capabilities(caps);
    }
    println!("Timeout: {}s", state.timeout_secs);
    let count = state.pending_notifications.len();
    if count > 0 {
        println!("{} pending notification(s). Type 'log' to view.", count.to_string().yellow());
    }
    Ok(())
}

async fn cmd_tools(state: &mut ReplState) -> Result<()> {
    let client = state.client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;
    let tools = client.list_tools().await?;

    // Update completer
    {
        let mut t = state.completer_state.tools.lock().await;
        *t = tools.iter().map(|t| t.name.clone()).collect();
    }

    display::print_tools(&tools);
    Ok(())
}

/// Extract raw JSON from the original input line, skipping `skip` whitespace-delimited words.
/// This avoids shell-word quote stripping mangling JSON strings.
fn raw_json_arg(line: &str, skip: usize) -> Option<&str> {
    let mut remaining = line.trim();
    for _ in 0..skip {
        remaining = remaining.trim_start();
        // skip one word (no quote handling needed here — just find next whitespace)
        let end = remaining.find(|c: char| c.is_whitespace()).unwrap_or(remaining.len());
        remaining = &remaining[end..];
    }
    let trimmed = remaining.trim();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

async fn cmd_call(state: &mut ReplState, args: &[String], raw_line: &str) -> Result<()> {
    if args.is_empty() {
        display::print_error("Usage: call <tool_name> [json_args]");
        return Ok(());
    }

    let client = state.client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;
    let name = &args[0];
    let json_args: Option<Value> = if args.len() > 1 {
        let json_str = raw_json_arg(raw_line, 2)
            .ok_or_else(|| anyhow!("Missing JSON argument"))?;
        Some(serde_json::from_str(json_str)
            .map_err(|e| anyhow!("Invalid JSON: {e}"))?)
    } else {
        None
    };

    display::print_info(&format!("Calling tool '{name}'..."));
    let result = client.call_tool(name, json_args).await?;
    display::print_tool_result(&result);
    Ok(())
}

async fn cmd_resources(state: &mut ReplState) -> Result<()> {
    let client = state.client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;
    let resources = client.list_resources().await?;

    {
        let mut r = state.completer_state.resources.lock().await;
        *r = resources.iter().map(|r| r.uri.clone()).collect();
    }

    display::print_resources(&resources);
    Ok(())
}

async fn cmd_read(state: &mut ReplState, args: &[String]) -> Result<()> {
    if args.is_empty() {
        display::print_error("Usage: read <uri>");
        return Ok(());
    }

    let client = state.client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;
    let uri = &args[0];
    let result = client.read_resource(uri).await?;
    display::print_resource_result(&result);
    Ok(())
}

async fn cmd_prompts(state: &mut ReplState) -> Result<()> {
    let client = state.client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;
    let prompts = client.list_prompts().await?;

    {
        let mut p = state.completer_state.prompts.lock().await;
        *p = prompts.iter().map(|p| p.name.clone()).collect();
    }

    display::print_prompts(&prompts);
    Ok(())
}

async fn cmd_prompt(state: &mut ReplState, args: &[String], raw_line: &str) -> Result<()> {
    if args.is_empty() {
        display::print_error("Usage: prompt <name> [json_args]");
        return Ok(());
    }

    let client = state.client.as_ref().ok_or_else(|| anyhow!("Not connected"))?;
    let name = &args[0];
    let json_args: Option<Value> = if args.len() > 1 {
        let json_str = raw_json_arg(raw_line, 2)
            .ok_or_else(|| anyhow!("Missing JSON argument"))?;
        Some(serde_json::from_str(json_str)
            .map_err(|e| anyhow!("Invalid JSON: {e}"))?)
    } else {
        None
    };

    let messages = client.get_prompt(name, json_args).await?;
    display::print_prompt_messages(&messages);
    Ok(())
}

fn cmd_export(state: &ReplState, args: &[String]) -> Result<()> {
    let filename = args.first().map(|s| s.as_str());
    write_config(state, filename)
}

fn cmd_set_name(state: &mut ReplState, args: &[String]) -> Result<()> {
    if args.is_empty() {
        display::print_error("Usage: set-name <name>");
        return Ok(());
    }
    state.server_name = args[0].clone();
    display::print_success(&format!("Server name set to '{}'", state.server_name));
    Ok(())
}

fn cmd_set_timeout(state: &mut ReplState, args: &[String]) -> Result<()> {
    if args.is_empty() {
        display::print_error("Usage: set-timeout <seconds>");
        return Ok(());
    }
    let secs: u64 = args[0].parse()
        .map_err(|_| anyhow!("Invalid timeout '{}': must be a positive integer", args[0]))?;
    if secs == 0 {
        return Err(anyhow!("Timeout must be at least 1 second"));
    }
    state.timeout_secs = secs;
    display::print_success(&format!("Request timeout set to {secs}s"));
    Ok(())
}

fn cmd_set_env(state: &mut ReplState, args: &[String]) -> Result<()> {
    if args.len() < 2 {
        display::print_error("Usage: set-env <key> <value>");
        return Ok(());
    }
    state.config.env.insert(args[0].clone(), args[1].clone());
    display::print_success(&format!("Set env {}={}", args[0], args[1]));
    Ok(())
}

async fn cmd_cap_set(state: &mut ReplState, args: &[String], raw_line: &str) -> Result<()> {
    if args.is_empty() {
        display::print_error("Usage: cap-set <method> <json_response>");
        display::print_error("  e.g. cap-set roots/list {\"roots\":[{\"uri\":\"file:///home/user/repos\",\"name\":\"repos\"}]}");
        return Ok(());
    }
    let method = args[0].clone();
    let json_str = raw_json_arg(raw_line, 2)
        .ok_or_else(|| anyhow!("Missing JSON response argument"))?;
    let payload: Value = serde_json::from_str(json_str)
        .map_err(|e| anyhow!("Invalid JSON: {e}"))?;

    {
        let mut caps = state.client_capabilities.lock().await;
        caps.insert(method.clone(), payload);
    }
    display::print_success(&format!("Capability handler set for '{method}'"));
    if state.is_connected() {
        display::print_info("Note: reconnect for capability to be advertised in initialize handshake.");
    }
    Ok(())
}

async fn cmd_cap_list(state: &ReplState) {
    let caps = state.client_capabilities.lock().await;
    if caps.is_empty() {
        println!("No client capability handlers configured.");
        println!("Use 'cap-set <method> <json>' to add one.");
        return;
    }
    let mut table = Table::new();
    table.set_header(vec![
        Cell::new("Method").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("Response Payload").add_attribute(Attribute::Bold).fg(Color::Cyan),
    ]);
    for (method, payload) in caps.iter() {
        let pretty = serde_json::to_string_pretty(payload).unwrap_or_default();
        table.add_row(vec![
            Cell::new(method).fg(Color::Green),
            Cell::new(&pretty),
        ]);
    }
    println!("{table}");
}

async fn cmd_cap_remove(state: &mut ReplState, args: &[String]) -> Result<()> {
    if args.is_empty() {
        display::print_error("Usage: cap-remove <method>");
        return Ok(());
    }
    let method = &args[0];
    let mut caps = state.client_capabilities.lock().await;
    if caps.remove(method).is_some() {
        display::print_success(&format!("Removed capability handler for '{method}'"));
    } else {
        display::print_info(&format!("No handler registered for '{method}'"));
    }
    Ok(())
}

fn cmd_log(state: &mut ReplState) {
    display::print_notifications(&state.pending_notifications);
    state.pending_notifications.clear();
}

fn cmd_history(state: &ReplState) {
    if state.history.is_empty() {
        println!("{}", "No history.".yellow());
        return;
    }
    for (i, entry) in state.history.iter().enumerate() {
        println!("{:>4}  {}", i + 1, entry);
    }
}

fn cmd_clear() {
    print!("\x1B[2J\x1B[H");
}

fn cmd_help(args: &[String]) {
    if let Some(cmd) = args.first() {
        print_command_help(cmd);
    } else {
        print_full_help();
    }
}

fn print_full_help() {
    println!("{}", "MCP Inspector Commands".bold().underline());
    println!();
    println!("{}", "Connection:".yellow().bold());
    println!("  {:30} {}", "connect <cmd> [args...]", "Connect via stdio transport");
    println!("  {:30} {}", "connect-http <url>", "Connect via HTTP/SSE transport");
    println!("  {:30} {}", "disconnect", "Disconnect from server");
    println!("  {:30} {}", "reconnect", "Reconnect using the current connection command");
    println!("  {:30} {}", "status", "Show connection status and capabilities");
    println!();
    println!("{}", "Tools:".yellow().bold());
    println!("  {:30} {}", "tools", "List all available tools");
    println!("  {:30} {}", "call <name> [json]", "Execute a tool with optional JSON args");
    println!();
    println!("{}", "Resources:".yellow().bold());
    println!("  {:30} {}", "resources", "List all resources");
    println!("  {:30} {}", "read <uri>", "Read resource content");
    println!();
    println!("{}", "Prompts:".yellow().bold());
    println!("  {:30} {}", "prompts", "List all prompts");
    println!("  {:30} {}", "prompt <name> [json]", "Get prompt with optional arguments");
    println!();
    println!("{}", "Configuration:".yellow().bold());
    println!("  {:30} {}", "set-name <name>", "Set server name for export");
    println!("  {:30} {}", "set-env <key> <val>", "Add environment variable");
    println!("  {:30} {}", "set-timeout <seconds>", "Set request timeout (default: 10s)");
    println!("  {:30} {}", "export [filename]", "Export Claude Desktop JSON config");
    println!();
    println!("{}", "Client Capabilities:".yellow().bold());
    println!("  {:30} {}", "cap-set <method> <json>", "Register a handler for a server request");
    println!("  {:30} {}", "cap-list", "Show all configured capability handlers");
    println!("  {:30} {}", "cap-remove <method>", "Remove a capability handler");
    println!();
    println!("{}", "Other:".yellow().bold());
    println!("  {:30} {}", "log", "Show buffered server notifications");
    println!("  {:30} {}", "history", "Show command history");
    println!("  {:30} {}", "clear", "Clear terminal");
    println!("  {:30} {}", "help [command]", "Show help");
    println!("  {:30} {}", "quit / exit", "Exit");
    println!();
    println!("Tip: Tab completion works for commands, tool names, resource URIs, and prompt names.");
}

fn print_command_help(cmd: &str) {
    match cmd {
        "connect" => {
            println!("{}", "connect <command> [args...]".bold());
            println!("  Connect to an MCP server via stdio transport.");
            println!("  The command is spawned as a subprocess with optional arguments.");
            println!("  Example: connect npx -y @modelcontextprotocol/server-filesystem /tmp");
        }
        "connect-http" => {
            println!("{}", "connect-http <url>".bold());
            println!("  Connect to an MCP server via HTTP/SSE transport.");
            println!("  Example: connect-http http://localhost:3000/mcp");
        }
        "call" => {
            println!("{}", "call <tool_name> [json_args]".bold());
            println!("  Execute a tool with optional JSON arguments.");
            println!("  Example: call read_file {{\"path\":\"/tmp/test.txt\"}}");
        }
        "export" => {
            println!("{}", "export [filename]".bold());
            println!("  Export Claude Desktop-compatible JSON configuration.");
            println!("  If no filename given, prints to stdout.");
            println!("  Example: export myserver.json");
        }
        "cap-set" => {
            println!("{}", "cap-set <method> <json_response>".bold());
            println!("  Register a fixed JSON response for a server-initiated request.");
            println!("  Must be set before connecting — capabilities are advertised in initialize.");
            println!("  The capability namespace is derived from the method (e.g. 'roots/list' → 'roots').");
            println!();
            println!("  Example (filesystem server roots):");
            println!("    cap-set roots/list {{\"roots\":[{{\"uri\":\"file:///home/user/repos\",\"name\":\"repos\"}}]}}");
        }
        "cap-list" => {
            println!("{}", "cap-list".bold());
            println!("  Show all configured client capability handlers.");
        }
        "cap-remove" => {
            println!("{}", "cap-remove <method>".bold());
            println!("  Remove a configured capability handler.");
            println!("  Example: cap-remove roots/list");
        }
        _ => {
            println!("No specific help for '{cmd}'. Type 'help' for all commands.");
        }
    }
}
