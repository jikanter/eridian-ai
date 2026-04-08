# Phase 28: Entity Evolution — Agent Composability

**Status:** Planned
**Epic:** 10 — Entity Evolution & Agent Dynamism
**Design:** [epic-10.md](../analysis/epic-10.md)

---

> **[ADDED 2026-03-16]** Makes agents first-class composable components in AIChat's pipeline/tool/macro
> system. Strategy: compete on agent composability (leveraging existing strengths), not agent autonomy
> (LangGraph/CrewAI territory). Full design: [`docs/analysis/epic-10.md`](../analysis/epic-10.md)

| Item | Status | Notes |
|---|---|---|
| 28A. Agent-as-tool | — | Agents callable through `ToolCall::eval()` dispatch. When tool name matches a known agent, init agent, run `call_react`, return output as `ToolResult`. Recursion prevention via `depth` parameter (max 3). Token isolation: sub-agent gets its own context window. Extend `tool_search` to include agents in discoverable index. ~150 lines. |
| 28B. Configurable react loop | — | Expose `react_max_steps:` in role/agent frontmatter (default 10). Add synthetic `finish` tool for explicit clean termination. ~40 lines. |
| 28C. Macro output chaining | — | `%%` variable in macro steps resolves to previous step's output. Reads `config.last_message` between steps. ~20 lines. |

> **Moved to Epic 6 Phase 19:** Unified entity resolution (→19B), agent-in-pipeline (→19C), agent MCP binding (→19D).

**Parallelization:** All 3 items are independent. 28A is the largest.

**Key files:** `src/function.rs` (28A), `src/client/common.rs` (28A depth + 28B steps), `src/config/role.rs` (28B), `src/config/mod.rs` (28C).
