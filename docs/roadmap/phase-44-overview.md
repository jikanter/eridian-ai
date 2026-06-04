# Phase 44 — Trace Projections & Training Extraction : Overview — Epic 15 (Observability Keystone)

**Status:** Planned (new — 2026-06-04 refresh) · **Owner:** aichat · **Horizon:** Next

> **Goal.** Realize the keystone payoff ([`ADR-0001`](../analysis/caching/ADR-0001-trace-as-keystone.md)):
> turn the SPEC-001 trace into its downstream products — an **OpenTelemetry** projection for
> observability and **SFT/DPO training-pair extraction** with contamination guards. The trace is
> the single source; every projection is *derived from* it and never becomes a parallel model
> (the anti-fragmentation rule).

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 44A | **OTel projection** — map SPEC-001 events onto OpenTelemetry semantic conventions (spans for turns / stages / tool calls) | Planned |
| 44B | **Training-pair extraction** — derive SFT/DPO pairs from traces (`prompt → chosen/rejected`) | Planned |
| 44C | **Contamination guard** — replayed responses (`cache_hit:true`, cassette hits) excluded/flagged so training never learns from its own replays | Planned |
| 44D | **Trace explorer** — CLI / Marimo view over `traces/` for inspection | Planned |

## Cross-repo seams

- The contamination guard (44C) depends on the `cache_hit` / `cache.lookup` markers stamped by
  astrophage (Phase 45D) and the in-aichat cache (Phase 37E).
- The OTel projection is consumed by **external** observability tooling — a projection, not a
  new aichat runtime dependency.

## Dependencies

- **Upstream:** Phase 42 (trace) + Phase 37E (`cache.lookup` marker).
- **Independent of:** Phase 43 (different consumer of the same trace).

## Acceptance criteria

1. Traces project to OTel spans verifiable in a collector.
2. A training-pair set is extracted from a trace corpus with **every replayed response
   flagged/excluded** (no contamination).
3. The explorer renders a single turn end-to-end (request → tool calls → stages → response).

## Grounding docs

[`ADR-0001-trace-as-keystone.md`](../analysis/caching/ADR-0001-trace-as-keystone.md) ·
[`SPEC-001-trace-format.md`](../analysis/caching/SPEC-001-trace-format.md) (§6 redaction/versioning) ·
[`ECOSYSTEM.md`](../analysis/caching/ECOSYSTEM.md)
