# Phase 3: MCP Consumption - CLI Integration

*2026-03-30T15:48:23Z by Showboat 0.6.1*
<!-- showboat-id: 02af83ac-10ad-4371-bc64-7391dc1f6c08 -->

aichat can consume external MCP servers as a client. This enables tool discovery, inspection, and direct invocation from the CLI, using any MCP-compatible server over stdio transport.

## Discovery

Connect to the MCP filesystem server and list its tools. The `--mcp-server` flag takes a command string that launches the server process.

```bash
aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --list-tools | sort | head -3
```

```output
create_directory - Create a new directory or ensure a directory exists. Can create multiple nested directories in one operation. If the directory already exists, this operation will succeed silently. Perfect for setting up directory structures for projects or ensuring required paths exist. Only works within allowed directories.
directory_tree - Get a recursive tree view of files and directories as a JSON structure. Each entry includes 'name', 'type' (file/directory), and 'children' for directories. Files have no children array, while directories always have a children array (which may be empty). The output is formatted with 2-space indentation for readability. Only works within allowed directories.
edit_file - Make line-based edits to a text file. Each edit replaces exact line sequences with new content. Returns a git-style diff showing the changes made. Only works within allowed directories.
```

Structured JSON output via `-o json` for agent consumption:

```bash
aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --list-tools -o json | jq '[.[] | .name] | sort | .[:3] | .[]'
```

```output
"create_directory"
"directory_tree"
"edit_file"
```

## Tool Inspection

Inspect a single tool's full schema with `--tool-info`. This returns the tool name, description, and JSON Schema for its parameters:

```bash
aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --tool-info list_allowed_directories
```

```output
{
  "name": "list_allowed_directories",
  "description": "Returns the list of directories that this server is allowed to access. Subdirectories within these allowed directories are also accessible. Use this to understand which directories and their nested paths are available before trying to access files.",
  "parameters": {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "type": "object",
    "properties": {}
  }
}
```

## Tool Execution

Call tools directly with `--call` and `--json` arguments. First, create a test file, then read it via MCP:

```bash
echo 'Phase 3 MCP test content' > /tmp/mcp_demo.txt && cat /tmp/mcp_demo.txt
```

```output
Phase 3 MCP test content
```

```bash
aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --call read_file --json '{"path": "/private/tmp/mcp_demo.txt"}'
```

```output
Phase 3 MCP test content

```

The `list_allowed_directories` tool takes no arguments and returns the server's allowed paths:

```bash
aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --call list_allowed_directories --json '{}'
```

```output
Allowed directories:
/private/tmp
```

## Error Handling

Clear error messages for connection failures when the server binary does not exist:

```bash
aichat --mcp-server 'nonexistent-binary' --list-tools 2>&1 || true
```

```output
Error: Could not start MCP server "nonexistent-binary": No such file or directory (os error 2)
hint: ensure the server binary is installed and on PATH
```

## Schema Caching and Config-Based Servers

Tool discovery results are cached with a 1-hour TTL. After the first `--list-tools` call, subsequent calls return instantly from cache without spawning a server process.

MCP servers can also be defined in `config.yaml` for persistent, named access:

    mcp_servers:
      github:
        command: npx
        args: ["@modelcontextprotocol/server-github"]
        env:
          GITHUB_TOKEN: "${GITHUB_TOKEN}"
      filesystem:
        command: npx
        args: ["@modelcontextprotocol/server-filesystem", "/home/user"]

MCP tools are namespaced by server name (e.g., `github:create_issue`, `filesystem:read_file`) to avoid collisions. Wildcard patterns like `github:*` are supported in `use_tools` and `mapping_tools`.

## Integration Tests

Automated tests to verify MCP client behavior remains correct.

### Test 1: Error handling for nonexistent server

```bash
output=$(aichat --mcp-server 'nonexistent-binary' --list-tools 2>&1 || true) && echo "$output" | grep -q 'No such file or directory' && echo 'PASS: error message contains expected text' || echo 'FAIL: unexpected error output'
```

```output
PASS: error message contains expected text
```

### Test 2: Tool listing returns expected tools

```bash
tools=$(aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --list-tools | sort) && count=$(echo "$tools" | wc -l | tr -d ' ') && has_read=$(echo "$tools" | grep -c 'read_file') && has_list=$(echo "$tools" | grep -c 'list_directory') && echo "tool_count>=10: $([ "$count" -ge 10 ] && echo PASS || echo FAIL)" && echo "has_read_file: $([ "$has_read" -ge 1 ] && echo PASS || echo FAIL)" && echo "has_list_directory: $([ "$has_list" -ge 1 ] && echo PASS || echo FAIL)"
```

```output
tool_count>=10: PASS
has_read_file: PASS
has_list_directory: PASS
```

### Test 3: Tool info returns valid JSON schema

```bash
info=$(aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --tool-info list_allowed_directories) && has_name=$(echo "$info" | jq -r '.name' 2>/dev/null) && has_params=$(echo "$info" | jq -r '.parameters.type' 2>/dev/null) && echo "valid_json: $([ -n "$has_name" ] && echo PASS || echo FAIL)" && echo "name_correct: $([ "$has_name" = 'list_allowed_directories' ] && echo PASS || echo FAIL)" && echo "has_parameters: $([ "$has_params" = 'object' ] && echo PASS || echo FAIL)"
```

```output
valid_json: PASS
name_correct: PASS
has_parameters: PASS
```

### Test 4: Tool call reads file content correctly

```bash
echo 'Phase 3 MCP test content' > /tmp/mcp_demo.txt && result=$(aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --call read_file --json '{"path": "/private/tmp/mcp_demo.txt"}') && echo "content_match: $([ "$result" = 'Phase 3 MCP test content' ] && echo PASS || echo FAIL)"
```

```output
content_match: PASS
```

### Test 5: Tool call with no-arg tool returns expected output

```bash
result=$(aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --call list_allowed_directories --json '{}') && echo "contains_tmp: $(echo "$result" | grep -q '/private/tmp' && echo PASS || echo FAIL)"
```

```output
contains_tmp: PASS
```
