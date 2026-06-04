# Phase 47 — Mock Policy & Cross-Process Fault Injection : Overview — Epic 16 (Astrophage Substrate)

**Status:** Planned (new — 2026-06-04 refresh) · **Owner:** astrophage (aichat seam) · **Horizon:** Next

> **Goal.** Scripted fault injection for **any** downstream binary — status codes, latency,
> malformed bodies, mid-stream disconnects — matched by request shape. The **mock** policy is the
> third face of the one CAS+SSE mechanism (cache / cassette / mock). It is the **cross-process
> complement** to the in-process wiremock-rs harness (Phase 43B): it lets aichat's resilience
> path (Phase 10 retry / fallback / timeout) be exercised end-to-end across the process boundary.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 47A | **Matcher engine** — match requests by path / method / canonical-key / ordinal | Planned (astrophage) |
| 47B | **Fault scripting** — scripted status / latency / body / disconnect responses | Planned (astrophage) |
| 47C | **aichat resilience integration** — exercise the Phase 10 retry budget, model fallback, and per-call timeout cross-process against scripted faults | Planned (aichat) |

## Cross-repo seams

- Mock is policy #3 of the substrate; it **does not replace** in-process `wiremock-rs`
  (Phase 43B) — it is the cross-process surface for binaries that can't be mocked in-process.
- Selected via the same control protocol as cache/cassette (`--cache-mode mock`,
  `x-aichat-cache-mode` header) — no new aichat architecture.

## Dependencies

- **Upstream:** Phase 45 (gateway + `replay-core`).
- **Sibling:** Phase 43B (in-process control-flow harness).
- **Realizes:** [`SPEC-003 §3`](../analysis/caching/SPEC-003-cache-substrate.md) (three policies) · [`SPEC-astrophage §6`](../architecture/integrated-architecture/SPEC-astrophage.md).

## Acceptance criteria

1. A scripted `429`-then-`200` drives aichat's retry path deterministically cross-process.
2. A mid-stream disconnect surfaces as the correct typed error / semantic exit code.
3. Latency injection triggers the per-call timeout ([`ToolTimeout`](../../src/utils/exit_code.rs)).

## Grounding docs

[`SPEC-003-cache-substrate.md`](../analysis/caching/SPEC-003-cache-substrate.md) (§3) ·
[`SPEC-astrophage.md`](../architecture/integrated-architecture/SPEC-astrophage.md) (§6) ·
[`EVAL-001-compare-to-mitmproxy.md`](../analysis/caching/EVAL-001-compare-to-mitmproxy.md) ·
[`archive/phase-10-overview.md`](archive/phase-10-overview.md) (resilience & retry)
