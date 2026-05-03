# Phase 31: Bridge Retirement & MCP Pool Hardening : Overview - Epic 11

> Retire the Node HTTP bridge in `llm-functions/mcp/bridge/` in favor of the
> portable `mcp.json` artifact + aichat's native `mcp_pool`. Two upstream
> aichat fixes block the retirement; this phase lands them and ships the
> portable-file loader so `config.yaml` stops being the source-of-truth for
> MCP server declarations.

| Item | Description | Status |
|---|---|---|
| 31A | `ToolCall::eval` MCP-pool routing (shared helper with `eval_tool_calls`, unskips probe-a) | -- |
| 31B | Multi-server pool init regression at N≥5 (diagnose runtime/IO race, unskips probe-b large-N) | -- |
| 31C | `mcp_servers_file:` portable loader (per [`SPEC-mcp-json-artifact.md`](../architecture/integrated-architecture/SPEC-mcp-json-artifact.md)) | -- |
| 31D | Unskip gated bats tests; refresh `docs/demos/demo-mcp-server.md` known-limitation sections | -- |
| 31E | `aichat --validate-mcp-config [PATH]` CLI subcommand (per SPEC § Validation) | -- |

**Cross-cutting docs:** [`bridge-retirement.md`](../architecture/integrated-architecture/bridge-retirement.md), [`SPEC-mcp-json-artifact.md`](../architecture/integrated-architecture/SPEC-mcp-json-artifact.md).

**Validation gates** (must hold before retirement diff lands in `llm-functions`):

1. `bats tests/integration/mcp-server.sh` — 7/7 passing, no skips.
2. `showboat verify docs/demos/demo-mcp-server.md` exits 0 with limitation sections re-purposed.
3. `aichat --mcp` against a portable `~/.config/mcp/mcp.json` can list AND invoke at least one tool from each user-configured server.
4. The portable file validates against `SPEC-mcp-json-artifact.md` § Validation.

## [Epic Details](./phase-31-bridge-retirement.md)