# Phase 3 Design Document: MCP Consumption

**Date:** 2026-03-10
**Parent:** [initial-phased-roadmap.md](./initial-phased-roadmap.md)
**Prerequisite phases:** 0 (pipeline fixes), 1 (token efficiency foundations)

---

## Problem Statement

aichat currently serves as an MCP server (`--mcp`), exposing llm-functions tools to agents like Claude Code. But it cannot **consume** external MCP servers — it has no way to discover or call tools hosted by `npx @modelcontextprotocol/server-github`, `server-filesystem`, or any other MCP server.

This means an agent that needs both aichat's role-based orchestration AND external MCP tools must maintain two separate tool interfaces: one for aichat (CLI or MCP) and one for each external server (MCP). Each external server injects its full JSON schema into the agent's context, paying the tool tax independently.

If aichat can consume MCP servers and re-expose their tools through its own CLI/MCP interface, the agent talks to one tool (aichat) and aichat handles the protocol translation. Token savings compound: the agent pays CLI-rate discovery costs for tools that are actually backed by MCP servers.

---

## Decision: Build In-Process or Separate Binary?

The UX reviewer raised a valid question: does MCP consumption belong in aichat at all, or should it be a separate `mcp2cli` style binary ("one tool per job" ethos)?

### Arguments for separate binary

- Unix philosophy: aichat orchestrates LLM calls, a bridge tool bridges protocols
- No dependency cost to aichat for MCP client features
- Can be developed and released independently

### Arguments for in-process

- aichat's **role system** adds value on top of raw MCP tools — you can wrap an MCP tool in a role with better prompting, examples, and output formatting
- aichat's **pipeline system** can chain MCP tools with local tools and LLM calls in a single pipeline
- aichat's **deferred loading** (Phase 1C) can apply to MCP-consumed tools, not just local ones
- A separate binary means the agent needs two tools instead of one, re-introducing the tool tax

### Decision

**Build in-process, but as an isolated module.** The MCP client code lives in a new `src/mcp_client.rs` (not mixed into the existing `src/mcp.rs` server). The module is feature-gated behind a Cargo feature flag so it can be compiled out if the dependency cost is rejected.

```toml
[features]
default = ["mcp-client"]
mcp-client = []  # enables MCP consumption; rmcp client feature always available
```

---

## rmcp Client Support: What We Have

**Current dependency:** `rmcp = { version = "0.17", features = ["server", "transport-io"] }`

rmcp 0.17 supports client mode via optional features:

| Feature | Purpose | New Dependencies |
|---|---|---|
| `client` | Base client handler | `tokio-stream` |
| `transport-child-process` | Spawn MCP server as subprocess | None (uses tokio process) |
| `transport-streamable-http-client` | HTTP/SSE transport | `eventsource-stream`, `reqwest` |

**Proposed Cargo.toml change:**

```toml
rmcp = { version = "0.17", features = [
    "server",
    "transport-io",
    "client",                    # NEW
    "transport-child-process",   # NEW — for stdio MCP servers
] }
```

**Dependency cost:** `tokio-stream` is already a transitive dependency of tokio. `transport-child-process` uses tokio's built-in process spawning. Net new dependencies: **zero or near-zero.** This stays within the "no significant increase in dependencies" constraint.

HTTP transport (`transport-streamable-http-client-reqwest`) is deferred — `reqwest` is already a dependency, but SSE client support adds `eventsource-stream`. Only add if remote MCP servers are needed.

---

## Architecture

### Process Model

MCP stdio servers are long-lived processes. The key design decision is how to manage their lifecycle.

**Option A: Ephemeral (spawn per invocation)**
```
aichat --mcp-server "npx server-github" --list-tools
  → spawn npx server-github
  → MCP initialize handshake (~200ms)
  → tools/list call
  → kill process

Total: 1-3 seconds (Node.js cold start + handshake)
```

