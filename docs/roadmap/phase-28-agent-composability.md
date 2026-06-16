# Phase 28: Entity Evolution — Agent Composability

**Status:** Done (28A/28B/28C shipped 2026-06-15)
**Epic:** 10 — Entity Evolution & Agent Dynamism
**Design:** [epic-10.md](../analysis/epic-10.md)

---

> **[ADDED 2026-03-16]** Makes agents first-class composable components in AIChat's pipeline/tool/macro
> system. Strategy: compete on agent composability (leveraging existing strengths), not agent autonomy
> (LangGraph/CrewAI territory). Full design: [`docs/analysis/epic-10.md`](../analysis/epic-10.md)

| Item | Status | Notes |
|---|---|---|
| 28A. Agent-as-tool | **Done** | Agents callable through `ToolCall::eval()` dispatch (`check_agent`/`eval_agent`). A known agent name matched → cloned-config sub-agent runs `call_react` in its own context window, output returned as `ToolResult`. Recursion bounded by `agent_depth` vs `react_max_depth` (config, default 3). A real function wins a name collision (`is_agent_tool`). `select_functions` emits `agent_as_tool` declarations for agents in `use_tools`. `tool_search` agent indexing deferred (agents discoverable via `use_tools` today). |
| 28B. Configurable react loop | **Done** | `react_max_steps:` role/agent frontmatter caps the loop (fallback `MAX_REACT_STEPS` = 10). Synthetic `finish` tool injected only when `react_max_steps` is set (`maybe_inject_finish`); `call_react` terminates on it via `finish_summary`. |
| 28C. Macro output chaining | **Done** | `%%` in macro steps resolves to the previous step's AI output (`substitute_prev_output`, reads `config.last_message`); dot commands skipped. Logic was live pre-phase; now unit-tested. |

> **Moved to Epic 6 Phase 19:** Unified entity resolution (→19B), agent-in-pipeline (→19C), agent MCP binding (→19D).

**Parallelization:** All 3 items are independent. 28A is the largest.

**Key files:** `src/function.rs` (28A), `src/client/common.rs` (28A depth + 28B steps), `src/config/role.rs` (28B), `src/config/mod.rs` (28C).
