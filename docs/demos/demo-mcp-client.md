# Phase 3: MCP Consumption - CLI Integration

*2026-03-11T06:36:40Z by Showboat 0.6.1*
<!-- showboat-id: 689589dc-32ef-4ee6-8eaa-63ce40afab31 -->

aichat can now consume external MCP servers as a client. This enables aichat to discover and call tools hosted by any MCP-compatible server, while re-exposing them through its own CLI and tool-calling interface.

## Phase 3B: Discovery

Connect to the MCP filesystem server and list its tools. The `--mcp-server` flag takes a command string that launches the server process. Discovery results are cached (1-hour TTL) for subsequent calls.

```bash
aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --list-tools | head -3
```

```output
read_file - Read the complete contents of a file as text. DEPRECATED: Use read_text_file instead.
read_text_file - Read the complete contents of a file from the file system as text. Handles various text encodings and provides detailed error messages if the file cannot be read. Use this tool when you need to examine the contents of a single file. Use the 'head' parameter to read only the first N lines of a file, or the 'tail' parameter to read only the last N lines of a file. Operates on the file as text regardless of extension. Only works within allowed directories.
read_media_file - Read an image or audio file. Returns the base64 encoded data and MIME type. Only works within allowed directories.
```

Structured JSON output via `-o json` for agent consumption:

```bash
aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --list-tools -o json | jq '.[0] | {name, description}'
```

```output
{
  "name": "read_file",
  "description": "Read the complete contents of a file as text. DEPRECATED: Use read_text_file instead."
}
```

## Tool Inspection

Inspect a single tool's full schema with `--tool-info`:

```bash
aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --tool-info list_directory
```

```output
{
  "name": "list_directory",
  "description": "Get a detailed listing of all files and directories in a specified path. Results clearly distinguish between files and directories with [FILE] and [DIR] prefixes. This tool is essential for understanding directory structure and finding specific files within a directory. Only works within allowed directories.",
  "parameters": {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "type": "object",
    "properties": {
      "path": {
        "type": "string"
      }
    },
    "required": [
      "path"
    ]
  }
}
```

## Phase 3C: Tool Execution

Call tools directly with `--call` and `--json` arguments:

```bash
aichat --mcp-server 'npx -y @modelcontextprotocol/server-filesystem /tmp' --call read_file --json '{"path": "/private/tmp/mcp_demo.txt"}'
```

```output
Phase 3 MCP test content

```

## Error Handling

Clear error messages for connection failures:

```bash
aichat --mcp-server 'nonexistent-binary' --list-tools 2>&1 || true
```

```output
Error: Could not start MCP server "nonexistent-binary": No such file or directory (os error 2)
hint: ensure the server binary is installed and on PATH
```

## Schema Caching

Tool discovery results are cached with a 1-hour TTL. After the first `--list-tools` call, subsequent calls return instantly from cache without spawning a server process.

## Phase 3D: Config-Based Servers

MCP servers can be defined in `config.yaml` for persistent, named access. Tools from configured servers are automatically loaded and available for tool-calling via the LLM loop:

    mcp_servers:
      github:
        command: npx
        args: ["@modelcontextprotocol/server-github"]
        env:
          GITHUB_TOKEN: "${GITHUB_TOKEN}"
      filesystem:
        command: npx
        args: ["@modelcontextprotocol/server-filesystem", "/home/user"]

MCP tools are namespaced by server name (e.g., `github:create_issue`, `filesystem:read_file`) to avoid collisions with local tools. Wildcard patterns like `github:*` are supported in `use_tools` and `mapping_tools`:

    use_tools: github:*,fs_cat
    mapping_tools:
      github: "github:*"
