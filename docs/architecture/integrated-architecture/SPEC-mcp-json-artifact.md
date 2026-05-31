# SPEC: Portable `mcp.json` Artifact

**Status:** Draft, 2026-05-01
**Applies to:** aichat (this repo), [llm-functions](https://github.com/jikanter/personal-llm-functions), and any future harness that consumes MCP server declarations.
**Companion:** [`bridge-retirement.md`](bridge-retirement.md)

## Purpose

Define one **portable file** that declares the user's MCP servers. Aichat reads it. The future harness reads it. Claude Code, Cursor, and Claude Desktop can read it (or symlink to it) because the schema is bit-compatible with their conventions.

The file is the *declaration*. It is not coupled to any one runtime. Deleting aichat must not lose the user's MCP server list.

## Non-goals

- This is **not** a published specification. There is no MCP RFC for config files; the dominant convention is what Claude Desktop and Claude Code ship. We align with that convention. We do not extend it for other clients.
- This file does **not** carry runtime tuning that is private to a single consumer (timeouts, cache TTLs, connection pool sizing). Those live in the consumer's own config.
- This file does **not** replace `tools.txt` / `agents.txt` registration for first-party llm-functions tools. It only describes external MCP servers.

## Dialect

We target **Claude Code's `.mcp.json` dialect**. Reasons:

- Plain JSON. Not embedded in a larger app config.
- Cross-platform; no Mac-specific path.
- Field set has been stable: `command`, `args`, `env`, plus `url` / `headers` for HTTP transport.
- Best track record of being read by tools other than the one that defined it.

We **do not** track Claude Desktop's `claude_desktop_config.json` because that file mixes MCP entries with chat history and theme settings. Wrong scope.

## File schema

The file is a JSON object with a single top-level key, `mcpServers`. Keys under `mcpServers` are user-chosen names; values are server declarations.

```json
{
  "mcpServers": {
    "git": {
      "command": "/Users/admin/.local/bin/uvx",
      "args": ["mcp-server-git"]
    },
    "sqlite": {
      "command": "/Users/admin/.local/bin/uvx",
      "args": ["mcp-server-sqlite", "--db-path", "/path/to/db.sqlite"]
    },
    "memory": {
      "command": "/opt/homebrew/bin/node",
      "args": ["/path/to/memory/dist/index.js"],
      "env": {
        "MEMORY_FILE_PATH": "/path/to/memory.json"
      }
    },
    "remote-api": {
      "url": "https://mcp.example.com/sse",
      "headers": {
        "Authorization": "Bearer ${API_TOKEN}"
      }
    }
  }
}
```

### Field set (aligned)

Every consumer is expected to honor these:

| Field | Type | Required when | Notes |
|---|---|---|---|
| `command` | string | stdio transport | Absolute path preferred; relative resolved against caller's `PATH`. |
| `args` | string[] | stdio transport | Passed to `command` verbatim. |
| `env` | object<string,string> | optional | Process environment. `${VAR}` interpolation against caller's environment. Consumers MUST document inheritance behavior (whether parent env passes through). |
| `url` | string | HTTP/SSE transport | Mutually exclusive with `command`. |
| `headers` | object<string,string> | optional, HTTP only | Same `${VAR}` interpolation. |
| `type` | `"stdio"` \| `"sse"` \| `"http"` | optional | Hint for ambiguous cases; consumer infers from `command`/`url` when absent. |

### Extensions: `x-aichat`

aichat-specific tuning lives under a per-server `x-aichat` object. Other consumers MUST ignore unknown `x-*` keys.

```json
{
  "mcpServers": {
    "git": {
      "command": "/Users/admin/.local/bin/uvx",
      "args": ["mcp-server-git"],
      "x-aichat": {
        "namespace": "git",
        "lazyDiscover": true
      }
    }
  }
}
```

| `x-aichat` field | Purpose |
|---|---|
| `namespace` | Override the prefix used for `<namespace>:<tool>` advertisement. Default: server key. |
| `lazyDiscover` | Force or suppress lazy discovery (Phase 5B) for this server. Default: inherit from global threshold. |

Global aichat tuning (`mcpStartupTimeout`, `mcpCallTimeout`, `mcpCacheTtl`, `mcpMaxConnections`) stays in `config.yaml`, not here. Those are runtime-private to aichat.

## File location and discovery

Consumers search in this order; first hit wins:

1. Path explicitly provided by the consumer's flag/config (e.g., `--mcp-config <path>` or `mcp_servers_file:` in `config.yaml`).
2. `./mcp.json` in the current working directory (project-scoped — matches Claude Code).
3. `$XDG_CONFIG_HOME/mcp/mcp.json` (typically `~/.config/mcp/mcp.json`).
4. `~/.config/mcp/mcp.json` if `XDG_CONFIG_HOME` is unset.

A consumer that finds none of these MUST treat the MCP server set as empty. It MUST NOT fall back to scanning aichat-specific or llm-functions-specific paths — that would re-introduce cross-project filesystem coupling.

## How aichat consumes this file

Add to `config.yaml`:

```yaml
mcp_servers_file: ~/.config/mcp/mcp.json   # optional; overrides the search path
```

aichat loads the file at startup (or on `POST /v1/reload` if Phase 16E lands), normalizes each `mcpServers` entry into the existing `McpServerConfig` struct, and merges with any inline `mcp_servers:` declarations in `config.yaml`. Inline entries win on key conflict (useful for tests).

This means **`config.yaml` no longer contains MCP server declarations as the canonical source**. The earlier draft of `bridge-retirement.md` proposed copying `mcp.json` content directly into `config.yaml`; that proposal is superseded by this spec.

## How llm-functions retires its bridge

After the bridge retirement (see [`bridge-retirement.md`](bridge-retirement.md)):

1. The user copies `llm-functions/mcp.json` to `~/.config/mcp/mcp.json` once. Format conversion is a key rename if anything (`mcpServers` is already the top-level key in the bridge's `mcp.json`).
2. `llm-functions/mcp.json` is deleted from the repo. The bridge is deleted with it.
3. llm-functions stops being involved in MCP server declarations entirely. Its scope narrows back to first-party tools and agents.

Neither aichat nor llm-functions reads the other's filesystem. The portable file at `~/.config/mcp/mcp.json` is the only thing they share, and they share it by convention, not by path coupling.

## How a future harness would consume this file

The harness interface (TBD; see [`README.md`](README.md)) reads the same file via the same search order. It does not need aichat installed and does not need llm-functions installed. If the harness is the consumer of last resort, the user gets MCP tools without aichat in the picture at all.

This is the property that makes this artifact "portable": any consumer can read it, in any order of installation, without depending on any other consumer.

## Validation

A `mcp.json` file is valid if:

1. It parses as JSON.
2. It contains a top-level `mcpServers` object.
3. Each entry has either `command` (stdio) or `url` (HTTP/SSE), but not both.
4. `args` (when present) is a string array.
5. `env` and `headers` (when present) are string-to-string objects.

Aichat exposes `aichat --validate-mcp-config [PATH]` (Phase 31E) which runs these checks and exits 0 on valid, 1 on parse/schema failure, 2 when no file is found via the search order. Use `-o json` for machine-readable output. With `PATH` omitted, the same search order described under "File location and discovery" applies.

## Open questions

1. **Env interpolation semantics.** Claude Code resolves `${VAR}` against the parent process env. aichat's current `mcp_servers:` block does the same. Cursor's behavior is less documented. The spec asks consumers to document their behavior; it does not normalize.
2. **Per-project overlay.** Should `./mcp.json` *override* or *merge with* the user-level file? Claude Code overrides at the project level. Recommended default: merge, with project entries winning on key conflict. Aichat's loader implements merge.
3. **Secret handling.** `env` values in plaintext is the current convention. A `secret:` indirection (read from file, read from keychain) is out of scope for v0.1; raise as a separate spec when needed.
4. **Schema versioning.** No `version` field today. If the convention forks, we add `mcpConfigVersion: 1` and bump deliberately.

## Sources and prior art

- Claude Code project-level `.mcp.json`: [docs.claude.com/en/docs/claude-code/mcp](https://docs.claude.com/en/docs/claude-code/mcp)
- MCP wire protocol spec: [modelcontextprotocol.io](https://modelcontextprotocol.io)
- Cursor's MCP config: `~/.cursor/mcp.json`
- Existing llm-functions `mcp.json` (already uses `mcpServers`): [github.com/jikanter/personal-llm-functions](https://github.com/jikanter/personal-llm-functions)
- Existing aichat `mcp_servers:` loader: `src/mcp_client/mod.rs`