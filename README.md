# mcpi — MCP Inspector CLI

An interactive command-line tool for connecting to and inspecting [Model Context Protocol (MCP)](https://modelcontextprotocol.io) servers. Mirrors the capabilities of the [MCP Inspector](https://modelcontextprotocol.io/docs/tools/inspector) web tool, but runs entirely in your terminal.

## Features

- Connect to MCP servers via **stdio** (subprocess) or **HTTP/SSE** transports
- Inspect and invoke **tools**, **resources**, and **prompts**
- View server **notifications** in real time or buffered
- **Tab completion** for commands, tool names, resource URIs, and prompt names
- **Command history** persisted across sessions
- Export **Claude Desktop-compatible JSON** configuration
- **Configurable request timeout** via CLI flag or REPL command
- **Fast failure** on bad connections — shows server stderr and exits immediately when the process dies

## Installation

```bash
git clone https://github.com/youruser/mcpinspector
cd mcpinspector
cargo install --path .
```

Or build locally and run from the project directory:

```bash
cargo build --release
./target/release/mcpi
```

## Usage

```
mcpi [OPTIONS]
```

### Options

| Flag | Description |
|------|-------------|
| `--connect <CMD>` | Connect to a stdio MCP server on startup |
| `--connect-http <URL>` | Connect to an HTTP MCP server on startup |
| `--live` | Print server notifications immediately instead of buffering them |
| `--timeout <SECS>` | Request timeout in seconds (default: 10) |
| `-h, --help` | Print help |
| `-V, --version` | Print version |

### Start the REPL

```bash
# Start the REPL with no connection
mcpi

# Auto-connect to a stdio server on startup
mcpi --connect "npx -y @modelcontextprotocol/server-filesystem /tmp"

# Auto-connect to an HTTP server on startup
mcpi --connect-http http://localhost:3000/mcp

# Print server notifications live as they arrive
mcpi --live --connect "npx -y @modelcontextprotocol/server-filesystem /tmp"

# Set a custom request timeout (default is 10s)
mcpi --timeout 30 --connect "npx -y @modelcontextprotocol/server-filesystem /tmp"
```

---

## REPL Commands

Once inside the REPL, you'll see the `mcpi>` prompt. Type `help` at any time to see available commands.

### Connection

#### `connect <command> [args...]`
Connect to an MCP server via stdio. The command is spawned as a child process.

```
mcpi> connect npx -y @modelcontextprotocol/server-filesystem /tmp
mcpi> connect python my_mcp_server.py --port 8080
mcpi> connect uv run mcp-server-git --repository /path/to/repo
```

#### `connect-http <url>`
Connect to an MCP server via HTTP with SSE fallback.

```
mcpi> connect-http http://localhost:3000/mcp
```

#### `disconnect`
Disconnect from the current server.

```
mcpi> disconnect
```

#### `status`
Show the current connection status, transport details, server capabilities, active timeout, and pending notification count.

```
mcpi> status
```

---

### Tools

#### `tools`
List all tools the server exposes, showing name, description, and input parameter keys.

```
mcpi> tools
```

#### `call <name> [json]`
Execute a tool. Optionally pass arguments as a JSON object.

```
mcpi> call list_directory {"path": "/tmp"}
mcpi> call read_file {"path": "/tmp/notes.txt"}
mcpi> call search_files {"pattern": "*.log", "path": "/var/log"}

# No arguments needed for some tools
mcpi> call get_current_time
```

Tab completion works for tool names after `call`.

---

### Resources

#### `resources`
List all resources the server exposes, showing URI, name, MIME type, and description.

```
mcpi> resources
```

#### `read <uri>`
Read the content of a resource by URI.

```
mcpi> read file:///tmp/notes.txt
mcpi> read config://app/settings
```

Tab completion works for resource URIs after `read`.

---

### Prompts

#### `prompts`
List all prompts the server exposes, showing name, description, and required/optional arguments (required args are marked with `*`).

```
mcpi> prompts
```

#### `prompt <name> [json]`
Retrieve a prompt, optionally passing arguments as a JSON object.

```
mcpi> prompt summarize {"topic": "climate change"}
mcpi> prompt code_review {"language": "rust", "code": "fn main() {}"}

# No arguments
mcpi> prompt greeting
```

Tab completion works for prompt names after `prompt`.

---

### Client Capabilities

MCP supports server-to-client requests — the server can call back into the client to ask questions during a session. Common examples are `roots/list` (filesystem server asking for your workspace roots) and `sampling/createMessage` (a server asking the client to run an LLM completion).

`mcpi` lets you configure fixed JSON responses for these server-initiated methods. When a handler is registered, the capability is automatically advertised in the `initialize` handshake so the server knows it can call that method.

#### `cap-set <method> <json>`
Register a fixed JSON response for a server-initiated method. Must be set **before connecting** so the capability is included in the `initialize` advertisement.

```
mcpi> cap-set roots/list {"roots":[{"uri":"file:///home/user/repos","name":"repos"}]}
mcpi> cap-set sampling/createMessage {"role":"assistant","content":{"type":"text","text":"stub"}}
```

#### `cap-list`
Show all configured capability handlers and their response payloads.

```
mcpi> cap-list
```

#### `cap-remove <method>`
Remove a configured handler.

```
mcpi> cap-remove roots/list
```

---

#### Example: `roots/list` with `server-filesystem`

The `@modelcontextprotocol/server-filesystem` package will call `roots/list` on the client immediately after connecting, to discover which workspace roots it should treat as authoritative. Without a handler, the server times out, logs a `notifications/cancelled` error, and carries on (using only the directories you passed on the command line).

With a handler configured, the server receives a real response and the exchange completes cleanly:

```
$ mcpi
MCP Inspector (mcpi)
Type 'help' for available commands, 'quit' to exit.

mcpi> cap-set roots/list {"roots":[{"uri":"file:///home/oshea00/repos","name":"repos"}]}
✓ Capability handler set for 'roots/list'

mcpi> connect npx @modelcontextprotocol/server-filesystem /home/oshea00/repos
ℹ Connecting to 'npx'...
✓ Connected!
+------------+--------------+
| Capability | Supported    |
+===========================+
| tools      | yes          |
|------------+--------------|
| resources  | no           |
|------------+--------------|
| prompts    | no           |
|------------+--------------|
| logging    | no           |
+------------+--------------+

mcpi> call list_allowed_directories
ℹ Calling tool 'list_allowed_directories'...
Allowed directories:
/home/oshea00/repos
[1 new notification(s) — type 'log' to view]

mcpi> log
[1] server→client: roots/list — responded
```

Compare this to the same session **without** a handler:

```
mcpi> log
[1] server→client: roots/list — no handler (replied method-not-found)
```

Without a handler `mcpi` still replies immediately with a `method-not-found` error (so the server doesn't hang waiting for a timeout), but records the event in the notification log so you can see the interchange took place.

---

### Configuration & Export

#### `set-timeout <seconds>`
Change the request timeout mid-session. Applies to all subsequent requests, including the next `connect`. The default is 10 seconds.

```
mcpi> set-timeout 30
mcpi> set-timeout 5
```

This is equivalent to starting with `--timeout <seconds>` but can be adjusted at any point without restarting.

#### `set-name <name>`
Set the server name used when exporting configuration. Defaults to `mcp-server`.

```
mcpi> set-name my-filesystem-server
```

#### `set-env <key> <value>`
Add an environment variable to the connection config. Useful before connecting if the server requires env vars, or before exporting.

```
mcpi> set-env API_KEY sk-abc123
mcpi> set-env HOME /home/user
```

#### `export [filename]`
Export a [Claude Desktop](https://claude.ai/download)-compatible JSON configuration. If no filename is given, prints to stdout.

```
# Print to stdout
mcpi> export

# Write to a file
mcpi> export myserver.json
```

Example output (stdio server):
```json
{
  "mcpServers": {
    "my-filesystem-server": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
      "env": {
        "API_KEY": "sk-abc123"
      }
    }
  }
}
```

Example output (HTTP server):
```json
{
  "mcpServers": {
    "my-filesystem-server": {
      "url": "http://localhost:3000/mcp"
    }
  }
}
```

To use this in Claude Desktop, merge the `mcpServers` block into your `claude_desktop_config.json` (usually at `~/.config/Claude/claude_desktop_config.json` on Linux or `~/Library/Application Support/Claude/claude_desktop_config.json` on macOS).

---

### Notifications

MCP servers can send notifications (log messages, list-changed events). By default, `mcpi` buffers them and shows a count indicator in the prompt area.

#### `log`
Display all buffered notifications and clear the buffer.

```
mcpi> log
```

To see notifications printed immediately as they arrive, start `mcpi` with `--live`.

---

### Session

#### `history`
Show the command history for the current session.

```
mcpi> history
```

History is also persisted to disk between sessions at:
- **Linux:** `~/.local/share/mcpi/history.txt`
- **macOS:** `~/Library/Application Support/mcpi/history.txt`

#### `clear`
Clear the terminal screen.

```
mcpi> clear
```

#### `help [command]`
Show all commands, or detailed help for a specific command.

```
mcpi> help
mcpi> help connect
mcpi> help export
```

#### `quit` / `exit`
Exit the REPL. History is saved automatically.

```
mcpi> quit
```

---

## Tab Completion

`mcpi` supports tab completion at the `mcpi>` prompt:

- **First word:** completes command names
- **After `call`:** completes tool names
- **After `read`:** completes resource URIs
- **After `prompt`:** completes prompt names

Press `Tab` once to complete, or twice to see all options.

---

## Example Session

```
$ mcpi
MCP Inspector (mcpi)
Type 'help' for available commands, 'quit' to exit.

mcpi> connect npx -y @modelcontextprotocol/server-filesystem /tmp
ℹ Connecting to 'npx'...
✓ Connected!
┌────────────┬───────────┐
│ Capability │ Supported │
├────────────┼───────────┤
│ tools      │ yes       │
│ resources  │ no        │
│ prompts    │ no        │
│ logging    │ no        │
└────────────┴───────────┘

mcpi> tools
┌────────────────────┬──────────────────────────────────────────────┬─────────────────────┐
│ Name               │ Description                                  │ Input Keys          │
├────────────────────┼──────────────────────────────────────────────┼─────────────────────┤
│ read_file          │ Read the complete contents of a file         │ path                │
│ read_multiple_files│ Read the contents of multiple files at once  │ paths               │
│ write_file         │ Create a new file or overwrite an existing…  │ path, content       │
│ list_directory     │ Get a listing of all files and directories…  │ path                │
└────────────────────┴──────────────────────────────────────────────┴─────────────────────┘

mcpi> call list_directory {"path": "/tmp"}
/tmp/notes.txt
/tmp/test/

mcpi> call read_file {"path": "/tmp/notes.txt"}
Hello from MCP!

mcpi> set-name filesystem-tmp
✓ Server name set to 'filesystem-tmp'

mcpi> export claude_config.json
Config written to claude_config.json

mcpi> quit
```

---

## Protocol

`mcpi` speaks [JSON-RPC 2.0](https://www.jsonrpc.org/specification) directly over the chosen transport. It implements the [MCP 2024-11-05 specification](https://spec.modelcontextprotocol.io) as a client, supporting:

- `initialize` / `notifications/initialized` handshake
- `tools/list`, `tools/call`
- `resources/list`, `resources/read`
- `prompts/list`, `prompts/get`
- Incoming notifications: `notifications/message`, `notifications/tools/list_changed`, `notifications/resources/list_changed`, `notifications/prompts/list_changed`
- Server-to-client requests: any method registered via `cap-set` is responded to with the configured payload; unregistered methods receive a `method-not-found` error immediately (no silent timeout)

Requests time out after 10 seconds by default (configurable via `--timeout` or `set-timeout`). If the server process exits before responding, pending requests fail immediately rather than waiting for the timeout. Server stderr output is captured and shown in the error message on connection failure.
