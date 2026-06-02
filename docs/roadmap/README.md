# Roadmap directory index

This directory holds the per-phase design docs. The **authoritative roadmap** — vision,
horizons (Now/Next/Later), themes, owning-repo tags, and the status ledger — lives one level up
in [`../ROADMAP.md`](../ROADMAP.md). Start there. This README is just the file map.

Cross-repo (integrated) requirements live in
[`../architecture/integrated-architecture/`](../architecture/integrated-architecture/), **not**
here — see that README for what qualifies.

## Meta docs

| File | Purpose |
|---|---|
| [`dependencies.md`](dependencies.md) | Cross-epic dependency graph + critical path. |
| [`success-metrics.md`](success-metrics.md) | Per-epic targets the roadmap commits to. |
| [`anti-roadmap.md`](anti-roadmap.md) | Proposals considered and rejected, with reasons. |
| [`REFRESH-NOTES.md`](REFRESH-NOTES.md) | Notes from the 2026-06-02 tri-repo roadmap refresh. |

## Phase docs (current)

Each phase has an `-overview.md`; some carry a longer companion design doc. Status is in the
[ledger](../ROADMAP.md#status-ledger).

| Phase | Overview | Companion detail |
|---|---|---|
| 9 Schema fidelity | [phase-9-overview.md](phase-9-overview.md) | [phase-9-schema-fidelity.md](phase-9-schema-fidelity.md) |
| 10 Resilience & retry | [phase-10-overview.md](phase-10-overview.md) | [phase-10-resilience.md](phase-10-resilience.md) |
| 11 Context budget | [phase-11-overview.md](phase-11-overview.md) | [phase-11-context-budget.md](phase-11-context-budget.md) |
| 12 Discoverability | [phase-12-overview.md](phase-12-overview.md) | — |
| 13 Authoring & teaching | [phase-13-overview.md](phase-13-overview.md) | — |
| 14 Capability manifests | [phase-14-overview.md](phase-14-overview.md) | — |
| 15 Contract testing | [phase-15-overview.md](phase-15-overview.md) | — |
| 16 Server hardening | [phase-16-overview.md](phase-16-overview.md) | [phase-16-server-hardening.md](phase-16-server-hardening.md) |
| 17 Role & pipeline execution | [phase-17-overview.md](phase-17-overview.md) | [phase-17-server-execution.md](phase-17-server-execution.md) |
| 18 Discovery & estimation *(deferred)* | [phase-18-overview.md](phase-18-overview.md) | [phase-18-server-discovery.md](phase-18-server-discovery.md) |
| 19 RoleResolver | [phase-19-overview.md](phase-19-overview.md) | — |
| 20 Remote & federated | [phase-20-overview.md](phase-20-overview.md) | — |
| 21 DAG primitives | [phase-21-overview.md](phase-21-overview.md) | — |
| 22 DAG observability & budget | [phase-22-overview.md](phase-22-overview.md) | — |
| 23 Role evaluation | [phase-23-overview.md](phase-23-overview.md) | — |
| 24 Regression & distillation *(planned)* | [phase-24-overview.md](phase-24-overview.md) | — |
| 25 Knowledge compilation | — | [phase-25-knowledge-compilation.md](phase-25-knowledge-compilation.md) |
| 26 Knowledge query | — | [phase-26-knowledge-query.md](phase-26-knowledge-query.md) |
| 27 Knowledge evolution | — | [phase-27-knowledge-evolution.md](phase-27-knowledge-evolution.md) |
| 28 Agent composability *(planned)* | [phase-28-overview.md](phase-28-overview.md) | [phase-28-agent-composability.md](phase-28-agent-composability.md) |
| 29 Agent dynamism *(planned)* | [phase-29-overview.md](phase-29-overview.md) | [phase-29-agent-dynamism.md](phase-29-agent-dynamism.md) |
| 30 Macro compilation | — | [phase-30-macro-compilation.md](phase-30-macro-compilation.md) |
| 31 Bridge retirement | [phase-31-overview.md](phase-31-overview.md) | [phase-31-bridge-retirement.md](phase-31-bridge-retirement.md) |
| 33 Typed input surface | [phase-33-overview.md](phase-33-overview.md) | — |
| 34 Auto-memory wiring | [phase-34-overview.md](phase-34-overview.md) | [phase-34-auto-memory.md](phase-34-auto-memory.md) |
| 35 Knowledge-MCP protocol *(planned)* | [phase-35-overview.md](phase-35-overview.md) | [phase-35-knowledge-mcp.md](phase-35-knowledge-mcp.md) |
| 36 Pipeline stage config isolation | [phase-36-overview.md](phase-36-overview.md) | [phase-36-implementation-plan.md](phase-36-implementation-plan.md) |
| 37 Transparent response caching *(in flight)* | [phase-37-overview.md](phase-37-overview.md) | [phase-37-response-caching.md](phase-37-response-caching.md) |
| 38 Cache backend abstraction *(planned)* | [phase-38-overview.md](phase-38-overview.md) | — |
| 39 Remote cache backends *(planned)* | [phase-39-overview.md](phase-39-overview.md) | — |
| 40 Embedding/rerank caching *(planned)* | [phase-40-overview.md](phase-40-overview.md) | — |
| 41 Cache observability & admin *(planned)* | [phase-41-overview.md](phase-41-overview.md) | — |

> Phase 32 (Pi as REPL Surface) is documented under [`../features/repl-pi.md`](../features/repl-pi.md),
> not here. Phase 8 (data processing & observability) lives in
> [`phase-8-data-observability.md`](phase-8-data-observability.md) and is **active** — see the note in the ledger.

## Archived (superseded — see [`archive/`](archive/))

Tombstones. Moved out of the active roadmap on 2026-06-02; nothing deleted.

| Archived file | Why archived |
|---|---|
| `initial-phased-roadmap.md` | Original 2026-03-10 flat plan (pre-renumber). Superseded by `../ROADMAP.md` + per-phase docs. |
| `phase-0-prerequisites.md` … `phase-7-error-messages.md` | Epic 1 foundation, all **Done**. Summarized in [`completed-epics.md`](archive/completed-epics.md). |
| `phase-31.md` | 3-line redirect stub; superseded by `phase-31-overview.md` + `phase-31-bridge-retirement.md`. |

_Epic 1's `phase-8-data-observability.md` was **not** archived — it backs active in-progress work._
