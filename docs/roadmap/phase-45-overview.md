# Phase 45 — Astrophage MVP: replay-core + Cache-Policy Gateway : Overview — Epic 16 (Astrophage Substrate)

**Status:** Planned (new — 2026-06-04 refresh) · **Owner:** astrophage (aichat seam) · **Horizon:** Next

> **Goal.** Stand up the **astrophage** repo and its `replay-core` crate, and ship the **cache
> policy**: a runtime-agnostic, wire-level **reverse gateway** that aichat points `base_url` at.
> On a miss it forwards upstream and stores; on a hit it returns the stored response,
> synthesizing SSE for streaming. The coupling to aichat is exactly **`base_url` + the
> `X-Eridian-Session-Id` header** — nothing else.
>
> **Boundary (critical).** astrophage owns **only** the wire cache keyed on the *canonicalized
> request body*. The structure-aware `StageCache (role, model, input)` key, provider
> `cache_control` (L3), and any knowledge `FactId` **stay in aichat** (Epics 2/9) and are never
> pushed across the seam — pushing them re-imports runtime-awareness into the one component
> whose value is being runtime-agnostic.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 45A | **`replay-core` crate** — content-addressed store (CAS), SSE synthesis, canonical request key (SHA-256 over response-determining fields; auth / stream-flag / correlation IDs / key-ordering normalized **out**) | Planned (astrophage repo) |
| 45B | **Cache-policy reverse gateway** — OpenAI-compatible listen address, forward-on-miss to the real provider, store, TTL + LRU | Planned (astrophage repo) |
| 45C | **aichat seam** — `base_url` targeting + `X-Eridian-Session-Id` injection + `CacheBackend::Remote` variant over the Phase 38 trait (in-process vs separate process becomes one trait, one deployment choice) | Planned (aichat) |
| 45D | **Trace correlation** — astrophage emits SPEC-001 `cache.lookup` events echoing the turn ULID; accounting is *derivable from* the keystone trace (projection, never a parallel model) | Planned (cross) |

## Cross-repo seams

This is **the** aichat ↔ astrophage seam. `replay-core` lives in the **astrophage repo**
(decision A, [`SPEC-astrophage §2.1`](../architecture/integrated-architecture/SPEC-astrophage.md));
aichat **build-depends** on it by cross-repo git dep — the only inbound coupling beyond
`base_url`. Removing astrophage (pointing `base_url` back at the provider) leaves aichat fully
functional. The harness ([pi](https://pi.dev)) inherits cache/replay **by topology, for free**
(its bridge already runs against `aichat --serve`; point that client at astrophage).

## Dependencies

- **Upstream:** Phase 38A (`CacheBackend` trait, for the `Remote` variant) + Phase 42 (trace, for `cache.lookup` correlation).
- **Realizes:** [`SPEC-003`](../analysis/caching/SPEC-003-cache-substrate.md) PLAN Phases 1–2 · [`ADR-0005`](../analysis/caching/ADR-0005-cache-substrate-extraction.md) · [`EVAL-0005`](../analysis/caching/EVAL-0005-build-vs-integrate-replay.md).
- **Blocks:** Phase 46 (cassette), Phase 47 (mock).

## Acceptance criteria

1. aichat ↔ astrophage coupling is **exactly** `base_url` + `X-Eridian-Session-Id`.
2. A request misses → forwards → stores; a second identical request **hits** with `cache_hit:true` and a correlated `cache.lookup`.
3. Streaming and non-streaming forms of the same request hit the **same** stored entry.
4. **Zero aichat code** in astrophage's dependency graph (a consumer can vendor astrophage + `replay-core` without cloning aichat).

## Grounding docs

[`SPEC-003-cache-substrate.md`](../analysis/caching/SPEC-003-cache-substrate.md) ·
[`ADR-0005`](../analysis/caching/ADR-0005-cache-substrate-extraction.md) ·
[`EVAL-0005`](../analysis/caching/EVAL-0005-build-vs-integrate-replay.md) ·
[`SPEC-astrophage.md`](../architecture/integrated-architecture/SPEC-astrophage.md) (§2–3) ·
[`SPEC-004`](../analysis/caching/SPEC-004-ecosystem-surfaces.md) (§Eridian) ·
[`PLAN-cache-substrate.md`](../analysis/caching/PLAN-cache-substrate.md)
