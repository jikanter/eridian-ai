# EVAL-0004: LiteLLM Caching — Feature-for-Feature Parity Map

**Status:** Analysis, 2026-05-29
**Inputs:** [BerriAI/litellm `litellm/caching/`](https://github.com/BerriAI/litellm/tree/main/litellm/caching)
(`caching.py`, `base_cache.py`, `in_memory_cache.py`, `dual_cache.py`, `disk_cache.py`,
`redis_cache.py`, `redis_cluster_cache.py`, `redis_semantic_cache.py`,
`qdrant_semantic_cache.py`, `s3_cache.py`, `gcs_cache.py`, `azure_blob_cache.py`,
`caching_handler.py`), [`litellm/types/caching.py`](https://github.com/BerriAI/litellm/blob/main/litellm/types/caching.py);
aichat `src/cache.rs`, `src/client/common.rs`, `src/serve.rs`, `src/rag/mod.rs`,
`EVAL-0002-full-caching.md`, `phase-37-overview.md`.
**Question:** If we *clone and rewrite LiteLLM's caching subsystem feature-for-feature*
into aichat's Rust runtime — under the project's governing constraints
(cost-conscious, one-tool-per-job, no new default dependencies, no breaking argc /
llm-functions) — which LiteLLM feature lands where, and what changes in the port?

This is the traceability artifact behind the caching sub-track (Phases **37–41**). It
inventories the *entire* LiteLLM caching feature set, maps each feature to the aichat
phase/item that reproduces it, and records the **adaptation delta** — the deliberate
difference between LiteLLM's Python/proxy shape and aichat's Rust/CLI shape. Where a
feature is dropped or reshaped, the reason is stated. EVAL-0002 framed *what* layered
caching aichat is missing; this document answers *how a mature reference implementation
structures the same problem* and what is worth copying.

---

## 1. Why LiteLLM is the right reference, and where it stops being one

LiteLLM is the de-facto LLM gateway. Its caching subsystem is the most complete
open-source treatment of the problem: a single `Cache` front object delegating to a
`BaseCache` trait, eight interchangeable backends, an HTTP-style per-request cache
control vocabulary, two-tier read-through caching, and a uniform cache-hit accounting
path. EVAL-0002 §3 already cites it as the bar `serve.rs` is measured against.

But LiteLLM is a **Python library + hosted proxy** with very different constraints from
aichat:

- **Dependencies are cheap for it.** `pip install litellm[caching]` pulling in
  `redis`, `boto3`, `diskcache`, `redisvl`, `qdrant-client` is normal Python packaging.
  For a Rust CLI whose constraint sheet lists "Significant increase in number of
  dependencies" under *Ask First*, every backend is a deliberate cost. **Resolution:**
  in-memory + disk backends are default (zero new deps — `sha2`, `parking_lot` already
  present); Redis / object-store backends compile only under cargo features (Phase 39).
- **It is multi-tenant by default.** Namespacing, model-group caching, and per-key Redis
  TTLs exist because one proxy serves many teams. aichat is single-user-first; these
  features port as *opt-in* surfaces (namespaces for the `--serve` multi-caller case,
  Phase 41C), not defaults.
- **It caches embeddings, transcriptions, reranks, and the Responses API.** aichat's
  analogues are RAG embeddings and rerank (Phase 40); transcription/Responses have no
  aichat surface and are dropped with a note.
- **It has no provider-prompt-cache (L3) story in this module.** LiteLLM handles
  Anthropic `cache_control` in its transformation layer, not its cache module. aichat
  folds L3 into the same sub-track (Phase 37B) because the project's cost constraint
  makes it the single largest win (EVAL-0002 §5).

The LLM-engineering framing EVAL-0002 introduced — the **L1/L2/L3/L4 layered model** —
is the spine. LiteLLM contributes the **horizontal abstractions** that cut across those
layers: the backend trait, the control protocol, the two-tier composition, the
accounting uniformity. Phases 38–41 are those abstractions, ported.

---

## 2. The parity map

Legend for **Port status**: ✅ ported as-is · ◑ ported with adaptation · ⚙ cargo-gated ·
⊘ deliberately dropped (reason given). "Phase/Item" points at the owning roadmap entry.

### 2.1 Front object & lifecycle — LiteLLM `Cache` (`caching.py`)

| LiteLLM feature | Source | aichat mechanism | Phase/Item | Port |
|---|---|---|---|---|
| `Cache` front delegating to a backend | `caching.py:55` | `ResponseCache` front holding `Box<dyn CacheBackend>` | 38A | ◑ |
| `type=` backend selector (`local`/`redis`/`disk`/`s3`/…) | `caching.py:168-250` | `cache_backend:` config enum → backend factory | 38A, 39C | ◑ |
| `mode` = `default_on` / `default_off` (opt-in) | `caching.py:49,815` | `CacheMode` enum; maps to `transparent_cache` default flip | 38D | ✅ |
| `enable_cache()` / `disable_cache()` / `update_cache()` | `caching.py:856-987` | `aichat cache enable/disable` + `/v1/cache/{enable,disable}` (37D) + runtime swap | 41D | ◑ |
| `supported_call_types` allow-list | `caching.py:70-83,257` | `CacheableCall` enum gate (chat/embedding/rerank) | 38E | ◑ |
| `ping()` / `disconnect()` lifecycle | `caching.py:830-844` | `CacheBackend::ping()` / `Drop` | 38A, 41A | ✅ |

### 2.2 Backend trait — LiteLLM `BaseCache` (`base_cache.py`)

| LiteLLM feature | Source | aichat mechanism | Phase/Item | Port |
|---|---|---|---|---|
| `BaseCache` abstract: get/set + async variants | `base_cache.py:22-49` | `trait CacheBackend` (sync core; `async_trait` for remote) | 38A | ◑ |
| `get_ttl()` default-vs-override resolution | `base_cache.py:26-33` | `CacheBackend::resolve_ttl(&CacheControl)` | 38A/38D | ✅ |
| `async_set_cache_pipeline()` batch write | `base_cache.py:42` | `CacheBackend::put_batch()` (default: loop) | 38A, 40B | ◑ |
| `batch_cache_write` high-traffic buffering | `caching.py:826` | `RedisBackend` flush-buffer (`redis_flush_size`) | 39A | ⚙ |
| `test_connection()` health probe | `base_cache.py:57` | `CacheBackend::health()` → `/v1/cache/health` | 41A | ✅ |

### 2.3 In-memory backend — LiteLLM `InMemoryCache` (`in_memory_cache.py`)

| LiteLLM feature | Source | aichat mechanism | Phase/Item | Port |
|---|---|---|---|---|
| Bounded entry count (`max_size_in_memory`, default 200) | `in_memory_cache.py:30,136` | `InMemoryBackend { max_entries }` | 38B | ✅ |
| Per-item size cap (`max_size_per_item`, 1 MiB) | `in_memory_cache.py:34,52` | `max_bytes_per_item` reject-on-oversize | 38B | ✅ |
| TTL dict + min-heap expiry eviction | `in_memory_cache.py:49,105` | `BinaryHeap<(Reverse<Instant>, key)>` eviction | 38B | ✅ |
| `default_ttl` (600 s) | `in_memory_cache.py:32` | `default_ttl_secs` config | 38B/38D | ✅ |
| `flush_cache()` / `delete_cache()` | `in_memory_cache.py:266,274` | `clear()` / `delete(key)` | 38B, 41A | ✅ |
| `increment_cache` / `sadd` (rate-limit helpers) | `in_memory_cache.py:231,190` | n/a — rate limiting is not a cache concern in aichat | — | ⊘ rate-limiter is out of scope |

### 2.4 Disk backend — LiteLLM `DiskCache` (`disk_cache.py`)

| LiteLLM feature | Source | aichat mechanism | Phase/Item | Port |
|---|---|---|---|---|
| File-backed cache with TTL expiry | `disk_cache.py:14,29` | `DiskBackend` over the existing `src/cache.rs` content-addressed file store | 38B | ◑ |
| `diskcache` dependency | `disk_cache.py:17` | none — reuse `StageCache`'s `fs` + `sha2` primitive (37C atomic-write + LRU uplift) | 38B | ◑ no new dep |
| `flush_cache()` / `delete` | `disk_cache.py:86,92` | `clear()` / `delete()` on the dir | 38B | ✅ |

### 2.5 Two-tier composition — LiteLLM `DualCache` (`dual_cache.py`)

| LiteLLM feature | Source | aichat mechanism | Phase/Item | Port |
|---|---|---|---|---|
| Memory front + persistent back, write-through | `dual_cache.py:114,364` | `DualBackend { front, back }` write-through | 38C | ✅ |
| Read-through with backfill (redis→memory) | `dual_cache.py:167-177` | `DualBackend::get` backfills front on back-hit | 38C | ✅ |
| `local_only` write flag | `dual_cache.py:114` | `CacheControl::local_only` (no-store-to-back) | 38C/38D | ◑ |
| `LimitedSizeOrderedDict` LRU helper | `dual_cache.py:39` | covered by `InMemoryBackend` eviction | 38B | ✅ |
| Redis-batch access throttle (`redis_batch_cache_expiry`) | `dual_cache.py:260-300` | `DualBackend` back-read coalescing window | 39A | ⚙ |

### 2.6 Cache-key generation — LiteLLM `get_cache_key` (`caching.py:276`)

| LiteLLM feature | Source | aichat mechanism | Phase/Item | Port |
|---|---|---|---|---|
| Hash over all LLM API params | `caching.py:294-312` | `transparent_key(model, system, messages, sampling, tools, schema)` | 37C | ◑ already specified |
| SHA-256 of canonicalized key string | `caching.py:404-421` | `sha2::Sha256` over null-delimited fields | 37C | ✅ |
| Namespace prefix on key | `caching.py:423-444` | `CacheControl::namespace` → `"{ns}:{hash}"` | 38D/41C | ✅ |
| `preset_cache_key` (compute-once reuse) | `caching.py:378-402` | compute key once per turn, thread through | 37C | ◑ |
| Caching across model groups (`caching_groups`) | `caching.py:337-362` | role-alias / model-group key coalescing | 41C | ◑ |
| Provider-specific optional params (feature flag) | `caching.py:301-310` | `cache_include_provider_params` config | 38D | ✅ |
| File checksum keying (transcription) | `caching.py:364-376` | n/a — no transcription surface | — | ⊘ no surface |

### 2.7 Per-request control protocol — LiteLLM `DynamicCacheControl` (`types/caching.py:69`)

| LiteLLM control | Semantics | aichat surface | Phase/Item | Port |
|---|---|---|---|---|
| `ttl` | per-request entry lifetime | `Cache-Control: max-age=` (serve) · `--cache-ttl` (CLI) · `cache_ttl:` (role) | 38D | ✅ |
| `namespace` | key prefix / partition | `Cache-Control` ext · `--cache-namespace` · `cache_namespace:` | 38D/41C | ✅ |
| `s-maxage` / `s-max-age` | reject reads older than N s | `Cache-Control: s-maxage=` freshness gate on read | 38D | ✅ |
| `no-cache` | skip read, still write | `Cache-Control: no-cache` · `--no-transparent-cache` (one-off) | 38D | ✅ |
| `no-store` | skip write (read allowed) | `Cache-Control: no-store` | 38D | ✅ |
| `use-cache` | opt-in when `mode=default_off` | implied by `--transparent-cache` / `cache: true` | 38D | ✅ |
| stored `{timestamp, response}` for age checks | `caching.py:600,469-498` | `.meta` sidecar already in 37 layout carries `ts` | 38D | ✅ |

### 2.8 Remote / distributed backends

| LiteLLM backend | Source | aichat mechanism | Phase/Item | Port |
|---|---|---|---|---|
| `RedisCache` (host/port/pw/namespace/ttl) | `redis_cache.py` | `RedisBackend` under `cache-redis` feature (`redis` crate) | 39A | ⚙ |
| `RedisClusterCache` (startup nodes) | `redis_cluster_cache.py` | `RedisBackend` cluster mode, same feature | 39A | ⚙ |
| `S3Cache` (boto3) | `s3_cache.py` | `S3Backend` under `cache-s3` feature (`aws-sdk-s3` or S3-compat via `reqwest` SigV4) | 39B | ⚙ |
| `GCSCache` | `gcs_cache.py` | `GcsBackend` under `cache-gcs` | 39B | ⚙ |
| `AzureBlobCache` | `azure_blob_cache.py` | `AzureBlobBackend` under `cache-azure` | 39B | ⚙ |
| Connection/cred lifecycle from env | `caching.py:168-250` | `cache_backend:` config + env creds, lazy connect | 39C | ◑ |
| Shared cross-machine team cache | (proxy default) | opt-in via shared Redis/S3 backend; namespaced per team | 39D | ◑ reverses 37 "out of scope" |

### 2.9 Semantic backends — LiteLLM `RedisSemanticCache` / `QdrantSemanticCache`

| LiteLLM feature | Source | aichat mechanism | Phase/Item | Port |
|---|---|---|---|---|
| Embedding-similarity lookup | `redis_semantic_cache.py:70-118` | reuse in-tree HNSW+BM25 store (`src/rag/`), no vector DB | 37F | ◑ |
| `similarity_threshold` → distance | `redis_semantic_cache.py:83-88` | `cache_similarity_threshold:` per role | 37F | ✅ |
| Configurable embedding model | `redis_semantic_cache.py:47` | reuse aichat embedding model config | 37F | ✅ |
| External vector DB (Redis/Qdrant) | both files | none by default; in-tree HNSW. Optional Qdrant under `cache-qdrant` | 37F / 39B | ◑/⚙ |

### 2.10 Streaming, embeddings & accounting

| LiteLLM feature | Source | aichat mechanism | Phase/Item | Port |
|---|---|---|---|---|
| Synthesize SSE chunks from cached string | `caching.py:446-459` | replay-from-buffer synthesized stream | 37D | ✅ |
| Streaming-chunk delay constant | `caching.py:459` | configurable replay chunking | 37D | ◑ |
| Embedding response caching | `caching.py:735-806` | RAG `embed()` cache | 40A | ◑ |
| Per-item `prompt_tokens_details` distribution | `caching.py:692-733` | per-item token split on batch embed | 40B | ✅ |
| `cache_hit` flag on response | `caching_handler.py:200,634` | `cache_hit: true` on `chat.response` trace + `usage` | 37E, 41B | ✅ |
| `cache_key` surfaced in `_hidden_params` | `caching_handler.py:247,365` | `x-aichat-cache-key` response header | 41B | ◑ |
| Cached-token accounting | `caching_handler.py:445` | `CallMetrics.cache_read/write_tokens` | 37A | ✅ |

### 2.11 Admin & observability surface

| LiteLLM feature | Source | aichat mechanism | Phase/Item | Port |
|---|---|---|---|---|
| `/cache/ping` health endpoint | `types/caching.py:90` (`CachePingResponse`) | `GET /v1/cache/ping` | 41A | ✅ |
| `/cache/delete` by key | `caching.py:836` (`delete_cache_keys`) | `POST /v1/cache/delete {keys|namespace}` | 41A | ✅ |
| `HealthCheckCacheParams` masked params | `types/caching.py:99` | `/v1/cache/health` masked summary | 41A | ✅ |
| Stats (hits/misses/$ saved) | (logging callbacks) | `GET /v1/cache/stats` (37D) + `aichat cache stats` | 37D, 41D | ◑ |
| Clear (all / scoped to model) | (proxy admin) | `POST /v1/cache/clear` (37D) | 37D | ✅ |

---

## 3. The adaptation deltas that matter

Five places where the port is **not** a transliteration, and why:

1. **Backend trait, not duck typing.** LiteLLM's `BaseCache` relies on Python duck typing
   and `getattr`-probing (`caching.py:830`). The Rust port is a real `trait CacheBackend`
   with an explicit method set; remote backends get `#[async_trait]`. This is the
   single most load-bearing structural change — every later phase depends on 38A's trait
   being right. (LLM-engineering framing: the trait *is* the "pluggable cache" pattern;
   getting the seam at the backend boundary, not the call site, is what lets Redis and S3
   drop in without touching `call_chat_completions`.)

2. **Dependencies are a budget, not a `pip extra`.** Every remote backend is cargo-gated
   so the default `cargo build` adds **zero** dependencies and stays byte-compatible with
   today's cost-conscious build. This is the direct consequence of the constraint sheet's
   *Ask First* on dependencies, and it is why Phase 39 exists as a separate, optional
   phase rather than being folded into 38.

3. **Cache control is HTTP-native at the server, flags at the CLI.** LiteLLM expresses the
   control protocol as a `cache={...}` kwarg. aichat already *is* an HTTP gateway
   (`serve.rs`) and a CLI. So the same six controls surface as standard `Cache-Control`
   request-header directives at the server (`no-cache`, `no-store`, `max-age`, `s-maxage`,
   plus `x-aichat-cache-namespace`) and as `--cache-*` flags / role frontmatter at the
   CLI. One protocol, two idiomatic surfaces — neither is a kwarg dict.

4. **Rate-limiting helpers are dropped.** `increment_cache`, `async_set_cache_sadd`, and
   the Redis pipeline-increment machinery exist in LiteLLM because its cache doubles as
   the proxy's rate-limit counter store. aichat has no such coupling; those methods are
   ⊘ out of scope. Keeping the cache a *cache* is the one-tool-per-job call.

5. **Provider prompt caching (L3) is in-sub-track, not in-module.** LiteLLM keeps
   `cache_control` emission in its transform layer. aichat keeps it in the same caching
   sub-track (37B) because, per EVAL-0002 §5, it is the largest single cost win and the
   accounting (37A) and trace (37E) it shares with L1/L2 make a unified sub-track the
   honest place for it.

---

## 4. Coverage scorecard

| LiteLLM capability area | Features | Ported | Adapted | Gated | Dropped |
|---|---|---|---|---|---|
| Front object & lifecycle | 6 | 6 | 4 | 0 | 0 |
| Backend trait | 5 | 5 | 2 | 1 | 0 |
| In-memory backend | 6 | 5 | 0 | 0 | 1 |
| Disk backend | 3 | 3 | 2 | 0 | 0 |
| Two-tier (Dual) | 5 | 5 | 1 | 1 | 0 |
| Cache-key generation | 7 | 6 | 4 | 0 | 1 |
| Control protocol | 7 | 7 | 1 | 0 | 0 |
| Remote/distributed | 7 | 7 | 2 | 6 | 0 |
| Semantic | 4 | 4 | 3 | 1 | 0 |
| Streaming/embed/accounting | 7 | 7 | 3 | 0 | 0 |
| Admin/observability | 5 | 5 | 2 | 0 | 0 |
| **Total** | **62** | **60** | **24** | **9** | **3** |

Two non-ports are the only true gaps, both intentional: transcription-call caching and
the Responses-API call type — neither has an aichat surface to cache. The three drops are
the rate-limiter helpers (`increment`/`sadd`/pipeline-increment), which are not caching.

Net: **97% feature coverage**, with the distributed third gated behind cargo features so
the default build's dependency footprint and token-cost posture are unchanged.

---

## 5. Phase mapping (the sub-track)

```
Phase 37  Transparent Response Caching      L1/L2/L3 layers, accounting, trace, pi  (Epic 2)
   └─ enhanced: backend abstraction forward-refs 38; distributed forward-refs 39
Phase 38  Cache backend abstraction + control protocol   BaseCache→trait, Dual, DynamicCacheControl
Phase 39  Distributed & remote backends (cargo-gated)     Redis/Cluster/S3/GCS/Azure + selection
Phase 40  Auxiliary-call caching                          embeddings (RAG), rerank, token distribution
Phase 41  Cache observability & admin parity              ping/health/delete/namespace, model-group, CLI
```

Sequencing rationale and per-item designs live in each phase's overview. The spine is
**37A (accounting) → 37B/C/D/E → 38A (trait) → {38B/C/D/E, 39, 40, 41}**: nothing can be
a pluggable backend until the trait exists, and nothing is measurable until accounting
and the trace event exist.

---

## Sources

- [LiteLLM — `litellm/caching/`](https://github.com/BerriAI/litellm/tree/main/litellm/caching)
- [LiteLLM — `Cache` class (`caching.py`)](https://github.com/BerriAI/litellm/blob/main/litellm/caching/caching.py)
- [LiteLLM — `BaseCache` (`base_cache.py`)](https://github.com/BerriAI/litellm/blob/main/litellm/caching/base_cache.py)
- [LiteLLM — `DualCache` (`dual_cache.py`)](https://github.com/BerriAI/litellm/blob/main/litellm/caching/dual_cache.py)
- [LiteLLM — `DynamicCacheControl` (`types/caching.py`)](https://github.com/BerriAI/litellm/blob/main/litellm/types/caching.py)
- [LiteLLM — Caching docs](https://docs.litellm.ai/docs/caching/all_caches)
- [`EVAL-0002-full-caching.md`](EVAL-0002-full-caching.md) — the layered-model gap inventory this builds on
- [`EVAL-0003-tool-call-caching.md`](EVAL-0003-tool-call-caching.md) — sibling analysis on the tool layer LiteLLM (and 37) cache *around*
- [`phase-37-overview.md`](../../roadmap/phase-37-overview.md) — the L1/L2/L3 phase this sub-track extends