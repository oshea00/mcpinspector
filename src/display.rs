use colored::Colorize;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
use serde_json::Value;

use crate::protocol::{
    McpPrompt, McpResource, McpResourceTemplate, McpTool, Notification, ServerCapabilities,
};

pub fn print_tools(tools: &[McpTool]) {
    if tools.is_empty() {
        println!("{}", "No tools available.".yellow());
        return;
    }
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Name")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
        Cell::new("Description")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
        Cell::new("Input Keys")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
    ]);
    for tool in tools {
        let keys = extract_schema_keys(&tool.input_schema);
        table.add_row(vec![
            Cell::new(&tool.name).fg(Color::Green),
            Cell::new(&tool.description),
            Cell::new(keys),
        ]);
    }
    println!("{table}");
}

pub fn print_resources(resources: &[McpResource]) {
    if resources.is_empty() {
        println!("{}", "No resources available.".yellow());
        return;
    }
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("URI")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
        Cell::new("Name")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
        Cell::new("MIME Type")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
        Cell::new("Description")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
    ]);
    for r in resources {
        table.add_row(vec![
            Cell::new(&r.uri).fg(Color::Green),
            Cell::new(&r.name),
            Cell::new(&r.mime_type),
            Cell::new(&r.description),
        ]);
    }
    println!("{table}");
}

pub fn print_resource_templates(templates: &[McpResourceTemplate]) {
    if templates.is_empty() {
        return;
    }
    println!("{}", "Resource Templates:".bold());
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("URI Template")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
        Cell::new("Name")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
        Cell::new("MIME Type")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
        Cell::new("Description")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
    ]);
    for t in templates {
        table.add_row(vec![
            Cell::new(&t.uri_template).fg(Color::Green),
            Cell::new(&t.name),
            Cell::new(&t.mime_type),
            Cell::new(&t.description),
        ]);
    }
    println!("{table}");
}

pub fn print_prompts(prompts: &[McpPrompt]) {
    if prompts.is_empty() {
        println!("{}", "No prompts available.".yellow());
        return;
    }
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Name")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
        Cell::new("Description")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
        Cell::new("Arguments")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
    ]);
    for p in prompts {
        let args: Vec<String> = p
            .arguments
            .iter()
            .map(|a| {
                if a.required {
                    format!("{}*", a.name)
                } else {
                    a.name.clone()
                }
            })
            .collect();
        table.add_row(vec![
            Cell::new(&p.name).fg(Color::Green),
            Cell::new(&p.description),
            Cell::new(args.join(", ")),
        ]);
    }
    println!("{table}");
}

pub fn print_capabilities(caps: &ServerCapabilities) {
    let mut table = Table::new();
    table.set_header(vec![
        Cell::new("Capability")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
        Cell::new("Supported")
            .add_attribute(Attribute::Bold)
            .fg(Color::Cyan),
    ]);
    table.add_row(vec![
        Cell::new("tools"),
        Cell::new(if caps.tools.is_some() {
            "yes".green().to_string()
        } else {
            "no".red().to_string()
        }),
    ]);
    table.add_row(vec![
        Cell::new("resources"),
        Cell::new(if caps.resources.is_some() {
            "yes".green().to_string()
        } else {
            "no".red().to_string()
        }),
    ]);
    table.add_row(vec![
        Cell::new("prompts"),
        Cell::new(if caps.prompts.is_some() {
            "yes".green().to_string()
        } else {
            "no".red().to_string()
        }),
    ]);
    table.add_row(vec![
        Cell::new("logging"),
        Cell::new(if caps.logging.is_some() {
            "yes".green().to_string()
        } else {
            "no".red().to_string()
        }),
    ]);
    println!("{table}");
}

pub fn print_tool_result(result: &Value) {
    if let Some(content) = result.get("content") {
        if let Some(arr) = content.as_array() {
            for item in arr {
                let type_ = item.get("type").and_then(|v| v.as_str()).unwrap_or("text");
                match type_ {
                    "text" => {
                        let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        println!("{text}");
                    }
                    "image" => {
                        let mime = item.get("mimeType").and_then(|v| v.as_str()).unwrap_or("");
                        println!("{}", format!("[image/{mime}]").yellow());
                    }
                    _ => {
                        println!("{}", serde_json::to_string_pretty(item).unwrap_or_default());
                    }
                }
            }
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(content).unwrap_or_default()
            );
        }
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
    }
}

