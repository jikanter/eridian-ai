# Phase 41: Cache Observability & Admin Parity : Overview - Epic 2

**Status (2026-05-29):** **Planned — design draft.** No items below are implemented. This
phase ports LiteLLM's cache **admin and observability surface** — `/cache/ping`,
`/cache/delete`, `HealthCheckCacheParams`, the cache-key-in-response convention, caching
across model groups, and the `enable_cache`/`disable_cache`/`update_cache` runtime
toggles ([`EVAL-0004`](../analysis/open-harness/EVAL-0004-litellm-cache-parity.md) §2.11,
§2.6). [Phase 37D](phase-37-overview.md) already specifies `GET /v1/cache/stats`,
`POST /v1/cache/clear`, and `/v1/cache/{enable,disable}` plus the pi slash commands; 41
completes the surface to full LiteLLM parity and adds the **one-tool-per-job CLI**
(`aichat cache …`) that the project's Unix ethos calls for. It is the *legibility* layer:
the cache built across 37–40 becomes inspectable, prunable, and health-checkable from the
CLI, the HTTP API, and the pi REPL alike.

| Item | Description | Status |
|---|---|---|
| 41A | Health & lifecycle endpoints — `GET /v1/cache/ping` (liveness, LiteLLM `CachePingResponse`), `GET /v1/cache/health` (masked backend params, LiteLLM `HealthCheckCacheParams`), `POST /v1/cache/delete {keys|namespace}` (LiteLLM `delete_cache_keys`). Extends 37D's stats/clear. | Planned (design draft) |
| 41B | Cache-key in response — surface `x-aichat-cache-key` response header + `usage.cache_key` (LiteLLM `_hidden_params["cache_key"]`), so a caller can correlate, delete, or warm a specific entry. | Planned (design draft) |
| 41C | Caching across model groups — a request via a role/alias that resolves to one of several interchangeable models can share a cache group (LiteLLM `caching_groups`/`model_group`). Namespaced; opt-in. | Planned (design draft) |
| 41D | `aichat cache` CLI + runtime toggles — `aichat cache {stats,clear,ping,health,delete,enable,disable}` (one-tool-per-job surface) backed by the same logic as the HTTP routes; runtime `enable`/`disable`/`update` of the backend (LiteLLM `enable_cache`/`disable_cache`/`update_cache`). | Planned (design draft) |

## Mapping to the LiteLLM reference