**Option B: Session-persistent (spawn once, reuse within session)**
```
First call:
  aichat --mcp-server "npx server-github" call create-issue ...
    → spawn npx server-github
    → MCP initialize handshake
    → tools/list (cache result)
    → tools/call create-issue
    → keep process alive

Subsequent calls (same session):
  aichat --mcp-server "npx server-github" call list-issues ...
    → reuse existing process
    → tools/call list-issues (no handshake)
```

**Option C: Config-based daemon**
```
# config.yaml
mcp_servers:
  github:
    command: ["npx", "@modelcontextprotocol/server-github"]
    env: { GITHUB_TOKEN: "${GITHUB_TOKEN}" }
  filesystem:
    command: ["npx", "@modelcontextprotocol/server-filesystem", "/home/user"]

# aichat manages server lifecycle, connection pooled
aichat --list-tools --source github
aichat call github:create-issue --title "Bug"
```

### Recommendation: Phased approach

- **Phase 3B (spike):** Option A — ephemeral. Accept the latency. Validate the architecture.
- **Phase 3C (production):** Option C — config-based with connection pooling. The `mcp_servers` config is analogous to Claude Code's `mcp.json`.

Option B is rejected because "session-persistent" has no clear lifecycle boundary in CMD mode (single invocation) and adds process management complexity without the benefits of a proper daemon.

---

## CLI Design

### Flag Structure

```
# Discovery
aichat --mcp-server <COMMAND> --list-tools
aichat --mcp-server <COMMAND> --list-tools -o json

# Tool help (single tool schema)
aichat --mcp-server <COMMAND> --tool-info <TOOL_NAME>

# Tool execution
aichat --mcp-server <COMMAND> call <TOOL_NAME> [--arg KEY=VALUE]...
aichat --mcp-server <COMMAND> call <TOOL_NAME> --json '{"key": "value"}'

# Config-based (Phase 3C)
aichat --list-tools --source github
aichat call github:create-issue --title "Bug" --body "Details"
```

### Flag Definition (clap)

```rust
/// Connect to an external MCP server (stdio transport)
#[clap(long = "mcp-server", value_name = "COMMAND")]
pub mcp_server: Option<String>,

/// List tools from an MCP server
#[clap(long, requires = "mcp_server")]
pub list_tools: bool,

/// Show info about a specific MCP tool
#[clap(long, value_name = "TOOL", requires = "mcp_server")]
pub tool_info: Option<String>,
```

### Why `--mcp-server` not `--mcp`

`--mcp` (existing) means "**be** an MCP server" (aichat listens on stdio).
`--mcp-server` means "**connect to** an MCP server" (aichat is the client).

These are opposite operations. Overloading the same flag would create a three-way ambiguity (serve stdio, serve HTTP, consume). Distinct flags with distinct semantics.

The noun "server" in `--mcp-server` describes **what you're connecting to**, matching the pattern of `--rag` (connects to a RAG index, not becomes one).

---

## Schema-to-CLI Argument Mapping

MCP tool schemas are JSON Schema. Converting to CLI flags is not always possible.

### The Problem

```json
{
  "name": "create-issue",
  "inputSchema": {
    "type": "object",
    "properties": {
      "title": { "type": "string" },
      "body": { "type": "string" },
      "labels": { "type": "array", "items": { "type": "string" } },
      "assignee": {
        "type": "object",
        "properties": {
          "login": { "type": "string" },
          "id": { "type": "integer" }
        }
      }
    },
    "required": ["title"]
  }
}
```

- `title` and `body`: flat string → clean `--title "Bug" --body "Details"`
- `labels`: array → `--labels '["bug","urgent"]'` (JSON string) or `--label bug --label urgent` (repeated flag)
- `assignee`: nested object → no clean CLI representation

### Design: Hybrid Approach

**Rule 1:** Top-level string, number, boolean, and enum properties become CLI flags.

```
--title "Bug"         # string
--priority 3          # number
--draft               # boolean (flag)
--state open          # enum
```

**Rule 2:** Top-level array-of-scalars become repeated flags.

```
--label bug --label urgent    # array of strings
```

**Rule 3:** Everything else (nested objects, arrays of objects, anyOf/oneOf) uses `--json` passthrough.

