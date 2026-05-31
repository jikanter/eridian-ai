# Phase 38: Cache Backend Abstraction & Control Protocol : Overview - Epic 2

**Status (2026-05-29):** **Planned — design draft.** No items below are implemented. This
phase ports the *horizontal abstractions* of [LiteLLM's caching subsystem](https://github.com/BerriAI/litellm/tree/main/litellm/caching)
into aichat, per the feature-for-feature parity map in
[`EVAL-0004-litellm-cache-parity.md`](../analysis/open-harness/EVAL-0004-litellm-cache-parity.md).
Where [Phase 37](phase-37-overview.md) builds the *layers* (L1 exact, L2 semantic, L3
provider) against the concrete [`StageCache`](../../src/cache.rs), Phase 38 builds the
*seam underneath them*: a `CacheBackend` trait (LiteLLM `BaseCache`), the two backends
that implement it without new dependencies (`InMemoryBackend`, `DiskBackend`), the
two-tier `DualBackend` (LiteLLM `DualCache`), and the per-request **cache-control
protocol** (LiteLLM `DynamicCacheControl`) that makes caching addressable from the CLI,
role frontmatter, and HTTP `Cache-Control` headers alike. 38 is the prerequisite for
Phase 39 (remote backends), Phase 40 (auxiliary-call caching), and Phase 41 (admin
surface) — none can be pluggable until the trait exists.

| Item | Description | Status |
|---|---|---|
| 38A | `CacheBackend` trait — port LiteLLM `BaseCache` (`get`/`put`/`delete`/`clear`/`ping`/`health`, `put_batch`, TTL resolution) to a Rust trait; refactor 37's `StageCache` + transparent/server caches to implement it behind a `ResponseCache` front. | Planned (design draft) |
| 38B | `InMemoryBackend` + `DiskBackend` — bounded-entry/size-capped in-memory LRU with min-heap TTL eviction (LiteLLM `InMemoryCache`); content-addressed disk store reusing the 37C atomic-write + LRU uplift, **no `diskcache`-equivalent dependency** (LiteLLM `DiskCache`). | Planned (design draft) |
| 38C | `DualBackend` — memory-front / persistent-back, write-through + read-through-with-backfill, `local_only` write flag (LiteLLM `DualCache`). Becomes the default backend (memory + disk) for the ordinary path. | Planned (design draft) |
| 38D | Cache-control protocol — port LiteLLM `DynamicCacheControl` (`ttl`, `namespace`, `s-maxage`, `no-cache`, `no-store`, `use-cache`) and `CacheMode` (`default_on`/`default_off`). Surfaces as `Cache-Control` headers (server), `--cache-*` flags (CLI), and `cache:` frontmatter (role). | Planned (design draft) |
| 38E | `supported_call_types` gate — a `CacheableCall` enum (chat / embedding / rerank) that scopes which operations a backend serves; the foundation [Phase 40](phase-40-overview.md) consumes. | Planned (design draft) |

## Mapping to the LiteLLM reference

| LiteLLM construct | Source | Phase 38 item |
|---|---|---|
| `BaseCache` abstract class | [`base_cache.py`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/base_cache.py) | 38A |
| `Cache` front + `type=` selector | [`caching.py:55`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/caching.py) | 38A |
| `InMemoryCache` | [`in_memory_cache.py`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/in_memory_cache.py) | 38B |
| `DiskCache` | [`disk_cache.py`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/disk_cache.py) | 38B |
| `DualCache` | [`dual_cache.py`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/dual_cache.py) | 38C |
| `DynamicCacheControl` + `CacheMode` | [`types/caching.py:69`](https://github.com/BerriAI/litellm/blob/main/litellm/types/caching.py), `caching.py:49` | 38D |
| `supported_call_types` | `caching.py:70-83` | 38E |

## Design tenets

1. **The seam is the backend boundary, not the call site.** This is the load-bearing
   decision (EVAL-0004 §3.1). `call_chat_completions` / `call_react` /
   `chat_completions` talk to a `ResponseCache` front; the front talks to a
   `Box<dyn CacheBackend>`. Swapping memory→disk→dual→redis never touches the call sites.
   Get this wrong and every later phase pays for it.
2. **Zero new default dependencies.** 38A–E ship using only crates already in
   `Cargo.toml` (`sha2`, `parking_lot`, `serde_json`, `tokio`). The trait is `async` via
   the already-present `async-trait`; the in-memory and disk backends need nothing new.
   Remote backends and their deps are Phase 39, cargo-gated.
3. **Extend 37, do not fork it.** 37's `transparent_key`, atomic-write, and LRU work
   (37C) become the `DiskBackend` body; 37D's in-memory LRU becomes `InMemoryBackend`.
   38 is a *refactor-to-trait* of code 37 specifies, plus the control protocol. The two
   phases share `src/cache.rs`.
4. **Cache-control is one protocol with idiomatic surfaces.** The six LiteLLM controls
   are expressed once as a `CacheControl` struct, then surfaced as HTTP `Cache-Control`
   directives at the server, `--cache-*` flags at the CLI, and `cache:` frontmatter on
   roles. No control exists on one surface and not the others (except where physically
   meaningless — `s-maxage` is a server/client read concern, not a role-author concern).
5. **Backends never block the request path on failure.** LiteLLM wraps every cache op in
   "never block execution" try/except (`caching.py:511,548`). The Rust port returns
   `Result` and the front swallows backend errors into a logged miss — a dead Redis or a
   full disk degrades to "cache miss," never to a failed turn.

## 38A Design — the `CacheBackend` trait

LiteLLM's `BaseCache` (`base_cache.py:22`) is duck-typed Python; the port is an explicit
trait. The front object (`ResponseCache`) owns one boxed backend and all the
control-protocol logic, mirroring how LiteLLM's `Cache` owns a `self.cache: BaseCache`.

```rust
// Sketch — src/cache.rs
#[async_trait::async_trait]
pub trait CacheBackend: Send + Sync {
    /// Fetch a stored entry. `None` on miss OR on any backend error (logged).
    async fn get(&self, key: &str) -> Option<CacheEntry>;
    /// Store an entry with a resolved TTL. Errors are logged, not propagated.
    async fn put(&self, key: &str, entry: CacheEntry, ttl: Option<Duration>) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
    /// Clear all, or only entries whose key carries `namespace:` prefix.
    async fn clear(&self, namespace: Option<&str>) -> Result<u64>;
    /// Batch write (LiteLLM async_set_cache_pipeline). Default: sequential puts.
    async fn put_batch(&self, items: Vec<(String, CacheEntry, Option<Duration>)>) -> Result<()> {
        for (k, e, ttl) in items { self.put(&k, e, ttl).await?; } Ok(())
    }
    /// Liveness probe (LiteLLM ping/test_connection). In-memory/disk are always live.
    async fn ping(&self) -> Result<()> { Ok(()) }
    fn health(&self) -> BackendHealth;     // masked params for /v1/cache/health (41A)
    fn stats(&self) -> BackendStats;       // hits/misses/bytes/entries for 37D /v1/cache/stats
}

/// Stored value + the metadata 37 already tracks in its `.meta` sidecar.
pub struct CacheEntry {
    pub body: String,                 // canonicalized response body
    pub timestamp: SystemTime,        // LiteLLM cached_data["timestamp"] — for s-maxage
    pub metrics: CallMetrics,         // tokens/cost (37A) — replayed in `usage`
    pub stream: bool,                 // streaming vs non-streaming entry (37D)
}

/// The front object. Holds the backend + the control/mode logic.
pub struct ResponseCache {
    backend: Box<dyn CacheBackend>,
    mode: CacheMode,                  // 38D
    namespace: Option<String>,        // default namespace (38D/41C)
    default_ttl: Option<Duration>,
}
```

`StageCache` (the existing concrete type) becomes a thin compatibility shim that
constructs a `DiskBackend` — its two callers (`src/pipe.rs`, `src/knowledge/compile.rs`)
keep working unchanged.

**Files:** [`src/cache.rs`](../../src/cache.rs) (trait, `ResponseCache`, `CacheEntry`,
shim), [`src/client/common.rs`](../../src/client/common.rs) (call sites talk to
`ResponseCache`), [`src/serve.rs`](../../src/serve.rs) (server holds a `ResponseCache`).

## 38B Design — `InMemoryBackend` + `DiskBackend`

**`InMemoryBackend`** ports LiteLLM `InMemoryCache` (`in_memory_cache.py`) faithfully:

- `max_entries` (default 200, matching LiteLLM) — bounded count.
- `max_bytes_per_item` (default 1 MiB, matching LiteLLM's `max_size_per_item`) —
  oversize entries are silently not cached (`check_value_size`).
- TTL eviction via a min-heap of `(expiry_instant, key)` (LiteLLM's `expiration_heap`),
  pruned on every `put`, plus count-eviction of the soonest-to-expire when at capacity.
- `default_ttl` (configurable; LiteLLM default 600 s).

```rust
// Sketch — src/cache.rs
pub struct InMemoryBackend {
    map: parking_lot::RwLock<HashMap<String, (CacheEntry, Instant)>>,   // value, expiry
    heap: parking_lot::Mutex<BinaryHeap<Reverse<(Instant, String)>>>,   // expiry min-heap
    max_entries: usize,            // 200
    max_bytes_per_item: usize,     // 1 MiB
    default_ttl: Duration,         // 600s
}
```

**`DiskBackend`** is the 37C work, reframed as a backend. It reuses the content-addressed
file store, the **atomic write-temp-then-rename** (37C), and the **mtime-LRU eviction
under a byte budget** (37C, default 500 MiB). The deliberate non-port: LiteLLM's
`DiskCache` pulls in the `diskcache` library (`disk_cache.py:17`); aichat reuses its own
`fs` + `sha2` primitive and adds **no dependency** (EVAL-0004 §2.4).

**Files:** [`src/cache.rs`](../../src/cache.rs) (`InMemoryBackend`, `DiskBackend`).

## 38C Design — `DualBackend` (two-tier)

LiteLLM's `DualCache` (`dual_cache.py:51`) is the pattern that makes "fast local + durable
shared" work: write-through to both tiers, read the front first and **backfill** the
front from the back on a back-hit (`dual_cache.py:167-177`). aichat's default ordinary-path
cache becomes a `DualBackend { front: InMemoryBackend, back: DiskBackend }` — process-local
reads are instant, survive restart via disk, and Phase 39 can swap `back` for a
`RedisBackend`/`S3Backend` with no call-site change.

```rust
// Sketch — src/cache.rs
pub struct DualBackend {
    front: Box<dyn CacheBackend>,   // InMemoryBackend
    back: Box<dyn CacheBackend>,    // DiskBackend (38B) or RedisBackend (39)
}

#[async_trait::async_trait]
impl CacheBackend for DualBackend {
    async fn get(&self, key: &str) -> Option<CacheEntry> {
        if let Some(hit) = self.front.get(key).await { return Some(hit); }
        let back_hit = self.back.get(key).await?;          // read-through
        let _ = self.front.put(key, back_hit.clone(), None).await;  // backfill
        Some(back_hit)
    }
    async fn put(&self, key, entry, ttl) -> Result<()> {   // write-through
        let _ = self.front.put(key, entry.clone(), ttl).await;
        self.back.put(key, entry, ttl).await               // local_only (38D) skips this
    }
    // delete/clear fan out to both tiers
}
```

The `local_only` write flag from LiteLLM (`dual_cache.py:114`) maps onto the `no-store`
control's tiered form: a request may say "cache in memory for this process, don't persist."

**Files:** [`src/cache.rs`](../../src/cache.rs) (`DualBackend`).

## 38D Design — cache-control protocol + mode

This is the most user-visible part of the phase. LiteLLM's `DynamicCacheControl`
(`types/caching.py:69`) is six controls passed per request; aichat expresses them once and
surfaces them on three idiomatic surfaces.

```rust
// Sketch — src/cache.rs
#[derive(Default, Clone)]
pub struct CacheControl {
    pub ttl: Option<Duration>,        // entry lifetime          (LiteLLM "ttl")
    pub namespace: Option<String>,    // key partition           (LiteLLM "namespace")
    pub s_maxage: Option<Duration>,   // reject reads older than (LiteLLM "s-maxage")
    pub no_cache: bool,               // skip read, still write  (LiteLLM "no-cache")
    pub no_store: bool,               // skip write, read ok     (LiteLLM "no-store")
    pub use_cache: bool,              // opt-in when default_off  (LiteLLM "use-cache")
    pub local_only: bool,            // 38C tiered no-store
}

pub enum CacheMode { DefaultOn, DefaultOff }   // LiteLLM CacheMode
```

**Read logic** ports LiteLLM `get_cache` + `_get_cache_logic` (`caching.py:500-537,461`):
`no-cache` skips the read; on a hit, if `s-maxage` is set and `now - entry.timestamp >
s-maxage`, treat as a miss (LiteLLM `caching.py:469-498`). **Write logic** ports `add_cache`
(`caching.py:607`): `no-store` skips the write; `ttl` overrides the default.

**`should_use_cache`** ports LiteLLM `caching.py:808`: under `DefaultOn`, cache unless
opted out; under `DefaultOff`, cache only when `use_cache` is set. This is exactly the
`transparent_cache` default-flip story already in 37C/37E open-question #1 — 38D gives it
a name and a per-request override.

**Three surfaces, one protocol:**

| Surface | How a control is expressed | Example |
|---|---|---|
| HTTP server (`serve.rs`) | standard `Cache-Control` request header + `x-aichat-cache-namespace` | `Cache-Control: no-store, s-maxage=60` |
| CLI | `--cache-ttl`, `--cache-namespace`, `--no-transparent-cache` (= `no-cache`), `--cache-no-store` | `aichat --transparent-cache --cache-ttl 600 -r summarize` |
| Role frontmatter | `cache:` block | `cache: { ttl: 3600, namespace: "faq" }` |

The `--no-transparent-cache` one-off (a request-scoped `no-cache`) is the ergonomics
escape hatch 37 reserved; `--no-cache` keeps its pipeline-only meaning (37C tenet).

**Files:** [`src/cache.rs`](../../src/cache.rs) (`CacheControl`, `CacheMode`, read/write
gating), [`src/cli.rs`](../../src/cli.rs) (flags), [`src/config/role.rs`](../../src/config/role.rs)
(`cache:` block parsing — extends 37C's `cache: false|semantic`),
[`src/serve.rs`](../../src/serve.rs) (`Cache-Control` header parse),
[`src/config/mod.rs`](../../src/config/mod.rs) (`cache_mode:` default).

## 38E Design — `supported_call_types` gate

LiteLLM's `supported_call_types` (`caching.py:70-83`) is an allow-list of which operation
kinds the cache serves (completion, embedding, transcription, rerank, responses). aichat's
analogue is narrower — the operations that exist in-tree:

```rust
// Sketch — src/cache.rs
pub enum CacheableCall { Chat, Embedding, Rerank }   // aichat's surfaces

// config.yaml
// cache_supported_calls: [chat]          # default — Phase 40 adds embedding/rerank
```

38E ships the enum and the gate (a backend refuses to serve a call type not in the
allow-list) with only `Chat` active. [Phase 40](phase-40-overview.md) flips on `Embedding`
and `Rerank`. This keeps the embedding-cache work cleanly behind a config gate rather than
implicitly caching every `embed()` the day the trait lands.

**Files:** [`src/cache.rs`](../../src/cache.rs) (`CacheableCall` gate),
[`src/config/mod.rs`](../../src/config/mod.rs) (`cache_supported_calls:`).

## How 38 composes with prior phases

| Prior phase | Composition |
|---|---|
| **Phase 10B** (`StageCache`) | `StageCache` becomes a shim over `DiskBackend`; its two callers are unchanged. |
| **Phase 37** (response caching) | 37 specifies `transparent_key`, atomic write, LRU, the trace event, the `.meta` sidecar. 38 turns those into trait methods. 37A's `CallMetrics` is the `CacheEntry.metrics`. 37E's `cache.lookup` event fires from the `ResponseCache` front, so every backend inherits the trace for free. |
| **Phase 17** (server execution) | The server's `ResponseCache` front sits where 37D placed the LRU; 38C makes it a `DualBackend`. |
| **Phase 21/22** (DAG) | 22D's per-branch caching uses a `ResponseCache` with a branch-scoped `namespace` — 38D's namespace control is exactly the partition key it needs. |
| **Phase 36** (stage config isolation) | A stage's `config_override` changes the cache key (different tools/sampling), so isolated stages get distinct entries naturally; 38 adds nothing 36 must know about. |

## Open questions

### 1. Sync trait vs `async_trait` everywhere

**Question:** In-memory and disk backends are synchronous; only remote backends (39) need
async. Should the trait be sync with an async adapter, or async throughout?

**Recommendation: async throughout via `async-trait`.** The crate is already a
dependency. A sync core with an async adapter splits the trait in two and makes
`DualBackend { front: sync, back: async }` awkward. The async overhead on an in-memory
`RwLock` read is negligible against a network turn. Uniform async keeps `DualBackend` and
the call sites simple.

### 2. Where does the `.meta` sidecar live in the trait?

**Question:** 37's disk layout stores body in `<hash>.out` and metrics in `<hash>.meta`.
Does the trait expose two blobs or one `CacheEntry`?

**Recommendation: one `CacheEntry`; the `DiskBackend` is free to split it into two files
internally.** The trait speaks `CacheEntry`; the on-disk two-file layout is a `DiskBackend`
implementation detail (it lets `--info` read metrics without parsing the body). Remote
backends serialize the whole `CacheEntry` as one JSON value, matching LiteLLM's single
`cached_data = {"timestamp", "response"}` blob (`caching.py:600`).

### 3. Default backend for the ordinary path

**Question:** Memory-only, disk-only, or dual?

**Recommendation: `DualBackend(memory, disk)`.** Memory alone loses the cache on every CLI
invocation (each `aichat ...` is a fresh process — the dominant case). Disk alone pays a
file read on every hit even within one `--serve` process. Dual gives both: instant repeat
hits within a process, durable hits across processes. This is precisely why LiteLLM's proxy
defaults to a dual (in-memory + redis) shape.

## Testing

Per project guideline ("*Always* add integration tests via bats in addition to unit tests"):

- **`tests/regression/cache-backends.sh`** — bats regression:
  - 38A: `aichat --transparent-cache -r summarize <in.txt>` twice; second is a hit (shim
    path), `cache.lookup outcome: hit` in the trace.
  - 38C: a `--serve` process serves a repeat request from the memory front (<5 ms); after
    process restart, the same request is served from disk back (still a hit).
  - 38D/no-store: `curl -H 'Cache-Control: no-store'` twice — second still misses (nothing
    was written).
  - 38D/no-cache: a write happens but `-H 'Cache-Control: no-cache'` always re-calls the model.
  - 38D/s-maxage: an entry older than `s-maxage=1` is treated as a miss.
  - 38D/namespace: two requests with different `x-aichat-cache-namespace` do not collide;
    `POST /v1/cache/clear {namespace}` drops only one.
  - 38E: with `cache_supported_calls: [chat]`, an embedding call is not cached.
- **Rust unit tests** in `src/cache.rs`:
  - `tests::in_memory_evicts_by_ttl_then_count`
  - `tests::in_memory_rejects_oversize_item`
  - `tests::dual_backfills_front_on_back_hit`
  - `tests::dual_write_through_hits_both_tiers`
  - `tests::cache_control_no_store_skips_write`
  - `tests::cache_control_s_maxage_rejects_stale_entry`
  - `tests::namespace_prefixes_key_and_scopes_clear`
  - `tests::backend_error_degrades_to_miss_not_failure`
  - `tests::stagecache_shim_roundtrips_via_disk_backend`

## Sequencing

- **38A first** — the trait blocks everything. Land it with `DiskBackend` (the `StageCache`
  shim) so 37's callers migrate in the same PR.
- **38B + 38C together** — `InMemoryBackend` is only useful composed into `DualBackend`.
- **38D after A–C** — the control protocol needs a backend to control.
- **38E last** — the call-type gate is a thin guard; it can land with 38D or just after.
- 38 must land **before** Phase 39 (no remote backend without the trait), and 38D's
  `CacheMode` is what 37's default-flip open-question resolves against.

## Files (consolidated)

- [`src/cache.rs`](../../src/cache.rs) — `CacheBackend` trait, `ResponseCache` front,
  `CacheEntry`, `InMemoryBackend`, `DiskBackend`, `DualBackend`, `CacheControl`,
  `CacheMode`, `CacheableCall`, `StageCache` shim
- [`src/client/common.rs`](../../src/client/common.rs) — call sites talk to `ResponseCache`
- [`src/serve.rs`](../../src/serve.rs) — server holds a `ResponseCache`; `Cache-Control` parse
- [`src/cli.rs`](../../src/cli.rs) — `--cache-ttl`, `--cache-namespace`, `--cache-no-store`,
  `--no-transparent-cache`
- [`src/config/role.rs`](../../src/config/role.rs) — `cache:` block parsing
- [`src/config/mod.rs`](../../src/config/mod.rs) — `cache_mode:`, `cache_supported_calls:`

## References

- [`EVAL-0004-litellm-cache-parity.md`](../analysis/open-harness/EVAL-0004-litellm-cache-parity.md) — the feature map this phase implements (§2.1–2.7)
- [`phase-37-overview.md`](phase-37-overview.md) — the layers this seam sits beneath
- [`phase-39-overview.md`](phase-39-overview.md) — remote backends that plug into 38A's trait
- [`phase-40-overview.md`](phase-40-overview.md) — auxiliary-call caching gated by 38E
- [`phase-41-overview.md`](phase-41-overview.md) — admin surface over 38A's `health`/`stats`/`ping`
- [LiteLLM `BaseCache`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/base_cache.py), [`DualCache`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/dual_cache.py), [`DynamicCacheControl`](https://github.com/BerriAI/litellm/blob/main/litellm/types/caching.py)