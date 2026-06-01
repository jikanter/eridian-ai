# PLAN: Cache/Cassette/Mock Substrate

**Status:** Draft, ready for implementation
**Inputs:** [`ADR-0005`](ADR-0005-cache-substrate-extraction.md), [`SPEC-003`](SPEC-003-cache-substrate.md),
[`SPEC-004`](SPEC-004-ecosystem-surfaces.md), [`EVAL-0004`](EVAL-0004-litellm-cache-parity.md),
[`EVAL-0005`](EVAL-0005-build-vs-integrate-replay.md), `docs/roadmap/phase-37-overview.md`,
`docs/roadmap/phase-38-overview.md`

Phased, **PR-sized** plan to build the substrate of `SPEC-003`. Sequence per `ADR-0005`
§3.10: **cassette → cache → mock**, distributed/observability (Phases 39–41) **deferred**.
Cassette ships first because it is deterministic, pinned, no-eviction — it proves the
CAS+SSE substrate and is the leg **nothing off-the-shelf gives us** (`EVAL-0005` §2).

**Every implementation PR** ships a Showboat demo: `demos/<milestone>.md` via `uvx
showboat`, ≥3 `showboat exec` blocks (happy path, error path, new flags), **evergreen
output** so `showboat validate` works (root `CLAUDE.md` hard constraint). Reviewers
spot-check with `showboat extract`. REPL + batch surfaces for any user-facing addition
(root `CLAUDE.md` soft constraint).

## Relationship to the 37–41 sub-track `TODO(ACE-id)`

This plan **extracts** part of Phases 37–41 into the substrate and **leaves** the rest
in-tree. The split (`ADR-0005` §1, `SPEC-003` §0):

| 37–41 item | Disposition | Where |
|---|---|---|
| 37A cache accounting (`CallMetrics`) | **stays** (projected to trace) | Eridian + trace |
| 37B provider prompt caching (L3) | **stays** — request-builder byte ordering | Eridian |
| 37C `transparent_key` (canonical key) | **reshaped** → `replay-core::key` | shared crate |
| 37C `StageCache` atomic-write + LRU | **reshaped** → `replay-core::cas` (StageCache rides it) | shared crate; `StageCache` stays in-tree |
| `StageCache` `(role,model,input)` keying | **stays** — structure-aware, never in substrate | Eridian |
| 37D server cache + `/v1/cache/*` + SSE replay | **moves** to substrate (in-process server keeps a `ResponseCache` over the same trait) | substrate + Eridian |
| 37E `cache.lookup` trace event | **stays** (substrate emits/feeds it) | trace + substrate |
| 38A `CacheBackend` trait + `ResponseCache` front | **shared** — substrate and Eridian both implement/hold | shared boundary |
| 38B `InMemoryBackend` / `DiskBackend` | **moves** (DiskBackend = `replay-core` CAS) | substrate + shared |
| 38C `DualBackend` | **moves** | substrate |
| 38D `CacheControl` + `CacheMode` | **moves**, extended with mode/cassette selector | substrate (vocabulary shared) |
| 38E `CacheableCall` gate | **stays** (call-type gate is runtime-side) | Eridian |
| 39 Redis/S3/GCS/Azure backends | **deferred** | (cargo-gated, later) |
| 40 embeddings/rerank caching | **deferred** | (RAG-side, later) |
| 41 admin/observability parity | **deferred** | (later) |

Nothing here ships before its trace projection (37E) exists — a turn-elimination path the
harness is blind to is the failure `ADR-0001` was written to prevent (`EVAL-0002` §6).

---

## Leg 0 — Foundation (`replay-core` + workspace)

### PR-0.1 — `replay-core` crate: CAS + canonical key
- **Scope:** new `crates/replay-core/` with `cas.rs` (content-addressed store, atomic
  write-temp-then-rename, byte-budget LRU — lifted from 37C/`src/cache.rs`) and `key.rs`
  (`canonical_key`, reshaped from 37C `transparent_key`: in-key/normalized-out per
  `SPEC-003` §2). Zero new default deps (`sha2`, `parking_lot`, `serde_json`).
- **Reshapes:** 37C key + atomic-write/LRU.
- **Acceptance:** key test varies each in-key field → distinct key; varies each
  normalized-out field (stream, request-id, auth header, key order) → identical key. CAS
  round-trips; concurrent writers to one key do not corrupt. Demo:
  `demos/replay-core-cas.md`.

### PR-0.2 — `StageCache` rides `replay-core::cas`
- **Scope:** refactor `src/cache.rs` `StageCache` to sit on `replay-core`'s CAS primitive;
  **keep its `(role, model, input)` key and its two callers (`src/pipe.rs`,
  `src/knowledge/compile.rs`) unchanged.** This proves the shared *mechanism* without
  sharing *keying* (`SPEC-003` §0).
