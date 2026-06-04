# Phase 46 — Cassette Policy & Eval-Replay Loop : Overview — Epic 16 (Astrophage Substrate)

**Status:** Planned (new — 2026-06-04 refresh) · **Owner:** astrophage + aichat (tool-replay) · **Horizon:** Next

> **Goal.** The committed-recording workflow that makes evals **deterministic, offline, and
> token-free**. A cassette is a *cultured strain* — a pinned, content-addressed colony of stored
> responses you replay an eval against for free. This phase adds the
> **record → review → commit → CI-replay** lifecycle, **drift detection**, and the aichat-side
> **deterministic tool-replay** that completes end-to-end determinism: a deterministic eval =
> astrophage **wire-replay** + aichat **tool-replay**.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 46A | **Cassette record/replay** — record mode grows a pinned set (no eviction); replay **hard-fails on miss**; redaction-at-record (no plaintext key ever hits disk) | Planned (astrophage) |
| 46B | **`astrophage cassette check` / `diff`** — drift detection: **stale** (unmatched pins) + **uncovered** (unpinned requests) against a live request stream; a role/prompt/model edit changes the canonical key and surfaces as **miss-as-error** | Planned (astrophage) |
| 46C | **aichat deterministic tool-replay** — replay tool stdout from the keystone-trace blob store keyed `(tool_name, args_hash)`; **resolves the [`SPEC-astrophage §9.2`](../architecture/integrated-architecture/SPEC-astrophage.md) key-stability open question** (llm-functions tool-dispatch seam) | Planned (aichat ↔ llm-functions) |
| 46D | **CI eval-replay** — committed cassettes replay byte-identically offline; drift fails the build naming the stale entry | Planned (cross) |

## Cross-repo seams

- Cassettes are **portable artifacts committed to the consuming repo** (like `mcp.json`),
  replayed by astrophage — astrophage is the engine, never the home.
- **Tool-replay stays aichat-side.** astrophage caches the **wire** only, never tool execution
  ([`SPEC-004 §llm-functions`](../analysis/caching/SPEC-004-ecosystem-surfaces.md)). 46C settles
  whether `(tool_name, args_hash)` is a stable *lookup* key across runs — the `STOP-AND-ASK`
  flagged in [`SPEC-astrophage §9.2`](../architecture/integrated-architecture/SPEC-astrophage.md).

## Dependencies

- **Upstream:** Phase 45 (cache gateway + `replay-core`) + Phase 42 (trace blob store, for tool-replay).
- **Feeds:** Phase 48 (brief binding), Phase 43D (CI eval-replay).
- **Realizes:** [`SPEC-003 §3/§7`](../analysis/caching/SPEC-003-cache-substrate.md) (cassette policy) · [`SPEC-astrophage §4/§7`](../architecture/integrated-architecture/SPEC-astrophage.md).

## Acceptance criteria

1. A cassette recorded against a live provider **replays byte-identically offline** with `cache_hit:true` + a correlated `cache.lookup`.
2. Editing a bound role's prompt changes the canonical key and **CI replay fails naming the stale entry** (drift is a feature, not a silent pass).
3. **No committed cassette entry contains a plaintext key.**
4. A deterministic eval runs end-to-end: astrophage wire-replay **+** aichat tool-replay, **zero** tokens spent.

## Grounding docs

[`SPEC-003-cache-substrate.md`](../analysis/caching/SPEC-003-cache-substrate.md) (§3/§7) ·
[`SPEC-astrophage.md`](../architecture/integrated-architecture/SPEC-astrophage.md) (§4/§7/§9.2) ·
[`SPEC-004`](../analysis/caching/SPEC-004-ecosystem-surfaces.md) (§llm-functions) ·
[`EVAL-0003-tool-call-caching.md`](../analysis/caching/EVAL-0003-tool-call-caching.md)
