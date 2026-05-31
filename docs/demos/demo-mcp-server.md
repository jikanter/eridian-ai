# MCP Server-Side: aichat --mcp and mcp_servers

*2026-05-01T16:59:50Z by Showboat 0.6.1*
<!-- showboat-id: 4d0ff528-06fd-4831-a3b9-1408e4d6125b -->

aichat exposes its functions and MCP-pool tools to external clients via `aichat --mcp` (stdio MCP server). External MCP servers configured under `mcp_servers:` in `config.yaml` are loaded into the same pool and re-advertised under namespaced names like `<server>:<tool>`.

This demo walks the protocol end-to-end. Sections 4 and 5 pin two regressions captured during the 2026-05-01 bridge-retirement validation pass; both were resolved in Phase 31 (`is_mcp_call` routing in `ToolCall::eval`, and concurrent + isolated pool init in `all_tool_declarations`). Section 6 demonstrates the Phase 31C portable `mcp.json` loader.

## 0. Setup

The exec blocks below share a tiny probe driver that builds an isolated `AICHAT_CONFIG_DIR` per call, writes a supplied YAML, and pipes JSON-RPC into `aichat --mcp`. They also share three YAML fixtures for the empty / git-only / git+sqlite scenarios. The setup block writes them to `/tmp` so `showboat verify` can re-run the demo end-to-end.

```bash
cat >/tmp/aichat_mcp_probe.sh <<'PROBE'
#!/usr/bin/env bash
set -e
yaml="$1"
cfg="$(mktemp -d)/aichat"
mkdir -p "$cfg"
cp "$yaml" "$cfg/config.yaml"
{
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    printf "%s\n" "$line"
    sleep 0.3
  done
  sleep 1
} | AICHAT_CONFIG_DIR="$cfg" timeout 30 "${AICHAT_BIN:-aichat}" --mcp 2>/dev/null
rm -rf "$cfg"
PROBE
chmod +x /tmp/aichat_mcp_probe.sh
cat >/tmp/aichat_mcp_empty.yaml <<EMPTY
model: ollama:gemma4:26b
function_calling: true
clients:
- type: openai-compatible
  name: ollama
  api_base: http://localhost:11434/v1
  models:
    - name: gemma4:26b
      max_input_tokens: 160000
      max_output_tokens: 8942
      supports_function_calling: true
EMPTY
cat >/tmp/aichat_mcp_git.yaml <<GITONLY
model: ollama:gemma4:26b
function_calling: true
clients:
- type: openai-compatible
  name: ollama
  api_base: http://localhost:11434/v1
  models:
    - name: gemma4:26b
      max_input_tokens: 160000
      max_output_tokens: 8942
      supports_function_calling: true

mcp_servers:
  git:
    command: /Users/admin/.local/bin/uvx
    args: ["mcp-server-git"]
GITONLY
cat >/tmp/aichat_mcp_small_n.yaml <<SMALLN
model: ollama:gemma4:26b
function_calling: true
clients:
- type: openai-compatible
  name: ollama
  api_base: http://localhost:11434/v1
  models:
    - name: gemma4:26b
      max_input_tokens: 160000
      max_output_tokens: 8942
      supports_function_calling: true

mcp_servers:
  sqlite:
    command: /Users/admin/.local/bin/uvx
    args: ["mcp-server-sqlite", "--db-path", "/tmp/probe-sqlite-demo.db"]
  git:
    command: /Users/admin/.local/bin/uvx
    args: ["mcp-server-git"]
SMALLN
echo setup-ok
```

```output
setup-ok
```

## 1. Initialize handshake (empty config)

With no functions and no `mcp_servers:` block, aichat initializes cleanly and advertises an empty tool list.

```bash
printf '%s\n%s\n%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | /tmp/aichat_mcp_probe.sh /tmp/aichat_mcp_empty.yaml | jq -c 'select(.id==1) | {protocol: .result.protocolVersion, server: .result.serverInfo.name, tools_capability: .result.capabilities.tools}'
```

```output
{"protocol":"2024-11-05","server":"aichat","tools_capability":{}}
```

```bash
printf '%s\n%s\n%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | /tmp/aichat_mcp_probe.sh /tmp/aichat_mcp_empty.yaml | jq -c 'select(.id==2) | {advertised_tool_count: (.result.tools | length)}'
```

```output
{"advertised_tool_count":0}
```

## 2. Adding an mcp_servers entry

Add one stdio MCP server (`mcp-server-git`) under `mcp_servers:`. aichat's native MCP client connects, the pool registers all 12 git tools, and lazy mode kicks in (threshold = 8), so only the `discover_roles` meta-tool is initially advertised.

```bash
printf '%s\n%s\n%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | /tmp/aichat_mcp_probe.sh /tmp/aichat_mcp_git.yaml | jq -c 'select(.id==1) | .result.capabilities.tools'
```

```output
{"listChanged":true}
```

