# Phase 5: Remote MCP & Token-Efficient Discovery

**Status:** Done

---

| Item | Status | Commit | Notes |
|---|---|---|---|
| 5A. Remote MCP servers (HTTP/SSE) | Done | — | `endpoint:` + `headers:` fields on `McpServerConfig`. Streamable HTTP transport via `ReqwestClient` adapter (avoids request version conflict). CLI auto-detects `http://`/`https://` URLs. |
| 5B. Lazy role discovery via MCP | Done | — | `discover_roles` meta-tool advertised when ≥8 tools. Full schemas injected on first use with `notifications/tools/list_changed`. Falls back to eager loading for small tool sets. |

**5A implementation notes:**
- `McpServerConfig.endpoint` is mutually exclusive with `command`. When set, uses rmcp's Streamable HTTP client transport.
- `McpServerConfig.headers` supports `${VAR}` env resolution (same as `env`). `Authorization` headers are extracted as bearer tokens.
- `ReqwestClient` wrapper in `src/mcp_client/streamable_http.rs` implements `StreamableHttpClient` for reqwest 0.12, avoiding the version conflict with rmcp's built-in reqwest 0.13 support.
- Connection retry/backoff handled by rmcp's `ExponentialBackoff` default.

**5B implementation notes:**
- Lazy mode activates when tool count ≥ `LAZY_DISCOVERY_THRESHOLD` (8). Below that, all tools served eagerly.
- `ServerCapabilities` advertises `list_changed: true` only in lazy mode.
- On `discover_roles` call: returns compact one-line-per-tool index (~30 tokens/tool vs ~121 tokens/schema).
- On first call to any tool: schema added to advertised set, `tools/list_changed` notification sent. Non-fatal if client ignores it.

**Token budget comparison** (from [tool analysis](../analysis/2026-03-10-tool-analysis.md)):

| Method | Per-turn cost | 20-turn session (30 roles) |
|---|---|---|
| Native MCP (all schemas) | ~3,630 tokens | 72,600 |
| aichat `discover_roles` + on-demand expansion | ~67 + ~121/used tool | ~1,940 (for 5 unique tools) |
| aichat `--list-roles` + on-demand `--describe` | ~67 + ~180/used role | ~1,940 (for 5 unique roles) |