- **Leaves in-tree:** `StageCache` keying & callers.
- **Acceptance:** existing `StageCache` unit tests pass unchanged; pipeline-stage and
  knowledge-compile caching behave identically. Demo: `demos/stagecache-on-replay-core.md`.

### PR-0.3 — `eridian-replay` binary skeleton + `CacheBackend` trait reuse
- **Scope:** new `crates/eridian-replay/` workspace member; `clap` CLI skeleton
  (`--mode`, `--listen`, `--upstream`); reuse 38A `CacheBackend` trait + `ResponseCache`
  front (shared boundary). Reverse-gateway shell over `axum`/`reqwest` that forwards
  upstream with **no** cache yet (pass-through).
- **Reuses:** 37D `serve.rs` plumbing patterns, 38A trait.
- **Acceptance:** `eridian-replay --upstream <url>` forwards a chat request and returns the
  response unmodified; `--help` lists modes. Demo: `demos/eridian-replay-skeleton.md`
  (happy pass-through, missing-upstream error, `--help` flags).

---

## Leg 1 — Cassette (first; the eval-replay payload)

### PR-1.1 — Cassette store + record/replay modes
- **Scope:** `cassette.rs` — on-disk layout (`SPEC-003` §7), `--mode cassette` with
  `--cache-record` / `--cache-replay`. Record sends upstream + stores (redacted); replay
  answers only from the pinned set; miss = configurable hard-fail / pass-through.
- **Moves:** the deterministic-replay core (new; closest 37 analog is 37D SSE-from-buffer,
  used here).
- **Acceptance:** record a request against a stub upstream → entry on disk; replay offline
  → byte-identical response, **zero upstream calls**; miss in hard-fail mode → non-zero
  exit naming the absent key. Demo: `demos/cassette-record-replay.md`.

### PR-1.2 — SSE synthesis (replay-from-buffer)
- **Scope:** `replay-core::sse` — buffer a streamed upstream response into a canonical body
  on record; synthesize an SSE stream from a stored body on replay (configurable chunking).
  Streaming and non-streaming requests with the same canonical key hit the same entry.
- **Moves:** 37D replay-from-buffer (factored to shared crate; in-process 37D server reuses
  it).
- **Acceptance:** `stream: true` replay produces a well-formed SSE stream; a streaming and
  a non-streaming request with identical canonical key resolve to one entry. Demo:
  `demos/cassette-sse-replay.md`.

### PR-1.3 — Redaction gate + cassette CLI (`diff`/`check`)
- **Scope:** record-mode redaction per `SPEC-001` §6 / `SPEC-003` §6 (strip
  `Authorization`/`X-Api-Key` before hash + store; pattern rules); `eridian-replay cassette
  diff` (added/changed entries) and `cassette check` (drift: stale/uncovered entries,
  `SPEC-003` §7).
- **Reshapes:** `SPEC-001` redaction rules applied at the substrate.
- **Acceptance:** no cassette entry contains a plaintext key or a `SPEC-001` §6 pattern
  match; editing a role's prompt → drift report lists the now-stale entry. Demo:
  `demos/cassette-redaction-drift.md`.

### PR-1.4 — Trace projection (`cache.lookup` + correlation)
- **Scope:** `trace.rs` — read `X-Eridian-Session-Id`; emit/feed 37E `cache.lookup`
  `{layer, outcome, key_hash, tokens_saved, cost_saved}` and set `cache_hit: true` on the
  response event; events carry the originating turn's `session_id` (`SPEC-003` §6).
- **Stays/extends:** 37E event (substrate is now a producer); 37A `CallMetrics` replayed in
  `usage`.
- **Acceptance:** a cassette replay emits a `cache.lookup` correlated to the turn
  `session_id`, and the response event carries `cache_hit: true`. Demo:
  `demos/cassette-trace-projection.md`.

### PR-1.5 — promptfoo replay wiring (the committed payload)
- **Scope:** `tests/regression/` provider targeting `--cache-mode cassette --cassette …
  --cache-replay` (`SPEC-004` §promptfoo); `helpers/trace.js` asserts `cache_hit: true`.
  Eridian-side `base_url` targeting (`SPEC-004` §Eridian) + correlation/determinism headers.
- **Moves/adds:** Eridian `--cache-mode`/`--cassette` flags; `X-Eridian-Session-Id` /
  `X-Aichat-Cacheable` injection.
- **Acceptance:** a promptfoo suite runs fully offline against a committed cassette set,
  zero tokens, deterministic; a stale prompt surfaces as a replay miss. Demo:
  `demos/promptfoo-replay.md`.

---

## Leg 2 — Cache (transparent TTL/LRU)

### PR-2.1 — `InMemoryBackend` + `DiskBackend` over the trait
- **Scope:** 38B backends in the substrate: `InMemoryBackend` (max_entries 200,
  max_bytes_per_item 1 MiB, min-heap TTL eviction, default_ttl 600s) and `DiskBackend`
  (= `replay-core` CAS, mtime-LRU, 500 MiB budget).
