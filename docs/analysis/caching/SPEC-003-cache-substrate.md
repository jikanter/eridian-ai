# SPEC-003: Cache/Cassette/Mock Substrate

**Version:** 0.1
**Status:** Draft, ready for implementation per [`PLAN-cache-substrate.md`](PLAN-cache-substrate.md)
**Amended 2026-06-02:** repo topology updated to match shipped reality — substrate is its
own repo (`astrophage`), `replay-core` is a member of *that* repo, aichat consumes it by
cross-repo git dep (SPEC-astrophage §2.1 decision A). §1, §9 reflect this; binary renamed
`eridian-replay` → `astrophage`.
**Owners:** project lead
**Inputs:** [`ADR-0005`](ADR-0005-cache-substrate-extraction.md), [`SPEC-001`](SPEC-001-trace-format.md),
[`EVAL-0004`](EVAL-0004-litellm-cache-parity.md), `src/serve.rs` (37D), `src/cache.rs`,
`docs/roadmap/phase-37-overview.md` (`transparent_key`), `docs/roadmap/phase-38-overview.md`
(`CacheBackend`, `CacheControl`)

This is the **contract** for the wire-level substrate decided in `ADR-0005`. It specifies
behavior, not implementation. Downstream consumers — promptfoo replay, the pi bridge,
Eridian's own `base_url` targeting — depend on this being stable. It is a *projection* of
the keystone trace (`SPEC-001`), not a new telemetry surface.

## 0. The boundary, restated `TODO(ACE-id)`

The substrate owns exactly **one** of Eridian's three cache families: the **wire-level
response cache**, keyed on the **canonicalized request body**. It does **not** own, and
**must never** be taught:

- the structure-aware `StageCache` key `(role, model, input)` (`stages` prefix; pipeline
  stages, `.cache/knowledge`) — stays in Eridian (`ADR-0005` §1);
- provider prompt caching / L3 `cache_control` emission — a request-builder property that
  stays in Eridian (`ADR-0005` §1);
- knowledge `FactId`s or any retrieval identity — `retrieve.rs` is synchronous and
  network-free; it produces the prompt the substrate later sees and is invisible to the
  substrate by construction (§3.4 of the kickoff).

The two stores **may share a storage mechanism** (the `replay-core` CAS + SSE crate) but
**not keying or semantics.** Pushing a structure-aware key into the substrate re-imports
runtime-awareness into the one component whose value is being runtime-agnostic — a category
error and a migration to undo, not a refactor.

## 1. Binary & repo shape