```bash
printf '%s\n%s\n%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | /tmp/aichat_mcp_probe.sh /tmp/aichat_mcp_git.yaml | jq -c 'select(.id==2) | [.result.tools[].name]'
```

```output
["discover_roles"]
```

## 3. discover_roles surfaces the namespaced tool list

The `discover_roles` meta-tool returns a flat description of every tool in the pool. `mcp_servers:` tools are namespaced as `<server>:<tool>` to avoid collisions.

```bash
printf '%s\n%s\n%s\n%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"discover_roles","arguments":{"query":"git_"}}}' | /tmp/aichat_mcp_probe.sh /tmp/aichat_mcp_git.yaml | jq -r 'select(.id==3) | .result.content[0].text' | grep -E '^- git:' | sort | head -6
```

```output
- git:git_add: Adds file contents to the staging area
- git:git_branch: List Git branches
- git:git_checkout: Switches branches
- git:git_commit: Records changes to the repository
- git:git_create_branch: Creates a new branch from an optional base branch
- git:git_diff_staged: Shows changes that are staged for commit
```

## 4. Tool-call dispatch through the MCP pool (probe a, fixed in Phase 31A)

`aichat --mcp` dispatches single-call invocations through the same MCP-pool routing that `eval_tool_calls` uses. The shared predicate is `is_mcp_call` in `src/function.rs`. A namespaced call like `git:git_status` resolves through `mcp_pool.call_tool` instead of the llm-functions binary path.

```bash
printf '%s\n%s\n%s\n%s\n%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"discover_roles","arguments":{"query":"git_"}}}' '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"git:git_status","arguments":{"repo_path":"/tmp"}}}' | /tmp/aichat_mcp_probe.sh /tmp/aichat_mcp_git.yaml | jq -r 'select(.id==4) | .result.content[0].text // .error.message // "(no response)"' | grep -oE 'branch|nothing to commit|untracked' | head -1
```

```output
branch
```

The fix landed when `ToolCall::eval` grew the same MCP-pool dispatch as `eval_tool_calls`; both paths now share `is_mcp_call`.

## 5. Multi-server pool: per-server isolation (probe b, fixed in Phase 31B)

Concurrent stdio servers boot via `join_all` and register independently. A single hung or misconfigured server no longer aborts pool init for the rest. Two-server happy path:

```bash
printf '%s\n%s\n%s\n%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"discover_roles","arguments":{}}}' | /tmp/aichat_mcp_probe.sh /tmp/aichat_mcp_small_n.yaml | jq -r 'select(.id==3) | .result.content[0].text' | grep -oE '^- (sqlite|git):[a-z_]+' | awk -F: '{print $2}' | sort -u | head -1
```

```output
append_insight
```

```bash
printf '%s\n%s\n%s\n%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"discover_roles","arguments":{}}}' | /tmp/aichat_mcp_probe.sh /tmp/aichat_mcp_small_n.yaml | jq -r 'select(.id==3) | .result.content[0].text' | grep -cE '^- (sqlite|git):'
```

```output
18
```

## 6. Portable mcp.json declarations (Phase 31C)

`mcp_servers_file:` in `config.yaml` points at a Claude-Code-compatible `mcp.json` (`{"mcpServers": {...}}`). Aichat normalizes each entry into the same `McpServerConfig` it uses for inline `mcp_servers:` declarations and merges them — inline wins on key conflict. Search order when the field is unset: `./mcp.json`, `$XDG_CONFIG_HOME/mcp/mcp.json`, `~/.config/mcp/mcp.json`. See [`SPEC-mcp-json-artifact.md`](../architecture/integrated-architecture/SPEC-mcp-json-artifact.md).

```bash
cat >/tmp/aichat_mcp_portable.json <<JSON
{
  "mcpServers": {
    "git": {
      "command": "/Users/admin/.local/bin/uvx",
      "args": ["mcp-server-git"]
    }
  }
}
JSON
cat >/tmp/aichat_mcp_portable.yaml <<YAML
model: ollama:gemma4:26b
function_calling: true
clients:
- type: openai-compatible
  name: ollama
  api_base: http://localhost:11434/v1
  models:
    - name: gemma4:26b
      max_input_tokens: 160000
      max_output_tokens: 8942
      supports_function_calling: true

mcp_servers_file: /tmp/aichat_mcp_portable.json
YAML
printf '%s\n%s\n%s\n%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"discover_roles","arguments":{"query":"git"}}}' | /tmp/aichat_mcp_probe.sh /tmp/aichat_mcp_portable.yaml | jq -r 'select(.id==3) | .result.content[0].text' | grep -oE '^- git:[a-z_]+' | head -1
```

```output
- git:git_status
```

## Verification

```bash
showboat verify docs/demos/demo-mcp-server.md
```

The same protocol is encoded as repeatable bats tests in `tests/integration/mcp-server.sh`.
