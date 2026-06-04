# Phase 39: Distributed & Remote Cache Backends : Overview - Epic 2
**Note:** Much of this has been superseded by `docs/analysis/caching/SPEC-003-cache-substrate.md`. Make sure to review that document before implementing this phase.

> **Boundary — in-aichat caching vs astrophage (Epic 16).** Phases 37–41 are the **structure-aware,
> in-process** cache: keyed on `(role, model, input)` plus provider `cache_control` (L3), living
> inside aichat. The wire-level, **runtime-agnostic** cache keyed on the canonicalized request body
> is **astrophage** (Phases 45–47), reached over `base_url`. The two never share a key — see
> [`SPEC-astrophage §0/§3`](../architecture/integrated-architecture/SPEC-astrophage.md). Phase 38A's
> `CacheBackend` trait is what lets an astrophage `Remote` backend present the same interface (45C).



**Status (2026-05-29):** **Planned — design draft.** No items below are implemented. This
phase ports LiteLLM's remote cache backends —
[`RedisCache`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/redis_cache.py),
[`RedisClusterCache`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/redis_cluster_cache.py),
[`S3Cache`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/s3_cache.py),
[`GCSCache`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/gcs_cache.py),
[`AzureBlobCache`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/azure_blob_cache.py) —
as implementations of the [Phase 38](phase-38-overview.md) `CacheBackend` trait. Per the
parity map ([`EVAL-0004`](../analysis/caching/EVAL-0004-litellm-cache-parity.md) §2.8)
**and the project's *Ask First* constraint on dependencies**, every remote backend is
**gated behind a Cargo feature** — `cargo build` with default features pulls in **zero**
new dependencies and produces a byte-identical cost-conscious binary. This phase
**reverses** [Phase 37's "distributed cache — out of scope"](phase-37-response-caching.md)
note: it is now in scope, opt-in, and isolated so it cannot tax the default build.

| Item | Description | Status |
|---|---|---|
| 39A | `RedisBackend` (feature `cache-redis`) — host/port/password/namespace/TTL, single-node and cluster (LiteLLM `RedisCache` + `RedisClusterCache`); batch flush buffer (`redis_flush_size`); `ping`/`health`; back-read coalescing for `DualBackend`. | Planned (design draft) |
| 39B | Object-store backends — `S3Backend` (`cache-s3`), `GcsBackend` (`cache-gcs`), `AzureBlobBackend` (`cache-azure`); optional `QdrantBackend` (`cache-qdrant`) for [37F](phase-37-overview.md) semantic-on-Qdrant. | Planned (design draft) |
| 39C | Backend selection + lifecycle — `cache_backend:` config enum (the LiteLLM `type=` switch), env-var credentials, lazy connect, graceful degradation to disk on connect failure. | Planned (design draft) |
| 39D | Cross-machine team cache — a shared Redis/S3 back tier behind a `DualBackend`, namespaced per team/project so a git-shared role library shares cache hits across machines. Reverses 37's per-machine-only stance. | Planned (design draft) |

## Mapping to the LiteLLM reference

| LiteLLM backend | Source | Phase 39 item | Cargo feature | Candidate crate |
|---|---|---|---|---|
| `RedisCache` | [`redis_cache.py`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/redis_cache.py) | 39A | `cache-redis` | [`redis`](https://crates.io/crates/redis) (tokio, optional cluster) |
| `RedisClusterCache` | [`redis_cluster_cache.py`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/redis_cluster_cache.py) | 39A | `cache-redis` | `redis` `cluster-async` feature |
| `S3Cache` | [`s3_cache.py`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/s3_cache.py) | 39B | `cache-s3` | `aws-sdk-s3`, or S3-compat via existing `reqwest` + SigV4 (`hmac` already present) |
| `GCSCache` | [`gcs_cache.py`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/gcs_cache.py) | 39B | `cache-gcs` | `reqwest` + GCS JSON API |
| `AzureBlobCache` | [`azure_blob_cache.py`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/azure_blob_cache.py) | 39B | `cache-azure` | `reqwest` + Azure Blob REST |
| `QdrantSemanticCache` | [`qdrant_semantic_cache.py`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/qdrant_semantic_cache.py) | 39B | `cache-qdrant` | `reqwest` + Qdrant REST |

## Design tenets

1. **Default build = zero new dependencies.** `Cargo.toml` gains its **first** `[features]`
   section. `default = []` for caching; every remote backend is an additive feature. A
   contributor who never opts in sees no new crates, no compile-time cost, no binary-size
   change. This is the non-negotiable that lets full LiteLLM parity coexist with the
   cost-conscious constraint (EVAL-0004 §3.2).
2. **Reuse `reqwest` before adding an SDK.** S3/GCS/Azure/Qdrant are all reachable over
   their REST APIs with the already-present `reqwest` + `hmac` (for SigV4) + `base64`.
   Prefer that to a heavyweight cloud SDK; reserve `aws-sdk-s3` as an alternative gated
   sub-feature (`cache-s3-sdk`) for users who want the full credential chain. Redis is the
   one backend that genuinely needs a protocol crate (`redis`).
3. **Remote is always a *back* tier, never a sole tier.** A remote backend is wrapped in a
   `DualBackend { front: InMemoryBackend, back: RemoteBackend }` (Phase 38C). This is
   LiteLLM's own posture (`DualCache` = in-memory + redis) and it means a slow or dead
   remote degrades to a fast local hit, never a slow miss-storm.
4. **Connect failure is a logged miss, not a crash.** Per 38A tenet 5: a backend whose
   connect or op fails returns "miss" and the front falls through to the model. A typo in
   `REDIS_HOST` slows you down; it never breaks a turn. 39C makes this explicit with a
   degrade-to-disk fallback.
5. **Namespacing is the multi-tenancy boundary.** All cross-machine sharing (39D) is
   scoped by the 38D `namespace` control. No global shared cache; a team opts a project
   into a shared namespace explicitly.

## 39A Design — `RedisBackend`

Ports LiteLLM `RedisCache`. Behind `#[cfg(feature = "cache-redis")]`. Implements the 38A
trait over an async `redis` connection manager.

```rust
// Sketch — src/cache/redis_backend.rs (compiled only with feature = "cache-redis")
#[cfg(feature = "cache-redis")]
pub struct RedisBackend {
    conn: redis::aio::ConnectionManager,
    namespace: Option<String>,
    default_ttl: Option<Duration>,
    flush_buffer: Mutex<Vec<(String, CacheEntry)>>,   // LiteLLM redis_flush_size
    flush_size: usize,
}

#[cfg(feature = "cache-redis")]
#[async_trait::async_trait]
impl CacheBackend for RedisBackend {
    async fn get(&self, key: &str) -> Option<CacheEntry> {
        let raw: Option<String> = self.conn.clone().get(self.k(key)).await.ok()?;
        raw.and_then(|s| serde_json::from_str(&s).ok())
    }
    async fn put(&self, key, entry, ttl) -> Result<()> {
        let v = serde_json::to_string(&entry)?;
        match ttl.or(self.default_ttl) {
            Some(t) => self.conn.clone().set_ex(self.k(key), v, t.as_secs()).await?,
            None => self.conn.clone().set(self.k(key), v).await?,
        }; Ok(())
    }
    async fn clear(&self, ns: Option<&str>) -> Result<u64> { /* SCAN + DEL by prefix */ }
    async fn ping(&self) -> Result<()> { redis::cmd("PING").query_async(...).await }
    // ...
}
```

- **Cluster mode** (LiteLLM `RedisClusterCache`): same backend, constructed from
  `redis_startup_nodes` / `REDIS_CLUSTER_NODES` env (LiteLLM `caching.py:170-192`), using
  the `redis` crate's `cluster-async` feature.
- **Batch flush** (LiteLLM `batch_cache_write` / `redis_flush_size`): the `flush_buffer`
  accumulates writes under high traffic and flushes via `MSET`/pipeline — the `put_batch`
  trait method (38A) backed by a real pipeline instead of the default loop.
- **TTL** maps to Redis `EX`; namespace is the key prefix (38D), aligning with LiteLLM's
  `_add_namespace_to_cache_key` (`caching.py:423`).

**Files:** `src/cache/redis_backend.rs` (new, feature-gated),
[`Cargo.toml`](../../Cargo.toml) (`[features] cache-redis`, optional `redis` dep).

## 39B Design — object-store backends

Ports `S3Cache` / `GCSCache` / `AzureBlobCache`. Each is one object PUT/GET per entry,
keyed `{namespace}/{hash}.json`, body = serialized `CacheEntry`. These suit *archival /
team-shared* caches where latency is dominated by the (avoided) model call, not the (added)
object fetch.

- **`S3Backend`** (`cache-s3`): default path uses `reqwest` + SigV4 (`hmac` is already a
  dep, as it is for the Bedrock client) so no AWS SDK is required; `s3_endpoint_url`
  support means it works against MinIO / R2 / any S3-compatible store (LiteLLM
  `s3_endpoint_url`, `s3_cache.py`). Optional `cache-s3-sdk` sub-feature swaps in
  `aws-sdk-s3` for the full credential provider chain.
- **`GcsBackend`** (`cache-gcs`) and **`AzureBlobBackend`** (`cache-azure`): `reqwest`
  against the GCS JSON API and Azure Blob REST respectively, mirroring LiteLLM's
  `gcs_path_service_account` / `azure_account_url` config.
- **`QdrantBackend`** (`cache-qdrant`): the one semantic remote — a vector-store back tier
  for [37F](phase-37-overview.md) when the in-tree HNSW store isn't shared across machines.
  REST via `reqwest`, mirroring LiteLLM `QdrantSemanticCache`.

Object-store TTL is best-effort (lifecycle policy on the bucket, or a `timestamp` +
`s-maxage` read-side check via 38D, since not all stores support per-object expiry —
exactly LiteLLM's stored-`timestamp` approach, `caching.py:600`).

**Files:** `src/cache/s3_backend.rs`, `src/cache/gcs_backend.rs`,
`src/cache/azure_backend.rs`, `src/cache/qdrant_backend.rs` (all new, feature-gated),
[`Cargo.toml`](../../Cargo.toml).

## 39C Design — backend selection & lifecycle

Ports LiteLLM's `type=` constructor switch (`caching.py:168-250`) to a config enum + a
factory. The factory is the only place that names concrete backends; everything else is
`Box<dyn CacheBackend>`.

```yaml
# config.yaml
cache_backend: dual            # local | disk | dual (default) | redis | s3 | gcs | azure
cache_redis_url: ${REDIS_URL}  # read from env; never inline secrets
cache_s3_bucket: my-cache-bucket
cache_namespace: team-eridian  # default namespace (38D); the 39D sharing boundary
```

```rust
// Sketch — src/cache.rs::build_backend
pub fn build_backend(cfg: &CacheConfig) -> Box<dyn CacheBackend> {
    match cfg.backend {
        CacheBackendKind::Local  => Box::new(InMemoryBackend::from(cfg)),
        CacheBackendKind::Disk   => Box::new(DiskBackend::from(cfg)),
        CacheBackendKind::Dual   => Box::new(DualBackend::memory_disk(cfg)),  // default
        #[cfg(feature = "cache-redis")]
        CacheBackendKind::Redis  => Box::new(DualBackend::memory_back(
                                        RedisBackend::connect(cfg).unwrap_or_degrade_to_disk(cfg))),
        #[cfg(feature = "cache-s3")]
        CacheBackendKind::S3     => Box::new(DualBackend::memory_back(S3Backend::from(cfg))),
        // a kind whose feature is not compiled in → warn once, fall back to Dual(disk)
        _ => { warn_feature_missing(cfg.backend); Box::new(DualBackend::memory_disk(cfg)) }
    }
}
```

- **Lazy connect + degrade-to-disk:** `RedisBackend::connect` failure logs once and the
  factory substitutes `DualBackend(memory, disk)` — the binary still runs, caching still
  works locally (tenet 4).
- **Feature-absent path:** asking for `cache_backend: redis` in a binary built without
  `cache-redis` warns once and degrades to the disk dual — never a hard error (the config
  is portable across builds with different feature sets).
- **Credentials from env only** (LiteLLM reads `REDIS_HOST`/`REDIS_PASSWORD` from env):
  config holds `${ENV_VAR}` references, resolved at load; secrets never live in
  `config.yaml`.

**Files:** [`src/cache.rs`](../../src/cache.rs) (`CacheConfig`, `CacheBackendKind`,
`build_backend`), [`src/config/mod.rs`](../../src/config/mod.rs) (config fields, env
resolution).

## 39D Design — cross-machine team cache

This is the explicit reversal of 37's "distributed cache — out of scope; the hit-rate gain
from sharing across machines is modest and the operational complexity is high." The user's
feature-parity request changes the calculus: it ships, but as an **opt-in back tier**, not
a default.

The mechanism is entirely 38C + 39A/B + 39C composed: a team points `cache_backend: redis`
(or `s3`) at a shared store and sets a shared `cache_namespace`. Now:

- Engineer A runs `aichat -r summarize <doc>`; the response lands in the shared back tier
  under `team-eridian:<hash>`.
- Engineer B, on a different machine, runs the identical role+input; their `InMemoryBackend`
  front misses, the shared back tier **hits**, and B pays nothing for the model call.
- The git-shared role library (the thing that makes the inputs *identical* across machines)
  is what makes this pay off — exactly the case 37 undervalued because it assumed roles
  diverge per machine.

Isolation and safety:

- **Namespace = tenancy boundary.** No global shared cache; sharing is per declared
  namespace. Two teams on one Redis don't collide or read each other's entries.
- **Determinism gates still apply** (37C): only `temperature == 0`, tool-free turns are
  shareable — the same correctness floor as the local cache.
- **The trace (37E) records `layer` and `key_hash`**, so a shared-cache hit is as auditable
  as a local one; a wrong shared entry is catchable.

**Files:** documentation + a `docs/features/distributed-cache.md` user guide; no new code
beyond 39A–C composition.

## Open questions

### 1. Redis crate vs hand-rolled RESP

**Question:** Pull in the `redis` crate, or speak RESP over a raw socket to avoid the dep?

**Recommendation: use the `redis` crate, gated.** Hand-rolling RESP + cluster slot routing
+ TLS is exactly the kind of subtle, security-sensitive code the project should not own.
The dep is opt-in (`cache-redis`); a user who wants Redis accepts the dep knowingly. This
is the one place a protocol crate beats `reqwest`.

### 2. Should object stores get a write-back queue?

**Question:** S3 PUT latency (50–200 ms) on the hot path of a *miss*-then-store is real.
Queue the write?

**Recommendation: yes — async fire-and-forget write to the back tier.** The `DualBackend`
already returns to the caller after the front (memory) write; the back (S3) write happens
on a spawned task. A failed back write is a logged miss-to-persist, not a turn failure.
This matches LiteLLM's async `async_add_cache` posture (`caching.py:628`).

### 3. TTL where the store has no native expiry

**Question:** S3/GCS lack reliable per-object TTL without lifecycle rules. How does `ttl` work?

**Recommendation: store `timestamp` in the `CacheEntry` (already there, 38A) and enforce
`ttl`/`s-maxage` on read** (38D). This is LiteLLM's exact mechanism (`_get_cache_logic`,
`caching.py:461`). Bucket lifecycle policies are an optional ops-side optimization for
reclaiming space, documented but not required.

## Testing

Remote backends need a service to test against; gate the bats tests on the feature + a
reachable service, and always run the **degrade-to-disk** path in CI (no service needed).

- **`tests/regression/cache-remote.sh`** — bats regression:
  - 39C/degrade: build without `cache-redis`, set `cache_backend: redis` → warns once,
    caching still works via disk dual, exit code 0.
  - 39C/degrade-connect: `cache-redis` built, `REDIS_URL` pointing at a dead port → warns,
    degrades to disk, turn succeeds.
  - 39A (gated `[ -n "$REDIS_URL" ]`): two identical requests against a live Redis back
    tier — second is a hit; `/v1/cache/stats` shows the Redis backend; `cache.lookup`
    trace records the hit.
  - 39D: two `aichat` processes (simulating two machines) sharing one Redis namespace —
    process 2's first request hits process 1's stored entry.
  - 39B (gated, MinIO in CI): S3-compat round-trip via the `reqwest`+SigV4 path against MinIO.
- **Rust unit tests** (feature-gated):
  - `tests::redis_key_carries_namespace_prefix`
  - `tests::redis_put_batch_uses_pipeline`
  - `tests::build_backend_degrades_when_feature_absent`
  - `tests::build_backend_degrades_on_connect_failure`
  - `tests::s3_entry_roundtrips_via_reqwest_sigv4` (mock HTTP)

## Sequencing

- **Blocked by Phase 38** — there is no backend trait to implement until 38A lands.
- **39C (selection/lifecycle) lands with 39A** — a backend with no way to select it is dead
  code; the degrade-to-disk path is part of the same PR.
- **39A (Redis) before 39B (object stores)** — Redis is the higher-value, more-requested
  backend and exercises the full trait (TTL, batch, ping) that 39B reuses.
- **39D is documentation + composition** — it lands once 39A (or 39B) + 39C exist; no new code.
- The `[features]` section addition is itself reviewed as the gating change (it is the
  *Ask First* dependency decision made concrete).

## Files (consolidated)

- [`Cargo.toml`](../../Cargo.toml) — **new `[features]` section**; optional `redis` dep; feature gates
- `src/cache/redis_backend.rs` — `RedisBackend` (`cache-redis`)
- `src/cache/s3_backend.rs`, `src/cache/gcs_backend.rs`, `src/cache/azure_backend.rs`,
  `src/cache/qdrant_backend.rs` — object/vector backends (each feature-gated)
- [`src/cache.rs`](../../src/cache.rs) — `CacheConfig`, `CacheBackendKind`, `build_backend` factory
- [`src/config/mod.rs`](../../src/config/mod.rs) — `cache_backend:` + connection config + env resolution
- `docs/features/distributed-cache.md` — user guide for the 39D team-cache setup

## References

- [`EVAL-0004-litellm-cache-parity.md`](../analysis/caching/EVAL-0004-litellm-cache-parity.md) §2.8 — remote-backend feature map
- [`phase-38-overview.md`](phase-38-overview.md) — the `CacheBackend` trait + `DualBackend` these implement
- [`phase-37-response-caching.md`](phase-37-response-caching.md) — the "distributed cache out of scope" note this reverses
- [LiteLLM `RedisCache`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/redis_cache.py), [`S3Cache`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/s3_cache.py)
- Project constraint: dependencies are *Ask First* ([`CLAUDE.md`](../../CLAUDE.md))