**Topology decided** — SPEC-astrophage §2/§2.1 **decision A**, realized on disk 2026-06-02.
The substrate is **its own repo** ([`astrophage`](https://github.com/jikanter/astrophage)),
**not** a member of aichat's workspace. `replay-core` is a workspace member **of the
astrophage repo**. aichat consumes `replay-core` as a **cross-repo git dependency** — the
only build coupling (the runtime coupling is `base_url` + the correlation header, §6). The
binary's `ADR-0005` working name `eridian-replay` shipped as **`astrophage`**.

```text
astrophage/                     # github.com/jikanter/astrophage (own repo + workspace)
├── Cargo.toml                  # [workspace] members=["crates/replay-core"]; bin: astrophage
├── src/main.rs                 # substrate binary: {gateway, policy, control, trace, cassette}
└── crates/replay-core/         # shared crate — IN-REPO (decision A)
    └── src/{cas.rs, sse.rs, key.rs, lib.rs}

aichat/  (this repo)
└── Cargo.toml                  # replay-core = { git = ".../astrophage", rev|branch|tag = … }
```

- **`astrophage`** (working name `eridian-replay` in `ADR-0005`) — the standalone
  reverse-gateway binary, in its own repo. CLI via `clap` (consistent with `aichat` and
  `brief`; **not** `argc` — see [`SPEC-004`](SPEC-004-ecosystem-surfaces.md) §argc). No new
  programming languages.
- **`replay-core`** — the shared library crate holding the **content-addressed store** and
  **SSE-synthesis**, a workspace member **of the astrophage repo**, depended on by *both*
  the `astrophage` binary (in-repo path dep) and `aichat`'s in-tree `StageCache`/serve path
  (cross-repo git dep). This is the "shared `cas` crate" of `ADR-0005`. `StageCache`
  (`src/cache.rs`) refactors to sit on `replay-core`'s CAS primitive so atomic-write and
  content-addressing exist once. **Dependency-arrow cost** (SPEC-astrophage §2.1): aichat
  now build-depends on the astrophage repo; `base_url` stays the only *runtime* coupling, so
  the seam remains reversible.
- **Dependencies.** Default build adds **zero** new default dependencies — reuse `sha2`,
  `parking_lot`, `serde_json`, and the existing HTTP stack (`axum`/`hyper`/`reqwest`)
  already in `aichat`. Anything beyond (Redis, object stores — Phase 39, deferred) is
  cargo-gated, per [`EVAL-0004`](EVAL-0004-litellm-cache-parity.md) delta #2.

The substrate reuses 37D's `serve.rs` HTTP plumbing and the 38A `CacheBackend` trait. The
**new** code is: request canonicalization, the determinism gate, the CAS store wiring, SSE
synthesis, the control protocol, the policy selector, and trace projection.

## 2. Request canonicalization & key

The cache key is a **runtime-agnostic, HTTP-computable** digest of the request body. It
**reuses and extends** Phase 37C's `transparent_key`
(`docs/roadmap/phase-37-overview.md` §37C). `TODO(ACE-id)`

### In-key fields

The canonical key hashes, null-delimited (collision-safe per `StageCache::key`), the
fields that determine the response:

| Field | Source in OpenAI-shape body | Notes |
|---|---|---|
| model id | `model` | resolved/normalized (provider alias → canonical) |
| messages | `messages[]` | full list, post-template-expansion, role+content |
| system | `system` or `messages[0]` if role=system | provider-normalized |
| sampling | `temperature`, `top_p`, `max_tokens`, `stop`, `frequency_penalty`, `presence_penalty`, `seed` | present even when default — affects output |
| tools | `tools[]` / `functions[]` | schema presence affects model behavior even if unused |
| response format / schema | `response_format`, `json_schema` | structured-output determinant |

### Normalized-OUT (not in key)

`stream` flag, request IDs, `X-Eridian-Session-Id` and all correlation/determinism
headers, `Authorization`/`X-Api-Key` (stripped before hashing, per `SPEC-001` §6),
`user`/telemetry fields, and object-key ordering (JSON is canonicalized to sorted keys,
normalized whitespace).

Key form: `sha256(null_delimited_fields)`, optionally namespaced as `"{namespace}:{hash}"`
(38D). **Open field decisions** (e.g., whether provider-specific optional params are
in-key) are governed by 37C's `cache_include_provider_params` flag (`EVAL-0004` §2.6); any
field decision *not already settled by 37C* is a stop-and-ask item per the kickoff §6, not
a guess.

### Determinism gate `TODO(ACE-id)`

A request is **cacheable** only when its result is reproducible. The gate (lifted from
37C/`pipe.rs`, not re-derived):

1. `temperature == 0` (or provider-deterministic per `Model::is_deterministic_at`);
2. no tools were invoked (tool side effects make replay unsound — see
   [`SPEC-004`](SPEC-004-ecosystem-surfaces.md) §llm-functions for the tool-replay split);
3. policy permits write (not `no-store`).

Eridian signals determinism to the substrate via request metadata (§6): temperature/seed/
top_p → a `cacheable` hint header. The substrate **also** independently inspects the body
so a non-Eridian client gets correct gating.

## 3. The three policies as one mechanism `TODO(ACE-id)`

One CAS+SSE machine; three policies differing only in keying, eviction, and fault rules. A
request selects its policy via the control protocol (§4) — header (server), flag (CLI), or
the launch mode of the binary.

| Policy | Keying | Eviction | Writes | Faults | Use |
|---|---|---|---|---|---|
| **cache** | canonical key (§2) | TTL + LRU (byte budget) | auto, on miss | none | transparent cost reduction (37C/37D parity) |
| **cassette** | canonical key (§2) | **none** (pinned, content-addressed) | only in explicit *record* mode | none | eval-replay: deterministic, offline, token-free |
| **mock** | matcher (path/method/key/ordinal) | n/a | n/a | scripted status/latency/body | cross-process fault injection |

- **cache** — auto-keyed TTL/LRU, exactly the 37C transparent + 37D server cache behavior,
  now served from the substrate. Misses fall through to the real provider (`base_url`
  upstream); hits replay from the CAS.
- **cassette** — a **pinned, content-addressed set** with **no eviction**. Two sub-modes:
  - *record*: every request is sent upstream, the response is stored, and the cassette set
    grows. Secrets redacted before write (§7).
  - *replay*: requests are answered **only** from the pinned set; a request whose canonical
    key is absent is a **miss-as-error** (configurable: hard-fail for CI determinism, or
    pass-through for record-extend). This is the eval-replay payload.
- **mock** — answers are scripted faults (HTTP status, latency, malformed/partial body,
  mid-stream disconnect) selected by a matcher, for **cross-process, end-to-end** fault
  injection when a real downstream tool points at the substrate. This does **not** replace
  in-process `wiremock-rs` (`SPEC-004` §wiremock).

A single binary launch fixes a default policy (`--mode cache|cassette|mock`); per-request
control (§4) can override read/write behavior within that policy.

## 4. Control protocol `TODO(ACE-id)`

One protocol, idiomatic at the server, mirrored as `aichat` CLI flags and role frontmatter.
It **reuses 38D's `CacheControl` vocabulary** (`docs/roadmap/phase-38-overview.md` §38D)
and extends it with a mode selector + correlation metadata.

### HTTP server surface (the substrate is pointed at by `base_url`)

Standard `Cache-Control` request-header directives plus `x-aichat-*` extensions:

| Directive | Semantics (LiteLLM `DynamicCacheControl` parity) |
|---|---|
| `no-cache` | skip read, still write |
| `no-store` | skip write, read allowed |
| `max-age=<s>` | per-request entry TTL |
| `s-maxage=<s>` | reject reads older than N seconds (freshness gate) |
| `x-aichat-cache-namespace: <ns>` | key partition |
| `x-aichat-cache-mode: cache\|cassette\|mock` | per-request policy selector (within launch default) |
| `x-aichat-cassette: <set-name>` | select cassette set for this request |
| `x-eridian-session-id: <ulid>` | correlation to the originating turn (§6) |
| `x-aichat-cacheable: 0\|1` | determinism hint from the client (§2, §6) |

`use-cache` (opt-in under `mode=default_off`) is implied by the launch `--mode` / a
`x-aichat-cache-mode` override.

### CLI surface (mirrors the server)

`--cache-ttl`, `--cache-namespace`, `--no-transparent-cache` (= `no-cache`),
`--cache-no-store`, `--cache-mode`, `--cassette <name>` — exactly the 38D `--cache-*`
flag family extended with `--cache-mode`/`--cassette`. See
[`SPEC-004`](SPEC-004-ecosystem-surfaces.md) §Eridian.

### Role frontmatter

```yaml
cache: { ttl: 3600, namespace: "faq", mode: cassette, cassette: "rust-reviewer-v1" }
```

## 5. SSE synthesis `TODO(ACE-id)`

Ports LiteLLM `caching.py:446-459` (`EVAL-0004` §2.10), implemented once in
`replay-core::sse`:

- **Replay**: a stored single-body response is replayed as a **synthesized SSE stream**
  when the request set `stream: true`. Chunking is configurable (chunk size / inter-chunk
  delay; default a small realistic delay).
- **Record**: a streamed upstream response is buffered into a single canonical body, stored
  once, and (on later replay) re-streamed. This is the 37D replay-from-buffer strategy,
  factored into the shared crate so the in-process server (37D) and the substrate share it.
- A non-streaming request replays a stored body verbatim.

The buffered-canonical body is what the canonical key and the cassette store address, so a
streaming and a non-streaming request with the same canonical key hit the **same** entry.

## 6. Trace projection `TODO(ACE-id)`

The substrate's accounting **is a projection of `SPEC-001`**, not a parallel telemetry
model (the F1 anti-fragmentation rule; a foreign-process event model that *cannot* be a
keystone projection is the failure being avoided).

- **Correlation.** Eridian injects `X-Eridian-Session-Id` (the turn's ULID) on every
  outbound request. The substrate reads it and stamps every event it emits with that
  `session_id`/`parent_session_id`, so a substrate hit correlates to the originating turn.
  This header is **legitimate here** because we own the client (contrast `EVAL-001` §3,
  which rejected it only for a third-party MITM). `TODO(ACE-id)`
- **Events emitted/fed.** A cache/cassette hit or miss projects to the 37E
  `cache.lookup` event `{layer, outcome: hit|miss|write, key_hash, tokens_saved,
  cost_saved, ttl_remaining_secs}` and sets `cache_hit: true` on the corresponding
  `provider.response`/`chat.response` event so replays are never mined as fresh model
  outputs (the training-contamination hazard, `EVAL-0002` §6). A mock fault projects to a
  `provider.response` with the scripted status and/or a `provider.retry`/`error` as the
  client classifies it.
- **Emission path.** The substrate writes events through the same `SPEC-001` JSONL + blob
  store contract (or hands them to Eridian's trace writer when in-process), so accounting
  is *derivable from* the keystone trace, never a second source of truth.
- **Redaction.** The substrate holds the live `Authorization` header (it forwards
  upstream); it **MUST** redact per `SPEC-001` §6 before any byte hits disk — strip
  `Authorization`/`X-Api-Key` before hashing and before storing request bodies, and apply
  the `*_API_KEY`/`sk-`/`Bearer ` pattern rules. A cassette set is committed to git; a
  leaked key in a cassette is a security incident, so redaction is a record-mode gate, not
  a later pass.

## 7. Cassette format & lifecycle `TODO(ACE-id)`

### On-disk layout

```text
cassettes/<set-name>/
├── manifest.json          # set name, schema_version, created/updated ts, entry index
├── <canonical-key>.json   # one entry per pinned request
└── blobs/<sha256>         # large bodies, content-addressed (shared replay-core CAS)
```

Each entry records: the canonical key (§2), the **redacted** request body (or its blob
hash), the response body (or blob hash), `stream` flag, stored `CallMetrics` (tokens/cost,
for replayed `usage`), and a `timestamp` (for `s-maxage` age checks, 38D). The entry format
is a strict superset of a `CacheEntry` (38A) so cache and cassette share the value type.

### Naming & selection

A cassette set is named (`rust-reviewer-v1`) and selected by `--cassette` /
`x-aichat-cassette` / role frontmatter / promptfoo provider config
([`SPEC-004`](SPEC-004-ecosystem-surfaces.md) §promptfoo). Sets are committed to the
consuming repo (e.g., `tests/regression/cassettes/`), not to `aichat`'s source tree.

### Record → review → commit workflow

1. **Record**: run the eval/regression suite against a live provider in
   `--mode cassette` record sub-mode; entries accumulate.
2. **Review**: a human (or `showboat extract` spot-check) inspects new entries — confirms
   redaction, sanity-checks responses. `astrophage cassette diff` shows added/changed
   entries.
3. **Commit**: pin the set into the repo. CI runs `--mode cassette` replay (hard-fail on
   miss) — deterministic, offline, token-free.

### Drift detection `TODO(ACE-id)`

A **recorded request whose canonical key no longer matches** (a role edit, a prompt-
template change, a model bump changed the request body) is **drift**. In replay mode this
surfaces as a miss-as-error naming the absent key; `astrophage cassette check` reports,
for a set + a live request stream, which pinned entries went unmatched (stale) and which
requests had no pin (uncovered). Drift is the signal that a cassette set needs re-recording
— it is a feature, not a failure: it catches "the prompt changed but the eval still passed
against a stale recording."

## 8. Non-goals `TODO(ACE-id)`

- **No TLS MITM / CA interception.** The substrate is a reverse gateway pointed at by
  `base_url`, not a forward proxy. mitmproxy's only value-add is irrelevant here and its
  CA-trust cost is pure liability (`ADR-0005` Alt 2, `EVAL-0005`).
- **No rate-limiting.** Dropped, per `EVAL-0004` delta #4 — a cache is a cache; the
  LiteLLM rate-limiter helpers (`increment`/`sadd`/pipeline-increment) are out of scope.
- **Does not own structure-aware or knowledge keys** (§0, `ADR-0005` §1). `StageCache` and
  L3 stay in Eridian.
- **Does not model knowledge serving as a backend.** `retrieve.rs` emits no HTTP request;
  it is invisible to the substrate by construction (§3.4 kickoff).
- **Does not replace in-process `wiremock-rs`.** The mock leg is cross-process,
  end-to-end fault injection; `wiremock-rs` stays for time-mockable
  (`tokio::time::pause()`) Rust control-flow tests
  ([`SPEC-004`](SPEC-004-ecosystem-surfaces.md) §wiremock, `SPEC-002` §3).
- **Distributed / remote backends (Phases 39–41)** are deferred; the `CacheBackend` (38A)
  seam leaves room for them but `SPEC-003` does not specify them.

## 9. Acceptance criteria for SPEC-003 v0.1

1. `astrophage` builds in its **own repo's** workspace (not aichat's); a default
   `cargo build` of `replay-core` adds zero new default dependencies to aichat.
2. `replay-core` holds the CAS + SSE + canonical-key code; `src/cache.rs` `StageCache`
   sits on it (consumed as a **cross-repo git dependency**) with its two existing callers
   unchanged.
3. The canonical key reuses 37C `transparent_key`'s in-key/normalized-out split; a test
   varies each in-key field one at a time and asserts a distinct key, and varies each
   normalized-out field and asserts an *identical* key.
4. A cassette recorded against a live provider replays byte-identically offline with
   `cache_hit: true` on the response event and a `cache.lookup` event correlated to the
   turn `session_id`.
5. A streaming request and a non-streaming request with the same canonical key resolve to
   the same stored entry; SSE replay produces a well-formed stream.
6. No cassette entry contains a plaintext `Authorization`/`X-Api-Key` value or a string
   matching the `SPEC-001` §6 key patterns.
7. Drift: editing a role's prompt changes the canonical key; replay against the old
   cassette reports the stale/uncovered entries rather than silently passing.
8. The substrate never reads `StageCache`'s `(role, model, input)` key or any `FactId`
   (enforced by construction — those types are not in `astrophage`'s dependency set).