- **Moves:** 38B.
- **Acceptance:** TTL expiry, count eviction, oversize reject all behave per LiteLLM
  parity; disk survives restart. Demo: `demos/cache-backends.md`.

### PR-2.2 — `DualBackend` + determinism gate + `--mode cache`
- **Scope:** 38C `DualBackend { front: InMemory, back: Disk }` write-through + read-through
  backfill; determinism gate (`SPEC-003` §2: temp==0, no-tools, write-permitted); auto-key
  on canonical key; miss → upstream, hit → replay.
- **Moves:** 38C + 37C/37D determinism gate.
- **Acceptance:** identical deterministic request hits on the second call (zero upstream);
  `temperature>0` or tool-bearing request never caches; back-tier hit backfills front.
  Demo: `demos/cache-dual-determinism.md`.

### PR-2.3 — Control protocol (`CacheControl` / `CacheMode`) — three surfaces
- **Scope:** 38D `CacheControl`/`CacheMode` in the substrate, extended with
  `x-aichat-cache-mode`/`x-aichat-cassette` (`SPEC-003` §4): HTTP `Cache-Control` directives
  at the gateway; `--cache-ttl`/`--cache-namespace`/`--no-transparent-cache`/
  `--cache-no-store`/`--cache-mode` at the CLI; `cache:` frontmatter mirror.
- **Moves:** 38D (vocabulary), extended.
- **Acceptance:** `Cache-Control: no-store, s-maxage=60` and the CLI/frontmatter
  equivalents each produce the documented read/write behavior; `s-maxage` stale entry reads
  as miss. Demo: `demos/cache-control-protocol.md`.

### PR-2.4 — pi-bridge + in-process server cache surface
- **Scope:** in-process `aichat --serve` holds a `ResponseCache` (37D) over the same trait;
  pi mode select via env/header; `/cache-mode`, `/cassette` slash commands extend the 37D
  `/cache-stats`/`/cache-clear`/`/transparent-cache` set (`SPEC-004` §pi).
- **Moves:** 37D server cache + pi surface.
- **Acceptance:** a pi session inherits the cache; `/cache-stats` shows savings;
  `/cache-mode cassette` switches a live session. Demo: `demos/pi-cache-surface.md`.

---

## Leg 3 — Mock (scripted faults)

### PR-3.1 — `--mode mock`: scripted status/latency/body faults
- **Scope:** `policy.rs` mock matcher (path/method/key/ordinal) → scripted HTTP status,
  latency, malformed/partial body, mid-stream disconnect, for **cross-process** fault
  injection (`SPEC-003` §3). Explicitly **not** a `wiremock-rs` replacement (`SPEC-004`
  §wiremock).
- **Adds:** new (no direct 37–41 analog; the cross-process counterpart to in-process
  wiremock).
- **Acceptance:** a downstream `aichat` pointed at `--mode mock` sees the scripted 502 /
  slow response / truncated stream and classifies it (trace `provider.retry`/`error`); a
  doc note states the wiremock boundary. Demo: `demos/mock-faults.md`.

---

## Deferred (not in this plan) `TODO(ACE-id)`

- **Phase 39** — Redis/Cluster/S3/GCS/Azure backends, cargo-gated behind 38A. Revisit when
  a measured hit-rate justifies a shared/remote tier (`ADR-0005` §3.10).
- **Phase 40** — embedding/rerank (RAG) caching. RAG-side, not wire-level.
- **Phase 41** — admin/observability parity (`/v1/cache/ping|health|delete`, namespaces,
  model-group coalescing, `aichat cache stats`).

These are referenced, not specified here. **No silent caps:** each deferral is logged in
the roadmap so coverage is not mistaken for completeness.

## Sequencing summary

```
Leg 0 (foundation)  →  Leg 1 (cassette)  →  Leg 2 (cache)  →  Leg 3 (mock)
   replay-core           eval-replay          TTL/LRU            faults
   StageCache rides      promptfoo offline    pi inherits        cross-process
   binary skeleton       (the commitment)     control protocol   (≠ wiremock)
                                                                  ┊
                                              Phases 39/40/41 ────┘ deferred
```

## Plan-level acceptance

- Legs land in order; each PR is independently reviewable and ships a passing Showboat
  demo (`showboat validate` green, `showboat extract` reproduces the commands).
- After Leg 1, a committed cassette set replays a promptfoo suite **offline, token-free,
  deterministically**, with `cache_hit` visible in the keystone trace — the eval-replay
  commitment realized.
- `StageCache` keying and L3 emission remain in-tree throughout; no structure-aware or
  knowledge key ever enters `eridian-replay`'s dependency set (`SPEC-003` §0, acceptance
  #8).
