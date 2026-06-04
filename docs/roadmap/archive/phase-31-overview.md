# Phase 31: Bridge Retirement & MCP Pool Hardening : Overview - Epic 11

> Retire the Node HTTP bridge in `llm-functions/mcp/bridge/` in favor of the
> portable `mcp.json` artifact + aichat's native `mcp_pool`. Two upstream
> aichat fixes block the retirement; this phase lands them and ships the
> portable-file loader so `config.yaml` stops being the source-of-truth for
> MCP server declarations.

| Item | Description | Status |
|---|---|---|
| 31A | `ToolCall::eval` MCP-pool routing (shared helper with `eval_tool_calls`, unskips probe-a) | **Done** |
| 31B | Multi-server pool init regression at N≥5 (diagnose runtime/IO race, unskips probe-b large-N) | **Done** |
| 31C | `mcp_servers_file:` portable loader (per [`SPEC-mcp-json-artifact.md`](../../architecture/integrated-architecture/SPEC-mcp-json-artifact.md)) | **Done** |
| 31D | Unskip gated bats tests; refresh `docs/demos/demo-mcp-server.md` known-limitation sections | **Done** |
| 31E | `aichat --validate-mcp-config [PATH]` CLI subcommand (per SPEC § Validation) | **Done** |

**Cross-cutting docs:** [`bridge-retirement.md`](../../architecture/integrated-architecture/bridge-retirement.md), [`SPEC-mcp-json-artifact.md`](../../architecture/integrated-architecture/SPEC-mcp-json-artifact.md).

**Validation gates** (must hold before retirement diff lands in `llm-functions`):

1. ✅ `bats tests/integration/mcp-server.sh` — 10/10 passing, no skips.
2. ✅ `showboat verify docs/demos/demo-mcp-server.md` — limitation sections rewritten as "fixed in Phase 31A/B/C" (§§ 4, 5, 6).
3. ✅ `aichat --mcp` against a portable `~/.config/mcp/mcp.json` can list AND invoke tools (covered by mcp-server.sh tests 5, 8, 9, 10).
4. ✅ The portable file validates against `SPEC-mcp-json-artifact.md` § Validation via `aichat --validate-mcp-config` (9 cases in `tests/integration/mcp-validate.sh`).

All four gates green. Bridge deletion in [`llm-functions`](https://github.com/jikanter/personal-llm-functions) is unblocked; that diff is out of scope for this repo (see § "What is explicitly NOT done in this phase" in [phase-31-bridge-retirement.md](./phase-31-bridge-retirement.md)).

## [Epic Details](./phase-31-bridge-retirement.md)