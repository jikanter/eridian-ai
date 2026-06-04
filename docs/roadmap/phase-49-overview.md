# Phase 49 — Agent Memory Federation : Overview — Epic 10 (Entity Evolution)

**Status:** Planned (new — 2026-06-04 refresh) · **Owner:** aichat ↔ llm-functions ↔ harness · **Horizon:** Next

> **Goal.** Close the loop between **agent memory** (Phase 29B JSONL fact store), the typed
> **knowledge store**, and the cross-machine **knowledge-MCP** surface (Phase 35) so an agent's
> accumulated memory becomes **queryable, attributable, and federatable** across machines —
> rather than a per-agent local file.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 49A | **Agent-memory → `KnowledgeStore` bridge** — promote agent JSONL facts (29B) into the typed store with provenance | Planned (aichat) |
| 49B | **Remote / federated agent memory** — read/write agent memory over the Phase 35 knowledge-MCP server + remote roles (Phase 20) | Planned (aichat ↔ harness) |
| 49C | **Trace-to-memory attribution** — which turn (SPEC-001 session ULID) wrote which fact | Planned (aichat) |

## Cross-repo seams

- Agents are defined in **llm-functions**; the memory itself rides the **knowledge-MCP protocol**
  (Phase 35) and the **keystone trace** (Phase 42) for attribution.
- Writes pass the **AEVS restore-check** gate (reuse Phase 35D) — federation does not weaken the
  write-validation invariant.

## Dependencies

- **Upstream:** Phase 29B (agent memory), Phase 35 (knowledge-MCP), Phase 42 (trace, for attribution).
- **Builds on:** [`archive/phase-20-overview.md`](archive/phase-20-overview.md) (remote/federated) · [`archive/phase-27-knowledge-evolution.md`](archive/phase-27-knowledge-evolution.md) (attribution & trace).

## Acceptance criteria

1. An agent's JSONL fact is promoted into the typed store carrying the **writing turn's ULID**.
2. The fact is **queryable over the knowledge-MCP server from another machine**.
3. The write is **AEVS-gated** (Phase 35D) — an un-restorable write is rejected.

## Grounding docs

[`phase-29-agent-dynamism.md`](phase-29-agent-dynamism.md) (29B) ·
[`phase-35-overview.md`](phase-35-overview.md) ·
[`archive/phase-27-knowledge-evolution.md`](archive/phase-27-knowledge-evolution.md) ·
[`archive/phase-20-overview.md`](archive/phase-20-overview.md)
