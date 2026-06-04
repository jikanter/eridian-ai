# Phase 28 — Agent Composability : Overview — Epic 10 (Entity Evolution)

**Status:** Planned · **Owner:** aichat ↔ llm-functions · **Horizon:** Next

> **Goal.** Make agents first-class **composable** units: callable as tools, with a configurable
> react loop and macro output-chaining. Agents are defined in
> [llm-functions](https://github.com/jikanter/personal-llm-functions), so the composability
> contract is inherently cross-repo.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 28A | **Agent-as-tool** — agents callable via `ToolCall::eval()` dispatch, with a recursion depth limit | Planned |
| 28B | **Configurable react loop** — `react_max_steps:` in frontmatter, `finish` synthetic tool | Planned |
| 28C | **Macro output chaining** — `%%` variable resolves to the previous step's output | Planned |

## Cross-repo seams

Agent definitions (`functions.json`, `_instructions`) live in **llm-functions**; aichat owns the
dispatch and react loop. Agent-as-tool composes with pipelines and macros — the topology aichat
deliberately does **not** build a multi-agent orchestration framework for (see
[`anti-roadmap.md`](anti-roadmap.md)).

## Dependencies & detail

- **Feeds:** Phase 29 (agent dynamism), Phase 49 (agent memory federation).
- **Full design:** [`phase-28-agent-composability.md`](phase-28-agent-composability.md).
