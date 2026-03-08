use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use colored::Colorize;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Config, Context, Editor, Helper};
use tokio::sync::{mpsc, oneshot};

use crate::commands::handle_command;
use crate::config::{CompleterState, ReplState};
use crate::protocol::Notification;

// Commands that take tool names as second arg
const TOOL_COMMANDS: &[&str] = &["call"];
const RESOURCE_COMMANDS: &[&str] = &["read"];
const PROMPT_COMMANDS: &[&str] = &["prompt"];

const ALL_COMMANDS: &[&str] = &[
    "connect", "connect-http", "disconnect", "reconnect", "status",
    "tools", "call", "resources", "read", "prompts", "prompt",
    "export", "set-name", "set-env", "set-timeout",
    "cap-set", "cap-list", "cap-remove",
    "log", "help", "history", "clear",
    "quit", "exit",
];

pub struct McpHelper {
    pub completer_state: Arc<CompleterState>,
}

impl Helper for McpHelper {}
impl Highlighter for McpHelper {}
impl Hinter for McpHelper {
    type Hint = String;
}
impl Validator for McpHelper {}

impl Completer for McpHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let slice = &line[..pos];
        let words: Vec<&str> = slice.split_whitespace().collect();

        // If we're completing the first word (command)
        if words.is_empty() || (words.len() == 1 && !slice.ends_with(' ')) {
            let prefix = words.first().copied().unwrap_or("");
            let candidates: Vec<Pair> = ALL_COMMANDS.iter()
                .filter(|c| c.starts_with(prefix))
                .map(|c| Pair {
                    display: c.to_string(),
                    replacement: c.to_string(),
                })
                .collect();
            let start = pos - prefix.len();
            return Ok((start, candidates));
        }

        // Second word completions based on command
        if words.len() >= 1 && (words.len() == 1 || (words.len() == 2 && !slice.ends_with(' '))) {
            let cmd = words[0];
            let prefix = if words.len() == 2 { words[1] } else { "" };

            if TOOL_COMMANDS.contains(&cmd) {
                // Complete tool names — use blocking read of mutex
                let tools = match self.completer_state.tools.try_lock() {
                    Ok(g) => g.clone(),
                    Err(_) => vec![],
                };
                let candidates: Vec<Pair> = tools.iter()
                    .filter(|t| t.starts_with(prefix))
                    .map(|t| Pair { display: t.clone(), replacement: t.clone() })
                    .collect();
                let start = pos - prefix.len();
                return Ok((start, candidates));
            }

            if RESOURCE_COMMANDS.contains(&cmd) {
                let resources = match self.completer_state.resources.try_lock() {
                    Ok(g) => g.clone(),
                    Err(_) => vec![],
                };
                let candidates: Vec<Pair> = resources.iter()
                    .filter(|r| r.starts_with(prefix))
                    .map(|r| Pair { display: r.clone(), replacement: r.clone() })
                    .collect();
                let start = pos - prefix.len();
                return Ok((start, candidates));
            }

            if PROMPT_COMMANDS.contains(&cmd) {
                let prompts = match self.completer_state.prompts.try_lock() {
                    Ok(g) => g.clone(),
                    Err(_) => vec![],
                };
                let candidates: Vec<Pair> = prompts.iter()
                    .filter(|p| p.starts_with(prefix))
                    .map(|p| Pair { display: p.clone(), replacement: p.clone() })
                    .collect();
                let start = pos - prefix.len();
                return Ok((start, candidates));
            }
        }

        Ok((pos, vec![]))
    }
}

fn history_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("mcpi").join("history.txt"))
}

pub async fn run_repl(state: &mut ReplState, live_notifications: bool) -> Result<()> {
    let history_file = history_path();

    // Create history directory if needed
    if let Some(ref p) = history_file {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    println!("{}", "MCP Inspector (mcpi)".bold().cyan());
    println!("Type {} for available commands, {} to exit.", "'help'".yellow(), "'quit'".yellow());
    println!();

    // Channel from rustyline thread → async event loop
    // done_tx carries bool: true = continue (show next prompt), false = exit
    let (input_tx, mut input_rx) = mpsc::channel::<Option<(String, oneshot::Sender<bool>)>>(1);
    let completer_state = state.completer_state.clone();

    // Spawn rustyline in a blocking thread
    tokio::task::spawn_blocking(move || {
        let config = Config::builder()
            .completion_type(CompletionType::List)
            .build();

        let helper = McpHelper { completer_state };
        let mut rl = Editor::with_config(config).expect("Failed to create editor");
        rl.set_helper(Some(helper));

        if let Some(ref p) = history_file {
            let _ = rl.load_history(p);
        }

        loop {
            match rl.readline("mcpi> ") {
                Ok(line) => {
                    let trimmed = line.trim().to_string();
                    if !trimmed.is_empty() {
                        rl.add_history_entry(&trimmed).ok();
                    }
                    let (done_tx, done_rx) = oneshot::channel::<bool>();
                    if input_tx.blocking_send(Some((trimmed, done_tx))).is_err() {
                        break;
                    }
                    // Wait for command to finish; false means exit requested
                    match done_rx.blocking_recv() {
                        Ok(true) => {}    // continue — show next prompt
                        _ => break,       // false or channel closed — exit
                    }
                }
                Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                    let _ = input_tx.blocking_send(None);
                    break;
                }
                Err(_) => {
                    let _ = input_tx.blocking_send(None);
                    break;
                }
            }
        }

        // Save history
        if let Some(ref p) = history_file {
            let _ = rl.save_history(p);
        }
    });

    // Async event loop
    loop {
        // Build the notification future
        let notif_future = async {
            match &mut state.notification_rx {
                Some(rx) => rx.recv().await,
                None => std::future::pending().await,
            }
        };

        tokio::select! {
            line_opt = input_rx.recv() => {
                match line_opt {
                    Some(Some((line, done_tx))) if !line.is_empty() => {
                        state.history.push(line.clone());
                        match handle_command(state, &line).await {
                            Ok(true) => { let _ = done_tx.send(false); break; }  // quit
                            Ok(false) => { let _ = done_tx.send(true); }
                            Err(e) => {
                                crate::display::print_error(&e.to_string());
                                let _ = done_tx.send(true);
                            }
                        }
                    }
                    Some(Some((_, done_tx))) => { let _ = done_tx.send(true); } // empty line
                    Some(None) | None => break, // EOF or channel closed
                }
            }
            Some(notif) = notif_future => {
                if live_notifications {
                    // Print inline
                    print!("\r\x1B[K");
                    match &notif {
                        Notification::Log { level, message } => {
                            println!("[notification] {level}: {message}");
                        }
                        Notification::ToolListChanged => {
                            println!("[notification] tools list changed");
                        }
                        Notification::ResourceListChanged => {
                            println!("[notification] resources list changed");
                        }
                        Notification::PromptListChanged => {
                            println!("[notification] prompts list changed");
                        }
                        Notification::ServerRequest { method, responded, .. } => {
                            if *responded {
                                println!("[server→client] {method} (responded)");
                            } else {
                                println!("[server→client] {method} (no handler — replied method-not-found)");
                            }
                        }
                        Notification::Unknown { method, .. } => {
                            println!("[notification] {method}");
                        }
                    }
                    print!("mcpi> ");
                } else {
                    state.pending_notifications.push(notif);
                    let count = state.pending_notifications.len();
                    print!("\r\x1B[K[{count} new notification(s) — type 'log' to view]\nmcpi> ");
                }
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
        }
    }

    Ok(())
}
