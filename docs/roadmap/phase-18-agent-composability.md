# Phase 18: Entity Evolution — Agent Composability

**Status:** Planned
**Epic:** 5 — Entity Evolution
**Design:** [epic-5.md](../analysis/epic-5.md)

---

> **[ADDED 2026-03-16]** Makes agents first-class composable components in AIChat's pipeline/tool/macro
> system. Strategy: compete on agent composability (leveraging existing strengths), not agent autonomy
> (LangGraph/CrewAI territory). Full design: [`docs/analysis/epic-5.md`](../analysis/epic-5.md)

| Item | Status | Notes |
|---|---|---|
| 18A. Agent-as-tool | — | Agents callable through `ToolCall::eval()` dispatch. When tool name matches a known agent, init agent, run `call_react`, return output as `ToolResult`. Recursion prevention via `depth` parameter (max 3). Token isolation: sub-agent gets its own context window. Extend `tool_search` to include agents in discoverable index. ~150 lines. |
| 18B. Unified entity resolution | — | `-r name` resolves against combined namespace: roles → agents → macros. `-a` and `--macro` remain as explicit overrides. New `Config::resolve_entity(name)`. ~50 lines. Zero breaking changes. |
| 18C. Configurable react loop | — | Expose `react_max_steps:` in role/agent frontmatter (default 10). Add synthetic `finish` tool for explicit clean termination. ~40 lines. |
| 18D. Agent-in-pipeline | — | Pipeline stages can reference agent names. `run_stage_inner()` falls back to `Agent::init()` → `to_role()` when role resolution fails. Agents get full capabilities (own tools, RAG) within pipeline stages. ~30 lines. |
| 18E. Agent MCP binding | — | Add `mcp_servers:` to `AgentConfig`. Synced to Role via `to_role()`. Leverages existing Phase 6C `mcp_servers` → `use_tools` expansion. ~15 lines. |

**Parallelization:** 18A is foundation (18D benefits from it). 18B, 18C, 18E are fully independent. All 5 can start in parallel since 18D only has a soft dep on 18A.

**Key files:** `src/function.rs` (18A), `src/client/common.rs` (18A depth + 18C steps), `src/config/mod.rs` (18B), `src/config/role.rs` (18C), `src/pipe.rs` (18D), `src/config/agent.rs` (18E).
