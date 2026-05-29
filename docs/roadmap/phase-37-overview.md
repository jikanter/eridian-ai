# Phase 37: Transparent Response Caching : Overview - Epic 2

**Status (2026-05-27):** **Planned — design draft.** No items below are implemented. Closes the gap inventoried in [`docs/analysis/open-harness/EVAL-0002-full-caching.md`](../analysis/open-harness/EVAL-0002-full-caching.md) — aichat ships a partial L1 (`src/cache.rs`'s `StageCache`, scoped to pipeline stages and per-file knowledge extraction) and no L2/L3/L4. This phase wires response caching across the ordinary request path, the OpenAI-compatible server (which is the pi REPL substrate), and the trace, in the C→B→A→D ordering EVAL-0002 prescribes. Aligns with the three-pattern spec (exact / semantic / proxy) and adds the layer (L3 provider prompt caching) the spec omitted but the project's cost-conscious constraint mandates.

| Item | Description | Status |
|---|---|---|
| 37A | Cache accounting — extend [`CallMetrics`](../../src/client/common.rs) (`src/client/common.rs:340`) with `cache_read_tokens` / `cache_write_tokens`; teach Claude/OpenAI/Gemini extractors to populate; price in [`compute_cost`](../../src/client/common.rs) (`src/client/common.rs:362`). Lands first — nothing below is measurable without it. | Planned (design draft) |
| 37B | Provider prompt caching (L3) — emit Anthropic `cache_control` breakpoints in [`claude_build_chat_completions_body`](../../src/client/claude.rs) (`src/client/claude.rs:180`); prefix-stability audit of [`src/config/input.rs`](../../src/config/input.rs) so volatile content sits *after* stable blocks (earns OpenAI/Gemini implicit caching too). | Planned (design draft) |
| 37C | L1 exact cache on the ordinary path — wire `StageCache` into [`call_chat_completions`](../../src/client/common.rs) (`src/client/common.rs:610`) / [`call_react`](../../src/client/common.rs) (`src/client/common.rs:475`) behind a **new opt-in flag `--transparent-cache`** ([`src/cli.rs`](../../src/cli.rs)). `--no-cache` semantics stay scoped to pipeline (unchanged). | Planned (design draft) |
| 37D | Server response cache + pi-bridge surface — bounded LRU in [`src/serve.rs`](../../src/serve.rs) keyed on canonicalized request body; new `/v1/cache/*` endpoints; pi extension slash commands (`/cache-stats`, `/cache-clear`, `/transparent-cache`). | Planned (design draft) |
| 37E | Trace `cache.lookup` event — `{layer, outcome, key_hash, tokens_saved, cost_saved}`; schema_version bump coordinated with the open-harness trace workstream. | Planned (design draft) |
| 37F | L2 semantic cache (opt-in per role) — reuse the HNSW+BM25 store in [`src/rag/`](../../src/rag/mod.rs); cosine threshold per role; `cache: semantic` in role frontmatter. Ships after A–E are measured. | Planned (design draft) |

## Mapping to the 3-pattern spec

| Spec pattern | Phase 37 sub-phase | Notes |
|---|---|---|
| Exact match (sqlite/lru) | **37C** (ordinary path) + **37D** (server) | Built primitive (`StageCache`) is reused; key must broaden beyond `(role, model, input)` to include resolved system prompt, full message list, sampling params, tool set, output schema. |
| Semantic (ChromaDB/GPTCache) | **37F** | Substrate already in tree — no new vector DB. Opt-in per role to bound correctness risk. |
| Proxy gateway (LiteLLM/Redis) | **37D** | The pi REPL already routes through the in-process server via `AICHAT_BRIDGE_URL` ([`src/repl/pi.rs`](../../src/repl/pi.rs)); turning `serve.rs` into an L1-at-gateway is the pi integration. |
| *(not in spec — added)* Provider prompt cache (L3) | **37B** | Largest single cost win (90% read discount on Anthropic; 50% on OpenAI; up to 90% on Gemini 2.5). Self-contained to the client layer. |

## The caching sub-track (37 → 41)

Phase 37 builds the *layers* of response caching against the concrete
[`StageCache`](../../src/cache.rs). A feature-for-feature study of
[LiteLLM's caching subsystem](https://github.com/BerriAI/litellm/tree/main/litellm/caching) —
the most complete open-source treatment of the problem — surfaced a set of *horizontal
abstractions* that cut across these layers and are worth porting. That study and its
traceability map live in
[`EVAL-0004-litellm-cache-parity.md`](../analysis/open-harness/EVAL-0004-litellm-cache-parity.md)
(97% feature coverage; the distributed third gated behind cargo features). The result is a
cohesive **caching sub-track within Epic 2**:

| Phase | Scope | LiteLLM analogue |
|---|---|---|
| **37** (this doc) | L1 exact / L2 semantic / L3 provider layers, accounting, trace, pi integration | the layered model + `cache_control` emission |
| [**38**](phase-38-overview.md) | `CacheBackend` trait, in-memory/disk/dual backends, cache-control protocol, modes | `BaseCache`, `InMemoryCache`, `DiskCache`, `DualCache`, `DynamicCacheControl` |
| [**39**](phase-39-overview.md) | Distributed/remote backends (Redis/S3/GCS/Azure), **cargo-gated** | `RedisCache`, `S3Cache`, `GCSCache`, `AzureBlobCache` |
| [**40**](phase-40-overview.md) | Embedding & rerank caching (RAG) | embedding caching + `supported_call_types` |
| [**41**](phase-41-overview.md) | Admin & observability surface (ping/health/delete, model-group, CLI) | `/cache/ping`, `delete_cache_keys`, `caching_groups`, `enable/disable_cache` |

37 lands first because nothing is a pluggable backend until 38's trait exists, and nothing
is measurable until 37A's accounting and 37E's trace event exist. The two abstractions 37
itself defers — a backend trait and distributed storage — are 38A and 39 respectively, with
the "what 37 does not solve" notes below updated to point at them.

## 37A Design — Cache accounting

`CallMetrics` ([`src/client/common.rs:340`](../../src/client/common.rs)) today carries `input_tokens` / `output_tokens` only. The Claude streaming extractor silently drops `cache_creation_input_tokens` and `cache_read_input_tokens`; OpenAI drops `prompt_tokens_details.cached_tokens`; Gemini drops `cachedContentTokenCount`. The `cacheRead` / `cacheWrite` keys in [`src/config/session.rs:1128`](../../src/config/session.rs) are hard-coded zeros in the pi-export shim — a placeholder, not a measurement.

```rust
// Sketch — actual change lives in src/client/common.rs
pub struct CallMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,    // NEW (37A)
    pub cache_write_tokens: u64,   // NEW (37A) — Anthropic only; 0 for OpenAI/Gemini
    pub cost_usd: f64,
    pub model_id: String,
}
```

`compute_cost` ([`src/client/common.rs:362`](../../src/client/common.rs)) becomes:

```
cost = (input - cache_read - cache_write) * input_price
     + cache_read  * input_price * provider.read_multiplier   // 0.1 Anthropic, 0.5 OpenAI, 0.1 Gemini
     + cache_write * input_price * provider.write_multiplier  // 1.25 Anthropic 5m, 2.0 Anthropic 1h
     + output * output_price
```

**Files:** [`src/client/common.rs`](../../src/client/common.rs), [`src/client/claude.rs`](../../src/client/claude.rs), [`src/client/openai_compatible.rs`](../../src/client/openai_compatible.rs), [`src/client/gemini.rs`](../../src/client/gemini.rs), [`src/client/model.rs`](../../src/client/model.rs) (cache-price multipliers per model).

## 37B Design — Provider prompt caching (L3)

Two sub-gaps, paired:

- **B1 — Anthropic `cache_control` breakpoint.** [`claude_build_chat_completions_body`](../../src/client/claude.rs) (`src/client/claude.rs:180`) attaches `cache_control: {type: "ephemeral"}` to the last block of `system` (and optionally the last `tool` definition). The stable prefix for aichat is `tools` + `system` (role body, MCP tool schemas, knowledge context) — exactly the large, reused content. Gate on prompt size (a breakpoint below the provider minimum is wasted) and on the model advertising `prompt_cache: true` in `Model`.
- **B2 — prefix-stability audit.** Automatic caching on OpenAI/Gemini and explicit caching on Anthropic both require volatile content (timestamps, per-turn RAG retrieval, dynamic agent `_instructions`) to sit **after** stable content. Audit and reorder [`src/config/input.rs`](../../src/config/input.rs) message assembly so the stable `system + tools` prefix is byte-stable across turns within a session.

This sub-phase **does not introduce a flag** — L3 caching is on whenever the model supports it. It is a cost reduction with no correctness risk (the provider's own KV-cache replay is mathematically identical to a fresh prefill).

## 37C Design — L1 exact cache + `--transparent-cache` flag

Per project convention, **`--no-cache` semantics are not broadened**. The existing flag stays scoped to pipeline (declared at [`src/cli.rs:197`](../../src/cli.rs) with `requires = "pipe"`). A new flag opts in to L1 caching on the ordinary path:

```rust
// Sketch — actual addition lives in src/cli.rs alongside no_cache at line 197
/// Replay deterministic prior responses from a local cache.
/// Hit when (model, system, messages, sampling, tools, schema) match exactly,
/// temperature == 0, no tools were used, and the entry is within TTL.
/// Mutually exclusive with --no-cache; orthogonal to pipeline stage caching.
#[clap(long = "transparent-cache")]
pub transparent_cache: bool,
```

Companion config (read in [`src/config/mod.rs`](../../src/config/mod.rs)):

```yaml
# config.yaml — optional defaults
transparent_cache: true           # opt in by default for this user
transparent_cache_ttl_secs: 3600
transparent_cache_max_bytes: 524288000   # 500 MiB; LRU eviction past this
```

Role frontmatter may opt **out** per role (a non-deterministic role like a creative-writing prompt):

```yaml
---
name: brainstorm
cache: false   # never serve from transparent-cache for this role
---
```

### Key broadening

`StageCache::key(role, model, input)` ([`src/cache.rs:32`](../../src/cache.rs)) is too narrow for a general turn. The cache key for 37C must hash everything that determines the response:

```rust
// Sketch — extends src/cache.rs
pub fn transparent_key(
    model_id: &str,
    resolved_system: &str,
    messages: &[Message],          // serialized JSON, post-template-expansion
    sampling: &SamplingParams,     // temperature, top_p, max_tokens
    tools: &[ToolSchema],           // even if unused — schema presence affects model behavior
    output_schema: Option<&Value>,
) -> String { ... }
```

### Determinism gating

Cache lookup happens at the top of `call_chat_completions` / `call_react`; cache write happens after a successful response. Gates (all must hold):

1. `--transparent-cache` flag is set (or config default is `true`).
2. Role-level `cache:` is not `false`.
3. `temperature == 0` (or provider-deterministic; see `Model::is_deterministic_at(temp)`).
4. No tools were invoked during the turn (`ToolResult::is_empty()`). Tool-using turns mutate the world — replay is unsound.
5. Not a dry-run.
6. Response is non-streaming, or streaming-replay-from-buffer is implemented (see Phase 37D for the server analog).

### Atomic writes + LRU eviction

`StageCache::put` is currently a plain `fs::write` ([`src/cache.rs:52`](../../src/cache.rs)) — two processes writing the same key can interleave. 37C upgrades to write-temp-then-rename. Disk-budget eviction (default 500 MiB) runs on `put` when the directory exceeds budget; oldest mtime wins. TTL stays as today's backstop.

**Files:** [`src/cli.rs`](../../src/cli.rs) (new flag), [`src/cache.rs`](../../src/cache.rs) (`transparent_key` + atomic write + LRU), [`src/client/common.rs`](../../src/client/common.rs) (`call_chat_completions` / `call_react` lookup + write), [`src/config/mod.rs`](../../src/config/mod.rs) (config field), [`src/config/role.rs`](../../src/config/role.rs) (`cache: false` parsing).

## 37D Design — Server response cache + pi-bridge surface

The OpenAI-compatible server in [`src/serve.rs:978`](../../src/serve.rs) (`chat_completions`) proxies straight to the upstream client with no response cache and explicit `Cache-Control: no-cache` headers ([`src/serve.rs:453, 714, 1182`](../../src/serve.rs)). It is the highest-leverage cache in the codebase because:

1. The pi REPL bridge runs against an in-process server ([`src/repl/pi.rs`](../../src/repl/pi.rs)) — every pi turn flows through `chat_completions`.
2. Any downstream tool (Claude Code, Cursor, a script) pointed at the server inherits the cache for free.
3. The deterministic-request question is answerable inside the server: `temperature`, `stream`, and tool presence are all in the request body.

### Cache layer

A bounded in-memory LRU (default 128 entries) backed by an on-disk LRU (reuses 37C's `cache.rs` infrastructure) sits in front of `chat_completions`. Key is the canonicalized request body (sorted keys, normalized whitespace, model id resolved). Same determinism gates as 37C (5 conditions; tool presence inferred from the `tools` field in the OpenAI-shape request).

### `Cache-Control` semantics

The server currently sends `Cache-Control: no-cache` on responses — that header was a pass-through hint to *clients*, not a description of the server's behavior, and it stays. The cache layer is internal to the server; a hit still returns a freshly-sent response body with `cached: true` in the `usage` block:

```json
{
  "usage": {
    "prompt_tokens": 842,
    "completion_tokens": 156,
    "cached_tokens": 842,           // NEW — surfaces the hit to API consumers
    "cost_usd": 0.0,                // hit costs nothing
    "cache_hit": true               // x-aichat extension
  }
}
```

### Streaming carve-out

Streaming responses (`stream: true`) follow one of two strategies (decide in implementation PR):

- **Carve-out:** never cache streaming responses. Simple, no replay infrastructure, loses the cache for pi's default mode.
- **Replay-from-buffer:** buffer the streamed response into a single body server-side, cache the buffered body, and on hit replay the body as a synthesized SSE stream with realistic chunking. Wins pi but adds a buffering layer.

Recommendation: replay-from-buffer. Pi is the dominant caller and streams by default; carving streaming out defeats the integration.

### New `/v1/cache/*` endpoints

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/v1/cache/stats` | Hit count, miss count, tokens saved, $ saved, current cache size, oldest entry mtime. |
| `POST` | `/v1/cache/clear` | Drop all entries. Optional `{ model: "claude-sonnet-4-6" }` body to scope. |
| `POST` | `/v1/cache/disable` | Per-session disable; idempotent. Bridge-token-authenticated like other `/v1/state/*` routes. |
| `POST` | `/v1/cache/enable` | Re-enable. |

These mirror the existing `/v1/state/*` shape ([`src/serve.rs`](../../src/serve.rs) `chat_completions_via_role` and the bridge auth at [`src/serve.rs:246`](../../src/serve.rs)) — same bearer-token gating.

### Pi extension surface

[`pi-extensions/src/index.ts`](../../pi-extensions/src/index.ts) gains four slash commands wrapping the endpoints above:

- `/cache-stats` — `GET /v1/cache/stats`, render hits/misses/$ saved inline.
- `/cache-clear` — `POST /v1/cache/clear`.
- `/transparent-cache off` / `/transparent-cache on` — `POST /v1/cache/disable` / `POST /v1/cache/enable`.
- `/info` (existing) extended to surface running savings: "saved $0.04 via cache this session."

The pattern is identical to the existing `/role`, `/agent`, `/macro`, `/rag`, `/aichat-session` commands: each command translates to an HTTP call via `bridgeFetch` ([`pi-extensions/src/index.ts:28`](../../pi-extensions/src/index.ts)).

**Files:** [`src/serve.rs`](../../src/serve.rs) (cache layer in `chat_completions` + 4 new routes), [`src/cache.rs`](../../src/cache.rs) (shared LRU; reused from 37C), [`pi-extensions/src/index.ts`](../../pi-extensions/src/index.ts) (new slash commands), [`assets/pi-extensions/aichat-bridge.js`](../../assets/pi-extensions/aichat-bridge.js) (built artifact regenerated from source).

## 37E Design — Trace `cache.lookup` event

A cache hit currently emits a `debug!` log only ([`src/pipe.rs:425`](../../src/pipe.rs)). The open-harness trace schema (SPEC-001, 13 event types) has no cache event — a turn-elimination path that ships invisible to the harness is exactly the failure mode ADR-0001 was written to prevent. Add:

```jsonl
{"event": "cache.lookup", "schema_version": "0.2", "ts": "2026-05-27T19:14:22Z",
 "session_id": "...", "layer": "L1", "outcome": "hit",
 "key_hash": "sha256:abc...", "tokens_saved": 842, "cost_saved": 0.012,
 "ttl_remaining_secs": 2400}
```

`layer` ∈ `{L1, L2, L3}`; `outcome` ∈ `{hit, miss, write}`. Replayed responses **must** carry a `cache_hit: true` field on the corresponding `chat.response` event so the deferred training pipeline never mines a replay as a fresh model output (training-data contamination hazard called out in EVAL-0002 §6).

Coordinated `schema_version` bump from `0.1` → `0.2` with the trace workstream — not a unilateral schema change.

**Files:** Open-harness `SPEC-001-trace-format.md` update + `src/trace/` (when that landing site exists).

## 37F Design — L2 semantic cache (opt-in per role)

Deferred until A–E are measured. Substrate exists: [`src/rag/`](../../src/rag/mod.rs) has hybrid HNSW+BM25 + an embedding pipeline. L2 reuses it — no vector DB added.

Opt-in per role:

```yaml
---
name: faq-bot
cache: semantic
cache_similarity_threshold: 0.95   # cosine; >= 0.95 → hit
---
```

Lookup order is **L1 first, L2 second** — exact match is cheaper and correctness-safe; semantic only runs on L1 miss. A semantic hit is logged as `outcome: hit, layer: L2, similarity_score: 0.97` so the trace can audit false-positive rate post-hoc.

**Files:** [`src/cache.rs`](../../src/cache.rs) (`SemanticCache` layered on `StageCache`), [`src/rag/mod.rs`](../../src/rag/mod.rs) (reuse `embed()` + nearest-neighbor), [`src/config/role.rs`](../../src/config/role.rs) (parse `cache: semantic`).

## Pi integration narrative

aichat owns inference and state; pi owns the TUI. The pi launcher ([`src/repl/pi.rs`](../../src/repl/pi.rs)) mints a localhost bridge port, stages [`assets/pi-extensions/aichat-bridge.js`](../../assets/pi-extensions/aichat-bridge.js) into `<cwd>/.pi/extensions/`, and execs `pi` with `AICHAT_BRIDGE_URL` + `AICHAT_BRIDGE_TOKEN` in the env. Every pi turn is an HTTP call to the in-process server.

That means **37D is the pi integration — every pi turn becomes cacheable for free, no extension change needed for the cache itself.** The slash commands in 37D are the *visibility* surface: they let a pi user inspect, clear, or toggle the cache. The cache itself is transparent.

This also means 37C's `--transparent-cache` flag is *both* a CLI flag and the launch-time toggle for pi (`aichat --pi-repl --transparent-cache` → pi sessions hit the cache by default). The config default (`transparent_cache: true`) flips on global opt-in once the feature is proven.

## Open questions

### 1. Default for `transparent_cache` config field

**Question:** Ship `transparent_cache: true` or `transparent_cache: false` as the default in `config.yaml`?

**Recommendation: false in 37C, flip to true in a follow-up PR after 37E has shipped two weeks.** Cost-conscious as guiding constraint argues for `true`. But shipping a cache that ate a turn before the trace can show it ate the turn is exactly the failure mode 37E exists to prevent. Sequence: 37A (visibility) → 37B (provider, zero-risk) → 37C with `false` default (opt-in) → 37D (server) → 37E (trace) → flip 37C default to `true` once trace events confirm hit rate and zero false hits over real workloads.

### 2. Should 37D's server cache key the bridge token?

**Question:** The pi bridge uses a per-launch bearer token. If two pi sessions share a process (currently they don't, but they could), should the cache key include the token?

**Recommendation: no.** The token is auth, not identity — a cache hit is a function of the request body, not who's asking. The pi launcher mints a fresh process per launch ([`src/repl/pi.rs`](../../src/repl/pi.rs)), so cross-session contamination is not currently a risk. If multi-tenant server mode ships later, revisit.

### 3. Interaction with session compression

**Question:** [`src/config/session.rs`](../../src/config/session.rs) compresses history past a token threshold. Compression rewrites the prefix, which invalidates whatever L1/L2 entries and L3 provider prefix-cache had formed. Does 37 fight 32?

**Recommendation: yes, and that is OK for now.** Compression amortizes against L4 prefix reuse, which is already weak. Documenting the tradeoff is sufficient for 37; a future phase can teach compression to amortize cache-write cost (compress on a cadence, not every turn). Flagged as a follow-up in EVAL-0002 §5 Gap G.

### 4. Phase number / epic placement

**Question:** Phase 37 under Epic 2 (Runtime Intelligence) is correct, but Epic 7 (DAG Execution) has Phase 22D ("DAG stage caching") already planned. Should 22D fold into 37?

**Recommendation: 22D stays in 22 and consumes 37's primitives.** 22D is about caching DAG *branches* (parallel-stage memoization) — different cache key shape (per-branch input) and different invalidation rules (one branch changing invalidates only that branch). 37 provides the cache substrate (atomic writes, LRU eviction, `cache.lookup` events) that 22D consumes. The phases compose; they do not collapse.

## Testing

Per project guideline ("*Always* add integration tests via bats in addition to unit tests"):

- **`tests/regression/transparent-cache.sh`** — bats regression covering:
  - 37A: `aichat --info -o json` after a turn includes `cache_read_tokens` / `cache_write_tokens` fields (zero when no caching, populated when Anthropic prompt caching fires).
  - 37B: a second identical request to Claude with `cache_control` emitted reports non-zero `cache_read_tokens` in the response.
  - 37C: `aichat --transparent-cache -r summarize <input.txt>` twice in a row — second call's wall time is <100ms and trace shows `cache.lookup outcome: hit`.
  - 37C/no-tools: same role with `use_tools: [read_file]` and an actual tool call — second call still hits the model (tool-using turns never cache).
  - 37C/temp-gating: same role with `temperature: 0.7` — second call still hits the model.
  - 37C/role-opt-out: a role with `cache: false` frontmatter — second call still hits the model.
  - 37D: `curl` two identical requests against `aichat --serve` — second response includes `usage.cached_tokens > 0` and `usage.cache_hit: true`.
  - 37D/pi-bridge: spawn `aichat --pi-repl`, send the same prompt twice via the bridge, verify the second response is from cache (via `/v1/cache/stats`).
  - 37D/slash-cache-clear: pi slash command `/cache-clear` followed by repeat request — hit count resets, request goes to model.
  - 37E: every cache hit emits exactly one `cache.lookup` trace event with the documented shape.
  - 37F (when shipped): role with `cache: semantic` and `cache_similarity_threshold: 0.95` — a paraphrase ("summarize this" vs "give me a summary") hits cache; an unrelated query does not.

- **Rust unit tests** in `src/cache.rs`:
  - `tests::transparent_key_covers_all_response_determinants` — vary each of (model, system, messages, sampling, tools, schema) one at a time; assert each variation produces a distinct key.
  - `tests::atomic_write_survives_concurrent_writers` — N threads writing the same key produce a readable result (no partial file).
  - `tests::lru_eviction_respects_budget` — fill cache past `max_bytes`; oldest entries are evicted by mtime.

- **Rust unit tests** in `src/client/common.rs`:
  - `tests::call_chat_completions_serves_from_cache_when_deterministic`
  - `tests::call_chat_completions_misses_cache_on_temperature_gt_zero`
  - `tests::call_react_never_caches_tool_using_turns`
  - `tests::compute_cost_discounts_cache_read_tokens` — 1000 cache-read tokens on Anthropic Sonnet 4.6 → 0.1× input price.

- **Rust unit tests** in `src/serve.rs`:
  - `tests::chat_completions_returns_cached_response_on_match`
  - `tests::chat_completions_streaming_replays_buffered_response_on_hit`
  - `tests::v1_cache_clear_drops_all_entries`

## Sequencing

**A → B → C → D → E**, with **F** deferred. EVAL-0002's C→B→A→D maps to **37A → 37B → 37C → 37D** here (numbering inverted to match phase-row ordering). The ordering buys the largest cost reduction earliest, keeps every change measurable, and never ships a cache the open harness cannot see.

- **37A** must land before 37B, 37C, 37D — they are unmeasurable without it.
- **37B** is independent of 37C/D; can land in parallel after 37A.
- **37C and 37D** share `src/cache.rs` infrastructure (atomic writes, LRU) — 37C lands first, 37D reuses.
- **37E** must land before 37C and 37D *ship to users* (a feature-flag carve-out is fine for the implementation PR ordering, but the user-facing flip happens with E in place).
- **37F** ships only after a measured baseline from A–E exists.

## Files (consolidated)

- [`src/cache.rs`](../../src/cache.rs) — `transparent_key`, atomic writes, LRU eviction, `SemanticCache` (37F)
- [`src/cli.rs`](../../src/cli.rs) — new `--transparent-cache` flag at line 197 alongside existing `--no-cache`
- [`src/client/common.rs`](../../src/client/common.rs) — `CallMetrics` extensions, `compute_cost` update, lookup/write in `call_chat_completions` / `call_react`
- [`src/client/claude.rs`](../../src/client/claude.rs) — `cache_control` emission in `claude_build_chat_completions_body` + cached-token extraction
- [`src/client/openai_compatible.rs`](../../src/client/openai_compatible.rs) — `prompt_tokens_details.cached_tokens` extraction
- [`src/client/gemini.rs`](../../src/client/gemini.rs) — `cachedContentTokenCount` extraction
- [`src/client/model.rs`](../../src/client/model.rs) — per-model cache-price multipliers + `prompt_cache: bool` field
- [`src/config/mod.rs`](../../src/config/mod.rs) — `transparent_cache` / `transparent_cache_ttl_secs` / `transparent_cache_max_bytes` config fields
- [`src/config/role.rs`](../../src/config/role.rs) — `cache: false | semantic` frontmatter parsing
- [`src/config/input.rs`](../../src/config/input.rs) — prefix-stability audit (37B2)
- [`src/serve.rs`](../../src/serve.rs) — cache layer + 4 new `/v1/cache/*` routes
- [`pi-extensions/src/index.ts`](../../pi-extensions/src/index.ts) — `/cache-stats`, `/cache-clear`, `/transparent-cache` slash commands
- [`assets/pi-extensions/aichat-bridge.js`](../../assets/pi-extensions/aichat-bridge.js) — rebuilt artifact
- See deep design notes: [`phase-37-response-caching.md`](phase-37-response-caching.md)

## References

- [`docs/analysis/open-harness/EVAL-0002-full-caching.md`](../analysis/open-harness/EVAL-0002-full-caching.md) — the gap inventory this phase implements
- [`docs/analysis/open-harness/EVAL-0004-litellm-cache-parity.md`](../analysis/open-harness/EVAL-0004-litellm-cache-parity.md) — the LiteLLM feature-for-feature parity map driving the 37→41 sub-track
- [Phase 38 overview](phase-38-overview.md) — `CacheBackend` trait + cache-control protocol (the abstraction 37 sits on)
- [Phase 39 overview](phase-39-overview.md) — distributed/remote backends (cargo-gated)
- [Phase 40 overview](phase-40-overview.md) — embedding/rerank caching
- [Phase 41 overview](phase-41-overview.md) — cache admin & observability surface
- [`src/cache.rs`](../../src/cache.rs) — existing `StageCache` primitive being broadened
- [`src/repl/pi.rs`](../../src/repl/pi.rs) — pi launcher; explains why 37D is the pi integration
- [`docs/features/repl-pi.md`](../features/repl-pi.md) — pi REPL surface; user-facing impact of 37D
- [Phase 22 overview](phase-22-overview.md) — sibling phase that consumes 37's substrate via 22D
- [Phase 36 overview](phase-36-overview.md) — sibling pipeline-isolation phase; shares the config-clone pattern documented at [`src/config/resolver.rs:170-185`](../../src/config/resolver.rs)
- Anthropic Prompt Caching: https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching
- OpenAI Prompt Caching: https://platform.openai.com/docs/guides/prompt-caching
- Gemini Context Caching: https://ai.google.dev/gemini-api/docs/caching