```
--json '{"assignee": {"login": "alice", "id": 42}}'
```

**Rule 4:** `--json` can be the *only* argument style (full passthrough, no flag generation).

```
aichat --mcp-server "npx server-github" call create-issue \
  --json '{"title": "Bug", "body": "Details", "labels": ["bug"]}'
```

**Rule 5:** Flags and `--json` can be mixed. Flag values override `--json` values.

```
aichat ... call create-issue --json '{"body": "Details"}' --title "Override"
# Merged: {"title": "Override", "body": "Details"}
```

### Implementation

Schema-to-flag conversion happens at **runtime**, not codegen. When `--list-tools` runs, the tool schemas are fetched and cached. When `call <tool>` runs, the cached schema is used to:

1. Generate a clap `Command` dynamically (or parse args manually)
2. Validate provided flags against the schema
3. Merge `--json` blob with flag values
4. Send merged JSON as `tools/call` arguments

Dynamic clap generation is possible via `clap::Command::new()` and `clap::Arg::new()` at runtime. This is well-supported and documented.

---

## Authentication

MCP servers need credentials. Three mechanisms, in order of implementation:

### 1. Environment Variable Passthrough (Phase 3B)

The simplest approach. MCP server commands inherit the caller's environment:

```bash
export GITHUB_TOKEN=ghp_xxx
aichat --mcp-server "npx @modelcontextprotocol/server-github" --list-tools
```

The spawned process receives all environment variables from the parent. This works for every MCP server that uses env-var auth (which is most of them).

**Implementation:** Zero code. `tokio::process::Command` inherits env by default.

### 2. Config-Based Environment (Phase 3C)

For config-based server definitions, explicit env vars:

```yaml
mcp_servers:
  github:
    command: ["npx", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_TOKEN: "${GITHUB_TOKEN}"     # reference parent env
      GITHUB_ORG: "my-org"               # literal value

  private-api:
    command: ["npx", "server-private"]
    env:
      API_KEY:
        file: "~/.config/private-api/key"  # read from file
```

The `${VAR}` syntax references the parent process environment. Literal values are passed directly. `file:` reads the value from a file path (for secrets that shouldn't be in config).

### 3. OAuth (Deferred)

For HTTP-transport MCP servers that use OAuth, defer to rmcp's built-in OAuth support or to a future phase. Stdio-transport servers (the primary target) don't use OAuth.

---

## Caching Strategy

### Tool Schema Cache

MCP `tools/list` results are cached to avoid re-fetching on every invocation.

```
~/.cache/aichat/mcp/
  └── <server-hash>/
      ├── tools.json          # cached tools/list result
      └── meta.json           # { "fetched_at": "...", "server_info": {...} }
```

**Cache key:** SHA-256 of the server command string (e.g., `sha256("npx @modelcontextprotocol/server-github")`).

**Cache TTL:** 1 hour (configurable via `mcp_cache_ttl` in config.yaml). Overridden by `--refresh` flag.

**Invalidation:** Manual (`--refresh`) or TTL expiry. No filesystem watch — MCP servers are remote processes, their tool sets don't change based on local file modifications.

### Connection Cache (Phase 3C only)

For config-based servers, maintain a connection pool:

```rust
struct McpConnectionPool {
    connections: HashMap<String, McpConnection>,
}

struct McpConnection {
    client: rmcp::Client,        // rmcp client handle
    child: tokio::process::Child, // spawned server process
    tools: Vec<Tool>,            // cached tools/list
    last_used: Instant,          // for idle timeout
}
```

**Idle timeout:** 5 minutes. After 5 minutes of no calls, the MCP server process is killed and the connection dropped. Next call re-spawns.

**Maximum connections:** 10 (configurable). Prevents runaway process spawning.

---

## Integration with Tool Dispatch

The critical architectural question: how do MCP-consumed tools merge into aichat's existing `FunctionDeclaration` / `ToolCall` / `select_functions` system?

### Current Dispatch Flow

```
FunctionDeclaration (from functions.json)
    → select_functions(role) filters by use_tools
    → LLM receives schemas, returns ToolCall
    → ToolCall::eval() → extract_call_config_from_config()
    → run_llm_function() → exec binary → read $LLM_OUTPUT
```

### Proposed: Unified Dispatch with Source Tag

Add a `source` field to `FunctionDeclaration`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: JsonSchema,
    #[serde(skip_serializing, default)]
    pub agent: bool,
    #[serde(skip, default)]
    pub source: ToolSource,      // NEW
}

