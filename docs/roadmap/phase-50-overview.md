# Phase 50 — Knowledge-as-Cassette / Federated KB : Overview — Epic 17 (Federation & Scale)

**Status:** Planned (new — 2026-06-04 refresh) · **Owner:** aichat (cross-repo harness edge) · **Horizon:** Later

> **Goal.** Make a compiled knowledge base a **portable, committed, content-addressed artifact** —
> the same portability rule as `mcp.json` and astrophage cassettes — so KBs can be **pinned,
> shared, and queried remotely with drift detection**. Extends remote addressing (Phase 20) and
> knowledge query (Phase 26) to cross-machine federation.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 50A | **KB export/import as a portable artifact** — manifest + content-addressed chunks, living in the consuming repo (not in aichat's source tree) | Planned (aichat) |
| 50B | **Remote KB query** — query a pinned/remote KB over the knowledge-MCP surface (extends Phases 20/26) | Planned (aichat ↔ harness) |
| 50C | **Cross-machine drift & attribution** — a KB edit changes the artifact key; stale-pin detection mirrors cassette drift (Phase 46B) | Planned (aichat) |

## Cross-repo seams

- Reuses the **portability rule** ([`SPEC-mcp-json-artifact.md`](../architecture/integrated-architecture/SPEC-mcp-json-artifact.md))
  and the **cassette drift model** (Phase 46B) — knowledge and cassettes share one
  pin-and-detect-drift discipline.
- Served over **knowledge-MCP** (Phase 35) to the harness; the harness inherits remote KBs by
  topology, like it inherits caching.

## Dependencies

- **Upstream:** Phase 35 (knowledge-MCP) + Phase 46 (cassette / drift pattern).
- **Builds on:** [`archive/phase-25-knowledge-compilation.md`](archive/phase-25-knowledge-compilation.md), [`archive/phase-26-knowledge-query.md`](archive/phase-26-knowledge-query.md), [`archive/phase-20-overview.md`](archive/phase-20-overview.md).

## Acceptance criteria

1. A KB **exports to a portable artifact** committed in a consumer repo.
2. A **second machine queries it remotely** over knowledge-MCP.
3. Editing the KB **changes the artifact key** and stale-pin detection fires (drift-as-feature).

## Grounding docs

[`SPEC-mcp-json-artifact.md`](../architecture/integrated-architecture/SPEC-mcp-json-artifact.md) ·
[`SPEC-astrophage.md`](../architecture/integrated-architecture/SPEC-astrophage.md) (§7 portability) ·
[`archive/phase-26-knowledge-query.md`](archive/phase-26-knowledge-query.md) ·
[`phase-35-overview.md`](phase-35-overview.md)
