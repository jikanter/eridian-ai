# Roadmap directory index

This directory holds the **live** per-phase design docs — planned, in-flight, active, and
deferred phases only. The **authoritative roadmap** — vision, horizons (Now/Next/Later), themes,
owning-repo tags, and the status ledger — lives one level up in [`../ROADMAP.md`](../ROADMAP.md).
Start there.

**Shipped (Done) phases are archived** under [`archive/`](archive/), indexed by the comprehensive
[`archive/completed-epics.md`](archive/completed-epics.md). Cross-repo (integrated) requirements
live in [`../architecture/integrated-architecture/`](../architecture/integrated-architecture/),
**not** here.

## Meta docs

| File | Purpose |
|---|---|
| [`dependencies.md`](dependencies.md) | Cross-epic dependency graph + critical path. |
| [`success-metrics.md`](success-metrics.md) | Per-epic targets the roadmap commits to. |
| [`anti-roadmap.md`](anti-roadmap.md) | Proposals considered and rejected, with reasons. |
| [`REFRESH-NOTES.md`](REFRESH-NOTES.md) | Notes from the 2026-06-04 next-year refresh. |

## Live phase docs

Status is in the [ledger](../ROADMAP.md#status-ledger).

### Active (Epic 1, in main worktree)

| Phase | Overview |
|---|---|
| 8 Data processing & observability | [phase-8-data-observability.md](phase-8-data-observability.md) |

### Planned — finishing committed work

| Phase | Overview | Companion detail |
|---|---|---|
| 24 Regression & distillation | [phase-24-overview.md](phase-24-overview.md) | — |
| 28 Agent composability | [phase-28-overview.md](phase-28-overview.md) | [phase-28-agent-composability.md](phase-28-agent-composability.md) |
| 29 Agent dynamism | [phase-29-overview.md](phase-29-overview.md) | [phase-29-agent-dynamism.md](phase-29-agent-dynamism.md) |
| 35 Knowledge-MCP protocol | [phase-35-overview.md](phase-35-overview.md) | [phase-35-knowledge-mcp.md](phase-35-knowledge-mcp.md) |
| 37 Transparent response caching | [phase-37-overview.md](phase-37-overview.md) | [phase-37-response-caching.md](phase-37-response-caching.md) |
| 38 Cache backend trait & control protocol | [phase-38-overview.md](phase-38-overview.md) | — |
| 39 Remote cache backends | [phase-39-overview.md](phase-39-overview.md) | — |
| 40 Embedding/rerank caching | [phase-40-overview.md](phase-40-overview.md) | — |
| 41 Cache observability & admin | [phase-41-overview.md](phase-41-overview.md) | — |

### Planned — next-year frontier (2026-06 refresh)

| Phase | Epic | Overview |
|---|---|---|
| 42 Trace emission (SPEC-001) | 15 Observability Keystone | [phase-42-overview.md](phase-42-overview.md) |
| 43 Test harness (SPEC-002) | 15 Observability Keystone | [phase-43-overview.md](phase-43-overview.md) |
| 44 Trace projections & training extraction | 15 Observability Keystone | [phase-44-overview.md](phase-44-overview.md) |
| 45 Astrophage MVP: replay-core + cache gateway | 16 Astrophage Substrate | [phase-45-overview.md](phase-45-overview.md) |
| 46 Cassette policy & eval-replay loop | 16 Astrophage Substrate | [phase-46-overview.md](phase-46-overview.md) |
| 47 Mock policy & fault injection | 16 Astrophage Substrate | [phase-47-overview.md](phase-47-overview.md) |
| 48 brief companion: cassette bindings | 16 Astrophage Substrate | [phase-48-overview.md](phase-48-overview.md) |
| 49 Agent memory federation | 10 Entity Evolution | [phase-49-overview.md](phase-49-overview.md) |
| 50 Knowledge-as-cassette / federated KB | 17 Federation & Scale | [phase-50-overview.md](phase-50-overview.md) |
| 51 Vendor model extensions | 17 Federation & Scale | [phase-51-overview.md](phase-51-overview.md) |

### Deferred

| Phase | Overview | Companion detail |
|---|---|---|
| 18 Discovery & estimation *(deferred 2026-04-17)* | [phase-18-overview.md](phase-18-overview.md) | [phase-18-server-discovery.md](phase-18-server-discovery.md) |

> Phase 32 (Pi as REPL Surface) shipped and is documented as a feature:
> [`../features/repl-pi.md`](../features/repl-pi.md).

## Archived (shipped — see [`archive/`](archive/))

All **Done** phase docs moved to `archive/` on the 2026-06-04 refresh; nothing was deleted. The
comprehensive ledger is [`archive/completed-epics.md`](archive/completed-epics.md).

| Archived docs | Why archived |
|---|---|
| `phase-0-*` … `phase-7-error-messages.md`, `initial-phased-roadmap.md`, `phase-31.md` | Epic 1 foundation + pre-renumber plan (archived 2026-06-02). |
| `phase-9-*`, `phase-10-*`, `phase-11-*`, `phase-12-overview`, `phase-13-overview`, `phase-14-overview`, `phase-15-overview`, `phase-16-*`, `phase-17-*` | Epics 2–5 shipped phases. |
| `phase-19-overview`, `phase-20-overview`, `phase-21-overview`, `phase-22-overview`, `phase-23-overview` | Epics 6–8 shipped phases. |
| `phase-25-knowledge-compilation`, `phase-26-knowledge-query`, `phase-27-knowledge-evolution` | Epic 9 (Knowledge Evolution) shipped. |
| `phase-30-macro-compilation`, `phase-31-overview`, `phase-31-bridge-retirement`, `phase-33-overview`, `phase-34-*`, `phase-36-*` | Epics 11–14 + DAG/typed-input shipped phases. |

_Internal `../` links inside the newly-archived docs were re-calibrated `+1` level for their new
location; the older frozen foundation docs (`phase-0-7`) keep their original (historical) links._
