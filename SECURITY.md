# Security

## Reporting vulnerabilities

If you discover a security vulnerability, please report it privately by
opening a GitHub Security Advisory on this repository. Do **not** file a
public issue.

## Trust model

AIChat executes on the user's machine with the user's privileges. The
primary trust boundary is the **configuration file** (`config.yaml`) and
any **role definitions** loaded at startup.

### MCP server commands

MCP (Model Context Protocol) server entries in `config.yaml` may specify
a `command` that AIChat spawns as a child process:

```yaml
mcp_servers:
  my-server:
    command: npx -y @my/mcp-server
    env:
      API_KEY: xxx
```

**These commands run with your full shell permissions.** Only add MCP
server entries from sources you trust. Importing a third-party config or
role that contains a malicious `command` value can execute arbitrary code
on your system.

### Tool / function execution

Tools declared via `functions.json` or through MCP servers are invoked
when the LLM requests them. Review tool definitions before enabling them,
especially tools that run shell commands or access the filesystem.

### Environment variable access in templates

Prompt templates support `{{$VAR}}` syntax to embed environment
variables. To prevent accidental leakage of secrets (API keys, tokens,
credentials) into prompts sent to LLM providers, **only variables with
the `AICHAT_` prefix are resolved**. All other `{{$VAR}}` references are
left unexpanded.

If you need to expose a non-AICHAT variable in a template, create an
alias:

```sh
export AICHAT_MY_VALUE="$MY_VALUE"
```

### Local API server (`aichat --serve`)

The built-in HTTP server sets CORS headers that restrict cross-origin
requests to localhost origins only. This prevents arbitrary websites from
making requests to your local API. The server is intended for local
development; do not expose it to untrusted networks without additional
access controls.
