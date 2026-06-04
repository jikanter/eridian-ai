# Phase 29 — Agent Dynamism : Overview — Epic 10 (Entity Evolution)

**Status:** Planned · **Owner:** aichat ↔ llm-functions · **Horizon:** Next

> **Goal.** Give agents **composable runtime policies** and a **persistent memory**. `ReactPolicy`
> generalizes the schema-retry (Phase 9C) and model-fallback (Phase 10D) behaviors into one
> pluggable trait; agent memory adds a JSONL fact store bridged from the trace — the substrate
> Phase 49 later federates.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 29A | **`ReactPolicy` trait** — composable policies: `CostGuard`, `StagnationGuard`, `ModelEscalation` (Phase 9C schema-retry and Phase 10D fallback become special cases) | Planned |
| 29B | **Agent memory** — JSONL fact store, trace-to-memory bridging, `memory: true` in `AgentConfig` | Planned |

## Cross-repo seams

Agents are defined in **llm-functions**; the policy trait and memory store are aichat-side. 29B's
fact store is the input to **Phase 49** (agent memory federation over knowledge-MCP) and bridges
from the **keystone trace** (Phase 42).

## Dependencies & detail

- **Upstream:** Phase 28 (composability).
- **Feeds:** Phase 49 (memory federation).
- **Full design:** [`phase-29-agent-dynamism.md`](phase-29-agent-dynamism.md).
