# Phase 43 — Test Harness (SPEC-002) : Overview — Epic 15 (Observability Keystone)

**Status:** Planned (new — 2026-06-04 refresh) · **Owner:** aichat (consumed by promptfoo/brief) · **Horizon:** Next

> **Goal.** Two complementary test surfaces that both consume the Phase 42 trace:
> **promptfoo** for *regression* ("does the role still produce good output?") and a **Rust +
> wiremock** harness for *control-flow* ("does retry / fallback / budget behave?"). The trace is
> the assertion target for both, so testing never forks a second data model.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 43A | promptfoo **regression provider** — invoke `aichat --role … --trace-out …` as a promptfoo `provider`; assert on output **and** trace | Planned |
| 43B | Rust **wiremock control-flow** harness — `tokio::time::pause` drives retry / fallback / timeout deterministically against a mocked provider | Planned |
| 43C | **Trace-assertion helpers** — typed readers over SPEC-001 JSONL (event presence, ordering, `cache.lookup` outcomes, cost) | Planned |
| 43D | **CI wiring** — regression + control-flow suites run fully offline (no live provider) in CI | Planned |

## Cross-repo seams

- The promptfoo provider config is exactly what brief's `## Fixtures` companion (Phase 48B)
  **emits** — brief authors the binding, this harness runs it.
- The wiremock control-flow harness (43B) is the **in-process complement** to astrophage's
  cross-process **mock** policy (Phase 47); they share the fault vocabulary, not a process.

## Dependencies

- **Upstream:** Phase 42 (trace). Realizes [`PLAN-test-harness.md`](../analysis/caching/PLAN-test-harness.md) / [`SPEC-002-test-harness.md`](../analysis/caching/SPEC-002-test-harness.md).
- **Feeds:** Phase 46D (CI eval-replay), Phase 48 (brief binding).
- **Sibling:** Phase 47 (cross-process mock).

## Acceptance criteria

1. A role regression runs **offline in CI** via promptfoo, asserting against the SPEC-001 trace.
2. A retry/fallback path is exercised **deterministically** via wiremock + `tokio::time::pause` (no wall-clock sleeps).
3. Trace-assertion helpers are unit-tested pure readers (no live model).

## Grounding docs

[`SPEC-002-test-harness.md`](../analysis/caching/SPEC-002-test-harness.md) ·
[`PLAN-test-harness.md`](../analysis/caching/PLAN-test-harness.md) ·
[`EVAL-001-compare-to-mitmproxy.md`](../analysis/caching/EVAL-001-compare-to-mitmproxy.md) ·
[`ADR-0004-demo-driven-dev.md`](../analysis/caching/ADR-0004-demo-driven-dev.md)