pub fn print_resource_result(result: &Value) {
    if let Some(contents) = result.get("contents") {
        if let Some(arr) = contents.as_array() {
            for item in arr {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    println!("{text}");
                } else if let Some(blob) = item.get("blob") {
                    println!("{}", format!("[binary data: {}]", blob).yellow());
                }
            }
        }
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
    }
}

pub fn print_notifications(notifications: &[Notification]) {
    if notifications.is_empty() {
        println!("{}", "No notifications.".yellow());
        return;
    }
    for (i, notif) in notifications.iter().enumerate() {
        let prefix = format!("[{}]", i + 1).dimmed();
        match notif {
            Notification::Log { level, message } => {
                let level_colored = match level.as_str() {
                    "error" => level.red().to_string(),
                    "warning" | "warn" => level.yellow().to_string(),
                    _ => level.blue().to_string(),
                };
                println!("{prefix} {level_colored}: {message}");
            }
            Notification::ToolListChanged => {
                println!("{prefix} {}", "tools/list_changed".cyan());
            }
            Notification::ResourceListChanged => {
                println!("{prefix} {}", "resources/list_changed".cyan());
            }
            Notification::PromptListChanged => {
                println!("{prefix} {}", "prompts/list_changed".cyan());
            }
            Notification::ServerRequest {
                method,
                params,
                responded,
            } => {
                let label = format!("server→client: {method}").magenta().to_string();
                let status = if *responded {
                    "responded".green().to_string()
                } else {
                    "no handler (replied method-not-found)".yellow().to_string()
                };
                let params_str = params
                    .as_ref()
                    .map(|p| format!(" params={p}"))
                    .unwrap_or_default();
                println!(
                    "{prefix} {label}{}{} — {status}",
                    params_str.dimmed(),
                    "".normal()
                );
            }
            Notification::Unknown { method, params } => {
                let params_str = params.as_ref().map(|p| p.to_string()).unwrap_or_default();
                println!("{prefix} {} {}", method.dimmed(), params_str.dimmed());
            }
        }
    }
}

pub fn print_prompt_messages(messages: &[crate::protocol::McpPromptMessage]) {
    for msg in messages {
        let role = msg.role.bold();
        match &msg.content {
            crate::protocol::McpContent::Text { text } => {
                println!("{}: {}", role, text);
            }
            crate::protocol::McpContent::Image { mime_type, .. } => {
                println!("{}: {}", role, format!("[image/{mime_type}]").yellow());
            }
        }
        println!();
    }
}

pub(crate) fn extract_schema_keys(schema: &Value) -> String {
    schema
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|obj| {
            let keys: Vec<&str> = obj.keys().map(|s| s.as_str()).collect();
            keys.join(", ")
        })
        .unwrap_or_default()
}

pub fn print_error(msg: &str) {
    eprintln!("{} {}", "Error:".red().bold(), msg);
}

pub fn print_success(msg: &str) {
    println!("{} {}", "✓".green().bold(), msg);
}

pub fn print_info(msg: &str) {
    println!("{} {}", "ℹ".blue(), msg);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_schema_keys_with_properties() {
        let schema = json!({"properties": {"a": {}, "b": {}}});
        let keys = extract_schema_keys(&schema);
        // Keys may be in any order
        assert!(keys.contains('a'));
        assert!(keys.contains('b'));
        assert!(keys.contains(", "));
    }

    #[test]
    fn extract_schema_keys_no_properties_key() {
        let schema = json!({"type": "object"});
        assert_eq!(extract_schema_keys(&schema), "");
    }

    #[test]
    fn extract_schema_keys_empty_properties() {
        let schema = json!({"properties": {}});
        assert_eq!(extract_schema_keys(&schema), "");
    }

    #[test]
    fn print_tools_empty_no_panic() {
        print_tools(&[]);
    }

    #[test]
    fn print_resource_templates_empty_no_panic() {
        print_resource_templates(&[]);
    }

    #[test]
    fn print_resource_templates_with_entries_no_panic() {
        let templates = vec![
            McpResourceTemplate {
                uri_template: "weather://{location}".to_string(),
                name: "Weather".to_string(),
                mime_type: "text/plain".to_string(),
                description: "Weather data".to_string(),
            },
            McpResourceTemplate {
                uri_template: "file://{path}".to_string(),
                name: String::new(),
                mime_type: String::new(),
                description: String::new(),
            },
        ];
        print_resource_templates(&templates);
    }
}