#[derive(Debug, Clone, Default)]
pub enum ToolSource {
    #[default]
    Local,                       // exec binary (current behavior)
    Mcp {
        server_name: String,     // config key or command hash
    },
}
```

### Modified Dispatch Flow

```
FunctionDeclaration (from functions.json OR mcp tools/list)
    → select_functions(role) filters by use_tools
        → MCP tools are namespaced: "github:create-issue"
        → mapping_tools supports MCP groups: "github: github:*"
    → LLM receives schemas, returns ToolCall
    → ToolCall::eval()
        → match source:
            ToolSource::Local → run_llm_function() (existing)
            ToolSource::Mcp { server_name } → run_mcp_tool() (new)
```

### `run_mcp_tool` Implementation

```rust
async fn run_mcp_tool(
    server_name: &str,
    tool_name: &str,
    arguments: Value,
    config: &GlobalConfig,
) -> Result<Value> {
    let pool = config.read().mcp_connection_pool();
    let connection = pool.get_or_connect(server_name).await?;

    let params = CallToolRequestParams {
        name: tool_name.into(),
        arguments: match arguments {
            Value::Object(map) => Some(map),
            _ => None,
        },
    };

    let result = connection.client
        .call_tool(params)
        .await
        .map_err(|e| anyhow!("MCP tool '{tool_name}' failed: {e}"))?;

    // Convert MCP Content to Value
    let text = result.content
        .into_iter()
        .filter_map(|c| match c {
            Content::Text(t) => Some(t.text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    Ok(json!({"output": text}))
}
```

### Tool Namespacing

MCP tools are namespaced by server name to avoid collisions with local tools:

```
Local tool:     fs_cat
MCP tool:       github:create-issue
MCP tool:       filesystem:read-file
```

The namespace is stripped before sending to the MCP server (the server knows its tools as `create-issue`, not `github:create-issue`).

In role YAML:

```yaml
use_tools: fs_cat,github:create-issue,github:list-issues
```

Or via mapping_tools:

```yaml
mapping_tools:
  fs: 'fs_cat,fs_ls,fs_write'
  github: 'github:*'           # wildcard: all tools from github server
```

---

## Error Handling

### Connection Errors

```
error: could not connect to MCP server "npx server-github"
  cause: process exited with code 1
  stderr: npm ERR! code ENOENT ...
  hint: ensure the server package is installed and GITHUB_TOKEN is set
```

Three-part error: what failed, why (stderr from the process), and a hint for common causes.

### Handshake Errors

```
error: MCP server "npx server-github" did not complete initialization
  cause: timed out after 10s
  hint: the server may require additional configuration or a newer Node.js version
```

### Tool Call Errors

```
error: MCP tool "github:create-issue" failed
  cause: {"code": -32602, "message": "Missing required field: title"}
```

MCP errors include JSON-RPC error codes. Map common codes to user-friendly messages:

| Code | Meaning | User Message |
|---|---|---|
| -32600 | Invalid request | "Invalid arguments for tool" |
| -32601 | Method not found | "Tool not found on server" |
| -32602 | Invalid params | "Missing or invalid parameters" |
| -32603 | Internal error | "Server internal error" |

### Timeout

All MCP operations have configurable timeouts:

| Operation | Default | Config Key |
|---|---|---|
| Server startup | 30s | `mcp_startup_timeout` |
| Initialize handshake | 10s | (hardcoded) |
| tools/list | 10s | (hardcoded) |
| tools/call | 120s | `mcp_call_timeout` |

---

## Implementation Phases

### Phase 3A: Design Validation (This Document)

Deliverable: This document, reviewed and approved. No code.

### Phase 3B: Read-Only Spike

**Goal:** Validate the architecture with discovery only. No tool execution.

**Scope:**
1. Add `client` and `transport-child-process` features to rmcp in `Cargo.toml`
2. Add `--mcp-server <COMMAND>` and `--list-tools` flags to `cli.rs`
3. Create `src/mcp_client.rs`:
   - `connect(command: &str) -> Result<McpClient>` — spawn process, handshake
   - `list_tools(client: &McpClient) -> Result<Vec<Tool>>` — fetch tool list
   - `disconnect(client: McpClient)` — kill process
4. Wire `--list-tools` output through `-o json` for agent consumption
5. Add schema cache (`~/.cache/aichat/mcp/`)

**Not in scope:** Tool execution, config-based servers, connection pooling, auth config.

**Estimated scope:** ~150-200 lines in `src/mcp_client.rs`, ~20 lines in `src/cli.rs`, ~10 lines in `src/main.rs`.

**Validation criteria:**
```bash
# Must work:
aichat --mcp-server "npx @modelcontextprotocol/server-filesystem /tmp" --list-tools
aichat --mcp-server "npx @modelcontextprotocol/server-filesystem /tmp" --list-tools -o json

# Must handle errors:
aichat --mcp-server "nonexistent-binary" --list-tools
# → clear error message, no panic

# Must cache:
time aichat --mcp-server "npx server-filesystem /tmp" --list-tools  # slow (cold)
time aichat --mcp-server "npx server-filesystem /tmp" --list-tools  # fast (cached)
```

### Phase 3C: Tool Execution

**Goal:** Call MCP tools from CLI and from within aichat's tool-calling loop.

**Scope:**
1. Add `call` subcommand parsing (dynamic arg generation from cached schema)
2. Implement `run_mcp_tool()` — send `tools/call` via rmcp client
3. Add `ToolSource::Mcp` variant to `FunctionDeclaration`
4. Modify `ToolCall::eval()` to dispatch based on `source`
5. Add `--json` passthrough and hybrid flag/JSON argument merging
6. Add `--tool-info <TOOL>` for single-tool schema inspection

**Estimated scope:** ~200-300 lines in `src/mcp_client.rs`, ~50 lines in `src/function.rs`.

**Validation criteria:**
```bash
# Flat args:
aichat --mcp-server "npx server-filesystem /tmp" call read-file --path "/tmp/test.txt"

# JSON passthrough:
aichat --mcp-server "npx server-github" call create-issue \
  --json '{"title": "Bug", "body": "Details", "labels": ["bug"]}'

# Mixed:
aichat --mcp-server "npx server-github" call create-issue \
  --title "Bug" --json '{"labels": ["bug"]}'

# From within LLM tool-calling (use_tools references MCP server):
aichat -r my-role "create a github issue for this bug"
# → model calls github:create-issue → dispatched via MCP client
```

### Phase 3D: Config-Based Servers & Connection Pooling

**Goal:** Persistent server definitions with managed lifecycle.

**Scope:**
1. Add `mcp_servers` section to config.yaml parsing
2. Implement `McpConnectionPool` with idle timeout and max connections
3. Add env var config (`env:`, `${VAR}`, `file:` syntax)
4. Load MCP server tools at startup (lazy: on first use, not at config load)
5. Merge MCP tool declarations into `select_functions()` with namespacing
6. Add `mapping_tools` wildcard support (`github:*`)

**Config format:**
```yaml
mcp_servers:
  github:
    command: ["npx", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_TOKEN: "${GITHUB_TOKEN}"
  filesystem:
    command: ["npx", "@modelcontextprotocol/server-filesystem", "/home/user"]

mcp_cache_ttl: 3600          # seconds, default 1 hour
mcp_startup_timeout: 30      # seconds
mcp_call_timeout: 120        # seconds
mcp_max_connections: 10
```

**Estimated scope:** ~300-400 lines across `src/mcp_client.rs`, `src/config/mod.rs`.

---

## Open Questions

### 1. Tool name collision resolution

If a local tool and an MCP tool have the same name, which wins? Options:
- **Local wins** (backward compatible, MCP tools always namespaced)
- **Error** (force explicit namespacing)
- **Config priority** (tool_sources order determines precedence)

**Recommended:** Local wins. MCP tools are always namespaced (`github:create-issue`). If someone creates a local tool called `create-issue`, it's unambiguous.

### 2. MCP server version pinning

MCP servers can change their tool schemas between versions. Should the cache store a version hash and invalidate when the server's `Implementation.version` changes?

**Recommended:** Yes. Store `server_info.version` in `meta.json`. On connect, compare. If different, invalidate cache and re-fetch `tools/list`.

### 3. Streaming tool results

Some MCP tools return streaming results (e.g., long-running queries). rmcp supports this via progress notifications. Should aichat stream MCP tool results to the terminal?

**Recommended:** Defer. For Phase 3B-3C, collect full result before returning. Streaming MCP results adds complexity (progress bars, partial output) that isn't needed for the initial implementation.

### 4. Resource and prompt support

MCP servers can expose resources (files, data) and prompts (templates) in addition to tools. Should aichat consume these?

**Recommended:** Defer. Tools are the primary use case and align with aichat's existing `FunctionDeclaration` system. Resources and prompts are a different abstraction that would need new data structures.

### 5. Remote MCP servers (HTTP transport)

The initial implementation targets stdio transport (local processes). Should HTTP/SSE transport be supported for remote MCP servers?

**Recommended:** Defer to Phase 3E. Add `transport-streamable-http-client-reqwest` feature to rmcp when needed. The config format already supports it:

```yaml
mcp_servers:
  remote-api:
    url: "https://mcp.example.com/sse"
    auth:
      header: "Authorization: Bearer ${API_TOKEN}"
```

---

## File Layout

```
src/
├── mcp.rs              # Existing MCP server (unchanged)
├── mcp_client.rs       # NEW: MCP client, connection pool, schema cache
├── function.rs          # Modified: ToolSource enum, dispatch routing
├── cli.rs              # Modified: --mcp-server, --list-tools, --tool-info flags
├── config/
│   └── mod.rs          # Modified: mcp_servers config, select_functions namespacing
└── main.rs             # Modified: --mcp-server dispatch
```

---

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| rmcp 0.17 client API is unstable or underdocumented | Medium | High | Phase 3B spike validates before committing. rmcp is the official MCP Rust SDK (modelcontextprotocol/rust-sdk). |
| MCP server startup latency (1-3s for Node.js) makes CLI feel slow | High | Medium | Schema caching eliminates latency for discovery. Connection pooling (Phase 3D) eliminates it for execution. Accept latency in Phase 3B spike. |
| Dynamic CLI arg generation from JSON Schema is complex for edge cases | Medium | Low | `--json` passthrough is always available as escape hatch. Only generate flags for flat properties. |
| Process lifecycle bugs (zombie processes, leaked connections) | Medium | High | Explicit shutdown in `Drop` impl. Idle timeout kills stale processes. `SIGTERM` then `SIGKILL` after 5s. |
| Tool schema changes between server restarts | Low | Low | Version-based cache invalidation. `--refresh` flag for manual override. |
| Dependency size increase from rmcp client features | Low | Medium | `client` + `transport-child-process` add near-zero new deps (tokio-stream is already transitive). Measured before merging. |

---

## Success Criteria

Phase 3 is complete when:

1. `aichat --mcp-server <cmd> --list-tools` discovers tools from any stdio MCP server
2. `aichat --mcp-server <cmd> call <tool> [args]` executes tools with proper error handling
3. MCP tools are callable from within aichat's LLM tool-calling loop via `use_tools: github:create-issue`
4. Config-based server definitions work with env var passthrough
5. Connection pooling keeps server processes alive across calls within a session
6. Schema cache eliminates redundant `tools/list` calls
7. All existing tests pass (no regressions to MCP server mode or local tool dispatch)