| LiteLLM feature | Source | Phase 41 item |
|---|---|---|
| `CachePingResponse` / `/cache/ping` | [`types/caching.py:90`](https://github.com/BerriAI/litellm/blob/main/litellm/types/caching.py) | 41A |
| `HealthCheckCacheParams` (masked) | `types/caching.py:99` | 41A |
| `delete_cache_keys` | [`caching.py:836`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/caching.py) | 41A |
| `cache_key` in `_hidden_params` | [`caching_handler.py:247,365`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/caching_handler.py) | 41B |
| `caching_groups` / `model_group` keying | `caching.py:337-362` | 41C |
| `enable_cache` / `disable_cache` / `update_cache` | `caching.py:856-987` | 41D |

## Design tenets

1. **Every surface, one core.** `stats`, `clear`, `delete`, `ping`, `health`,
   `enable`/`disable` are implemented once on the [Phase 38](phase-38-overview.md)
   `ResponseCache` front (via the trait's `stats()`/`health()`/`ping()`/`clear()`/`delete()`).
   The HTTP routes, the CLI subcommands, and the pi slash commands are thin adapters over
   that one core. No logic is duplicated across surfaces.
2. **A cache you can't inspect is a regression hazard.** This is ADR-0001's "trace as
   keystone" applied to the admin surface: the same reasoning that makes 37E's
   `cache.lookup` event blocking makes the stats/health surface non-optional. A silently
   broken cache that always misses costs money invisibly; `ping`/`health`/`stats` make it
   visible.
3. **Secrets are masked, always.** LiteLLM's `HealthCheckCacheParams` runs `mask_dict()`
   before returning. The aichat `health()` output masks Redis passwords, S3 keys, and any
   `${ENV}`-sourced credential — health is for humans and dashboards, never a credential leak.
4. **Model-group caching is opt-in and namespaced.** Sharing one cache entry across "the
   model that answered" is powerful and dangerous; it ships behind an explicit
   `cache_model_group:` declaration (41C), never implicitly.
5. **The CLI is the primary surface; HTTP and pi mirror it.** Per the project's
   one-tool-per-job ethos, `aichat cache stats` is a first-class command a user can pipe,
   script, and `watch`. The HTTP routes exist for the `--serve`/pi topology; the CLI exists
   for everyone.

## 41A Design — health & lifecycle endpoints

Extends 37D's `/v1/cache/*` block. New routes, each backed by a `ResponseCache`/`CacheBackend`
method (38A):

| Method | Path | Backend method | Returns |
|---|---|---|---|
| `GET` | `/v1/cache/ping` | `backend.ping()` | `{"status":"ok","backend":"dual(memory,disk)","latency_ms":2}` (LiteLLM `CachePingResponse`) |
| `GET` | `/v1/cache/health` | `backend.health()` | masked `{"backend":"redis","host":"r***.cache","namespace":"team-eridian","entries":1840,"bytes":48210011}` |
| `POST` | `/v1/cache/delete` | `backend.delete(key)` / `clear(ns)` | `{"deleted": 3}` — body `{"keys":[...]}` or `{"namespace":"faq"}` |

`ping` is the liveness check a `DualBackend` answers by pinging its back tier (a dead Redis
shows `degraded`, not `ok`). `health` is the masked-params summary for a dashboard. `delete`
is the surgical complement to 37D's bulk `clear` — drop one stale entry by key (the key a
caller learned from 41B) without flushing the whole cache.

All routes keep 37D's bridge-token gating (the `/v1/state/*` auth at `src/serve.rs:246`).

**Files:** [`src/serve.rs`](../../src/serve.rs) (3 new routes),
[`src/cache.rs`](../../src/cache.rs) (`BackendHealth` masking, `delete`/`ping`/`health`).

## 41B Design — cache-key in response

LiteLLM stamps the computed cache key onto the response (`_hidden_params["cache_key"]`,
`caching_handler.py:247`) so a caller can later inspect or delete exactly that entry. aichat
surfaces it two ways on the server response:

```
HTTP/1.1 200 OK
x-aichat-cache-key: sha256:abc123…
x-aichat-cache-hit: true
```
```json
{ "usage": { "prompt_tokens": 842, "cached_tokens": 842, "cache_hit": true,
             "cache_key": "sha256:abc123…", "cost_usd": 0.0 } }
```

This closes the loop with 41A: a caller sees a wrong/stale reply, reads its
`x-aichat-cache-key`, and `POST /v1/cache/delete {"keys":["sha256:abc123…"]}` evicts exactly
it. It also enables **cache warming** scripts (a deferred 37 non-goal that becomes trivial
once keys are addressable) and lets the open-harness trace (37E) correlate a `chat.response`
to its `cache.lookup` by key.

**Files:** [`src/serve.rs`](../../src/serve.rs) (response headers + `usage` fields),
[`src/cache.rs`](../../src/cache.rs) (expose the computed key on the front).

## 41C Design — caching across model groups

LiteLLM's router caches across a *model group*: a request routed to any of
`[gpt-5, gpt-5-mini]` can share a cache entry if they're declared interchangeable
(`_get_model_param_value` → `caching_groups`, `caching.py:337`). aichat's analogue is the
role/alias layer — a role pinned to a logical model that resolves to one of several
back-ends, or a user who treats two models as equivalent for a task.

```yaml
# config.yaml — opt-in, explicit
cache_model_groups:
  - name: fast-summarizers
    members: [claude-haiku-4-5, gpt-5-mini]      # share a cache namespace
    namespace: grp:fast-summarizers
```

When a request's model is a member, the cache key uses the **group name** in place of the
`model_id` field (everything else in the 37C key is unchanged). A summarize turn answered by
Haiku is then a cache hit for the same turn requested against gpt-5-mini.

This is the one feature that *deliberately weakens* the "cache key includes `model_id`
because models produce different responses" rule from 37 — so it is **opt-in, explicit, and
namespaced**, with the trade-off documented: members must be genuinely interchangeable for
the task or the group will serve a Haiku answer where the user expected gpt-5. The trace
(37E) records which group served the entry so the trade-off is auditable.

**Files:** [`src/cache.rs`](../../src/cache.rs) (group resolution in key construction —
ports LiteLLM `_get_model_param_value`/`_get_caching_group`),
[`src/config/mod.rs`](../../src/config/mod.rs) (`cache_model_groups:` config).

## 41D Design — `aichat cache` CLI + runtime toggles

The one-tool-per-job surface. A new `cache` subcommand group whose handlers call the same
`ResponseCache` methods the HTTP routes do:

```
aichat cache stats                 # hits/misses/$ saved/entries/bytes (= GET /v1/cache/stats)
aichat cache ping                  # liveness of the configured backend
aichat cache health                # masked backend params
aichat cache clear [--namespace N] [--model M]   # bulk evict
aichat cache delete <key>...       # surgical evict by key (from 41B)
aichat cache enable | disable      # runtime toggle (persists to config)
```

Runtime `enable`/`disable`/`update` port LiteLLM's `enable_cache()`/`disable_cache()`/
`update_cache()` (`caching.py:856-987`): they swap the active backend (e.g. `update` to
re-point at a different Redis) without restarting a long-running `--serve` process. `disable`
sets `CacheMode::DefaultOff` live; `enable` restores it.

The pi slash commands (37D's `/cache-stats`, `/cache-clear`, `/transparent-cache`) gain
`/cache-ping` and `/cache-health` to mirror the CLI, each translating to the matching
`/v1/cache/*` route via `bridgeFetch` ([`pi-extensions/src/index.ts:28`](../../pi-extensions/src/index.ts)).

**Files:** [`src/cli.rs`](../../src/cli.rs) (`cache` subcommand),
[`src/main.rs`](../../src/main.rs) (dispatch), [`src/cache.rs`](../../src/cache.rs)
(runtime enable/disable/update), [`pi-extensions/src/index.ts`](../../pi-extensions/src/index.ts)
(`/cache-ping`, `/cache-health`), [`assets/pi-extensions/aichat-bridge.js`](../../assets/pi-extensions/aichat-bridge.js) (rebuilt).

## How 41 composes with prior phases

| Prior phase | Composition |
|---|---|
| **Phase 37D** | 41A extends the `/v1/cache/*` route block; 41D extends the pi slash commands. Same bridge-token auth. |
| **Phase 37E** (trace) | 41B's `cache_key` is the join key between a `chat.response` and its `cache.lookup` event. |
| **Phase 38A** (trait) | 41A/D are thin adapters over `stats()`/`health()`/`ping()`/`clear()`/`delete()`. |
| **Phase 39** (remote) | `ping`/`health` are how a user confirms their Redis/S3 backend is actually connected (vs silently degraded to disk). |
| **Phase 40** (aux calls) | `stats` aggregates embedding/rerank hits alongside chat; `clear --namespace` can drop just the embedding cache. |

## Open questions

### 1. Should `aichat cache stats` read a running server or the on-disk cache?

**Question:** Stats for a `--serve` process live in memory; stats for the CLI cache live on
disk. Which does `aichat cache stats` report?

**Recommendation: on-disk by default; `--server URL` to query a running server.** The CLI
command should work with no server running (the dominant case) by reading the disk backend's
counters. A `--server` flag (or auto-detect of a local bridge) queries `GET /v1/cache/stats`
for live in-memory figures. Both go through the same `stats()` method; only the backend
instance differs.

### 2. Does `update_cache` (re-point the backend) risk losing entries?

**Question:** Runtime `update` to a different backend abandons the old one's entries.

**Recommendation: warn and require `--force`.** `aichat cache update --backend redis
--force` is allowed; without `--force` it errors with "this abandons N entries in the
current backend." LiteLLM's `update_cache` silently replaces; aichat adds the guard because a
CLI user is more likely to fat-finger it than a programmatic proxy config.

### 3. Model-group caching default

**Question:** Ship any default model groups?

**Recommendation: none.** Model-group caching trades correctness for hit rate (41C). Shipping
a default group ("all the cheap summarizers are interchangeable") would make that trade for
the user silently. It stays empty until the user declares a group they understand.

## Testing

- **`tests/regression/cache-admin.sh`** — bats regression:
  - 41A/ping: `aichat cache ping` and `GET /v1/cache/ping` both report the configured
    backend and `ok`; with a dead Redis (39), both report `degraded`.
  - 41A/health: `aichat cache health` masks the Redis password and S3 key.
  - 41A/delete: store an entry, read its key (41B), `aichat cache delete <key>` → the next
    identical request misses.
  - 41B: a server hit response carries `x-aichat-cache-key` and `x-aichat-cache-hit: true`;
    the key matches the one `cache.lookup` logged.
  - 41C: declare a `cache_model_group`; a turn answered by member A is a hit for the same
    turn requested against member B; the trace records the group.
  - 41D/toggle: `aichat cache disable` → next turn misses; `aichat cache enable` → caching
    resumes; setting persists to config.
  - 41D/update-guard: `aichat cache update --backend redis` without `--force` errors and
    abandons nothing.
- **Rust unit tests** in `src/cache.rs` / `src/serve.rs`:
  - `tests::health_masks_credentials`
  - `tests::delete_by_key_evicts_single_entry`
  - `tests::delete_by_namespace_scopes_eviction`
  - `tests::model_group_key_substitutes_group_for_model_id`
  - `tests::runtime_disable_sets_default_off_mode`
  - `tests::v1_cache_ping_reports_degraded_on_dead_back_tier`

## Sequencing

- **Blocked by Phase 38** (the trait methods 41 adapts) and **Phase 37D** (the route block
  and pi commands 41 extends).
- **41A + 41B land together** — `delete` (41A) is only useful once a caller can learn a key
  (41B); ship the pair.
- **41D (CLI) can land alongside 41A** — it is the same logic on a different surface; doing
  both at once keeps the "one core, every surface" tenet honest.
- **41C (model groups) is independent** and lands last — it is the one feature that touches
  key construction and carries a correctness trade-off, so it ships after the rest is
  measured.

## Files (consolidated)

- [`src/serve.rs`](../../src/serve.rs) — `/v1/cache/{ping,health,delete}` routes; response
  `x-aichat-cache-key`/`x-aichat-cache-hit` headers + `usage.cache_key`
- [`src/cache.rs`](../../src/cache.rs) — `BackendHealth` masking; `delete`/`ping`/`health`;
  model-group key resolution; runtime enable/disable/update
- [`src/cli.rs`](../../src/cli.rs) — `aichat cache {stats,clear,ping,health,delete,enable,disable,update}`
- [`src/main.rs`](../../src/main.rs) — subcommand dispatch
- [`src/config/mod.rs`](../../src/config/mod.rs) — `cache_model_groups:`
- [`pi-extensions/src/index.ts`](../../pi-extensions/src/index.ts) — `/cache-ping`, `/cache-health`
- [`assets/pi-extensions/aichat-bridge.js`](../../assets/pi-extensions/aichat-bridge.js) — rebuilt artifact

## References

- [`EVAL-0004-litellm-cache-parity.md`](../analysis/open-harness/EVAL-0004-litellm-cache-parity.md) §2.11, §2.6 — admin/observability + model-group feature map
- [`phase-37-overview.md`](phase-37-overview.md) — 37D's `/v1/cache/*` block + pi commands this extends; 37E trace
- [`phase-38-overview.md`](phase-38-overview.md) — the trait methods 41 adapts to every surface
- [`phase-39-overview.md`](phase-39-overview.md) — remote backends whose connection `ping`/`health` verify
- [LiteLLM `CachePingResponse`/`HealthCheckCacheParams`](https://github.com/BerriAI/litellm/blob/main/litellm/types/caching.py), [`enable_cache`/`disable_cache`/`update_cache`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/caching.py)