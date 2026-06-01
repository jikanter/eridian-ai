# Phase 40: Auxiliary-Call Caching (Embeddings & Rerank) : Overview - Epic 2

**Status (2026-05-29):** **Planned — design draft.** No items below are implemented. This
phase ports LiteLLM's **embedding caching** ([`caching.py:735-806`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/caching.py))
and its `supported_call_types` breadth beyond chat completion
([`EVAL-0004`](../analysis/caching/EVAL-0004-litellm-cache-parity.md) §2.10, §2.6).
[Phase 37](phase-37-overview.md) caches *chat responses*; LiteLLM also caches embeddings,
reranks, transcriptions, and Responses-API calls keyed by their inputs. aichat's analogues
are the **embedding** and **rerank** calls that power RAG — `embeddings()` at
[`src/client/common.rs:136`](../../src/client/common.rs) and `rerank()` at
[`src/client/common.rs:143`](../../src/client/common.rs). These are *deterministic by
construction* (an embedding of a fixed string under a fixed model is fixed), so they are
the safest possible cache targets — no temperature gate, no tool-side-effect risk. The win
is concrete: RAG sync (`src/rag/mod.rs:647` `create_embeddings`) re-embeds unchanged
documents on every rebuild today.

| Item | Description | Status |
|---|---|---|
| 40A | Embedding cache — wire the [Phase 38](phase-38-overview.md) `ResponseCache` into `embeddings_inner` keyed on `(model_id, input_text)`; flip `CacheableCall::Embedding` (38E) on. The largest RAG-sync cost saver. | Planned (design draft) |
| 40B | Per-item batch caching — an `EmbeddingsData` batch hits/misses per input; cache each input independently and only call the provider for the misses (LiteLLM `async_add_cache_pipeline` + per-item `prompt_tokens_details` distribution). | Planned (design draft) |
| 40C | Rerank cache — wire `rerank_inner` keyed on `(model_id, query, documents)`; flip `CacheableCall::Rerank` on. Deterministic; safe to cache. | Planned (design draft) |
| 40D | RAG sync skip-unchanged — `create_embeddings` consults the embedding cache so re-syncing a doc set re-embeds only changed chunks; surfaces "N/M chunks cached" in sync output. | Planned (design draft) |

## Mapping to the LiteLLM reference

| LiteLLM feature | Source | Phase 40 item |
|---|---|---|
| Embedding response caching | [`caching.py:735`](https://github.com/BerriAI/litellm/blob/main/litellm/caching/caching.py) (`add_embedding_response_to_cache`) | 40A |
| Bulk pipeline embedding write | `caching.py:766` (`async_add_cache_pipeline`) | 40B |
| Per-item `prompt_tokens_details` split | `caching.py:692-733` (`_get_per_item_prompt_tokens_details`) | 40B |
| `supported_call_types` incl. embedding/rerank | `caching.py:70-83` | 40A/40C (consumes 38E) |
| `file`/input-keyed cache key | `caching.py:364` | 40A (input-text key) |

## Design tenets

1. **Auxiliary calls are the *safest* cache, not the riskiest.** Unlike chat (which needs
   `temperature == 0` and no-tools gating), an embedding or rerank under a fixed model is
   deterministic by definition. 40 carries none of 37C's determinism gates — the only gate
   is the 38E call-type allow-list and the per-role/`Cache-Control` opt-out.
2. **Per-input granularity, not per-batch.** LiteLLM caches each element of an embedding
   batch independently (`async_add_cache_pipeline` loops over `kwargs["input"]`). aichat
   does the same: a 100-chunk RAG sync where 98 chunks are unchanged calls the provider for
   2, not 100. This is where the cost actually lives.
3. **Reuse 38, add nothing structural.** 40 is *wiring*, not new cache machinery. The
   backend, control protocol, trace event, and accounting all come from 37/38. 40 flips two
   `CacheableCall` variants on and threads the cache through two more call sites.
4. **The embedding cache is the bridge to L2.** [37F](phase-37-overview.md)'s semantic cache
   needs an embedding of the *query* to do nearest-neighbour lookup. That query embedding is
   itself an `embeddings()` call — so 40A's cache directly cuts 37F's per-lookup cost. The
   two phases compound.

## 40A Design — embedding cache

`embeddings_inner` ([`src/client/common.rs:163`](../../src/client/common.rs)) is the choke
point every provider's embedding call flows through (the `macros.rs:242` generated impls
delegate to it). 40A inserts a `ResponseCache` lookup/store keyed on the embedding
determinants:

```rust
// Sketch — src/client/common.rs, around embeddings_inner
async fn embeddings(&self, data: &EmbeddingsData) -> Result<Vec<Vec<f32>>> {
    let cache = self.response_cache();          // 38A front, scoped to CacheableCall::Embedding
    let key = embedding_key(self.model().id(), &data.texts);   // 38/37C-style hash
    if let Some(entry) = cache.get_for(CacheableCall::Embedding, &key).await {
        trace_cache_lookup("embedding", "hit", &key);          // 37E event
        return Ok(entry.decode_embeddings());
    }
    let out = self.embeddings_inner(data).await?;
    cache.put_for(CacheableCall::Embedding, &key, encode(&out)).await;
    Ok(out)
}
```

The key is `SHA-256(model_id \0 input_text)` per input — same null-delimited construction
as `StageCache::key`. `CacheableCall::Embedding` (38E) gates it; `cache_supported_calls`
in config flips it on (default keeps it off until measured, matching 37's posture).

**Files:** [`src/client/common.rs`](../../src/client/common.rs) (cache wrap around
`embeddings`/`embeddings_inner`), [`src/cache.rs`](../../src/cache.rs) (`embedding_key`,
`CacheEntry::decode_embeddings`), [`src/config/mod.rs`](../../src/config/mod.rs)
(`cache_supported_calls` adds `embedding`).

## 40B Design — per-item batch caching

`EmbeddingsData` carries a batch of `texts`. A naive cache keys the whole batch — but then
adding one new chunk misses the entire batch. LiteLLM solves this by caching each input
independently (`async_add_cache_pipeline`, `caching.py:766`). 40B does the same:

```rust
// Sketch — src/client/common.rs
async fn embeddings(&self, data: &EmbeddingsData) -> Result<Vec<Vec<f32>>> {
    let mut out = vec![None; data.texts.len()];
    let mut misses = Vec::new();                          // (orig_index, text)
    for (i, text) in data.texts.iter().enumerate() {
        match cache.get_for(Embedding, &embedding_key(model, text)).await {
            Some(e) => out[i] = Some(e.decode_one()),
            None => misses.push((i, text.clone())),
        }
    }
    if !misses.is_empty() {
        let fresh = self.embeddings_inner(&EmbeddingsData::from(&misses)).await?;
        for ((i, text), vec) in misses.iter().zip(fresh) {
            out[*i] = Some(vec.clone());
            cache.put_for(Embedding, &embedding_key(model, text), encode_one(&vec)).await;
        }
    }
    Ok(out.into_iter().map(Option::unwrap).collect())
}
```

**Token accounting** (37A): the provider only bills for the `misses`, so `CallMetrics` for a
partially-cached batch reports the real (reduced) input tokens for the missed inputs and
zero for the hits. LiteLLM's `_get_per_item_prompt_tokens_details` (`caching.py:692`)
distributes batch token details across items so the per-item cache entries sum back to the
original total — aichat ports the same distribution so a hit's replayed `usage` is honest.

**Files:** [`src/client/common.rs`](../../src/client/common.rs) (batch split/merge),
[`src/cache.rs`](../../src/cache.rs) (`encode_one`/`decode_one`, per-item token split).

## 40C Design — rerank cache

`rerank_inner` ([`src/client/common.rs:171`](../../src/client/common.rs), used by
[`src/client/cohere.rs:93`](../../src/client/cohere.rs)) takes a `RerankData`
([`src/client/common.rs:383`](../../src/client/common.rs)) — a query + a document list — and
returns a `RerankOutput` (relevance-scored ordering). Deterministic under a fixed model, so
safe to cache, keyed on `SHA-256(model_id \0 query \0 each_doc)`. `CacheableCall::Rerank`
(38E) gates it.

Rerank is lower-frequency than embedding but higher per-call cost (it scores every document
against the query), so a repeated RAG query against an unchanged corpus is a clean win.

**Files:** [`src/client/common.rs`](../../src/client/common.rs) (cache wrap around
`rerank`/`rerank_inner`), [`src/cache.rs`](../../src/cache.rs) (`rerank_key`).

## 40D Design — RAG sync skip-unchanged

`create_embeddings` ([`src/rag/mod.rs:647`](../../src/rag/mod.rs)) batches document chunks
and embeds them on every `.rag rebuild` / document add. With 40A/40B in place,
`create_embeddings` consults the embedding cache per chunk, so a re-sync re-embeds only the
chunks whose text changed. The sync output surfaces the saving:

```
$ aichat --rag mydocs --rebuild
Creating embeddings [3/3]  (847/892 chunks cached, 45 re-embedded)
Saved ~$0.011 and 38s via embedding cache.
```

This is the most user-visible payoff of the phase: RAG rebuilds over a slowly-changing
corpus become near-instant and near-free. It also composes with 39D — a team sharing a
Redis/S3 back tier shares embedding cache entries, so the *first* engineer to sync a doc set
pays; the rest don't.

**Files:** [`src/rag/mod.rs`](../../src/rag/mod.rs) (`create_embeddings` consults cache;
sync-summary line), [`src/client/common.rs`](../../src/client/common.rs) (reuses 40A/40B).

## How 40 composes with prior phases

| Prior phase | Composition |
|---|---|
| **Phase 37A** (accounting) | Embedding/rerank cache hits report `cache_read_tokens` and `cost_saved` through the same `CallMetrics`. |
| **Phase 37E** (trace) | Embedding/rerank lookups emit `cache.lookup` with `layer: L1`, `call_type: embedding|rerank`. |
| **Phase 37F** (semantic L2) | 37F's per-query embedding is a 40A-cached call; 40A cuts 37F's lookup cost. |
| **Phase 38E** (`supported_call_types`) | 40 is the consumer that flips `Embedding`/`Rerank` on. |
| **Phase 39D** (team cache) | Shared back tier → shared embedding cache across machines. |
| **Phase 15-17 RAG** | 40D makes RAG sync incremental without changing the HNSW/BM25 store. |

## Open questions

### 1. Default-on for embeddings, given they're deterministic?

**Question:** Embeddings carry no determinism risk. Ship `cache_supported_calls: [chat,
embedding]` on by default?

**Recommendation: ship embedding-cache *off* by default in 40A, flip on after one release
measured — same discipline as 37.** The correctness risk is zero, but the *staleness* risk
is not: an embedding-model version bump (provider silently upgrades `text-embedding-3`)
changes the vectors, and a cached vector mixed with fresh ones in the same HNSW index is a
silent quality regression. Key the cache on a model *version* where the provider exposes one;
until 40D's trace confirms no version drift, keep it opt-in. (Rerank similarly.)

### 2. Embedding cache key — raw text or normalized?

**Question:** Normalize whitespace/case before hashing the input text?

**Recommendation: hash raw text.** An embedding model treats `"Hello"` and `"hello"` as
different inputs producing different vectors; normalizing the key would serve the wrong
vector. The key must match the exact bytes sent to the provider — same rule as 37C.

### 3. Should the embedding cache live in the RAG store or the response cache?

**Question:** `src/rag/` already has a vector store. Cache embeddings there, or in the 38
`ResponseCache`?

**Recommendation: in the 38 `ResponseCache`.** The RAG store indexes *documents* for search;
the embedding cache memoizes *calls* (including one-off query embeddings that never enter the
doc index). They're different lifecycles. Keeping embeddings in the response cache also means
they inherit 38/39's backends (a team shares embedding cache via Redis) and 37E's trace for
free. The RAG store stays a search index, not a call memo.

## Testing

- **`tests/regression/cache-embeddings.sh`** — bats regression:
  - 40A: `aichat --rag d --rebuild` twice over an unchanged corpus — second rebuild reports
    100% chunks cached and makes zero embedding HTTP calls (assert via a mock provider or
    `cache.lookup` count).
  - 40B/partial: add one doc to a synced corpus, rebuild — only the new chunk's embedding is
    requested; existing chunks are cache hits.
  - 40C: a repeated rerank query over the same corpus returns from cache; `cache.lookup`
    records `call_type: rerank`.
  - 40D: the sync summary line reports the cached/re-embedded counts and a non-zero `Saved`.
  - opt-out: a role with `cache: false` does not cache its RAG embeddings.
- **Rust unit tests** in `src/cache.rs` / `src/client/common.rs`:
  - `tests::embedding_key_varies_by_model_and_text`
  - `tests::batch_embeddings_only_requests_missed_inputs`
  - `tests::batch_token_details_distribute_and_sum_to_total`
  - `tests::rerank_key_covers_query_and_all_docs`
  - `tests::embedding_cache_gated_by_supported_calls`

## Sequencing

- **Blocked by Phase 38** (needs the `ResponseCache` front + 38E `CacheableCall` gate) and
  by **Phase 37A** (accounting) / **37E** (trace) for the saving to be measurable.
- **40A → 40B** — single-input cache first, then the per-item batch split (40B is the
  optimization that makes 40A pay on real RAG workloads).
- **40C (rerank)** is independent of 40A/B and can land in parallel after 38E.
- **40D** lands last — it is the RAG-side consumer of 40A/40B and the user-visible summary.

## Files (consolidated)

- [`src/client/common.rs`](../../src/client/common.rs) — cache wrap around
  `embeddings`/`embeddings_inner` and `rerank`/`rerank_inner`; batch split/merge; token split
- [`src/cache.rs`](../../src/cache.rs) — `embedding_key`, `rerank_key`, per-item encode/decode
- [`src/rag/mod.rs`](../../src/rag/mod.rs) — `create_embeddings` consults cache; sync summary
- [`src/config/mod.rs`](../../src/config/mod.rs) — `cache_supported_calls` gains `embedding`/`rerank`

## References

- [`EVAL-0004-litellm-cache-parity.md`](../analysis/caching/EVAL-0004-litellm-cache-parity.md) §2.10 — embedding/rerank feature map
- [`phase-38-overview.md`](phase-38-overview.md) — the `ResponseCache` front + `CacheableCall` gate this consumes
- [`phase-37-overview.md`](phase-37-overview.md) — accounting (37A), trace (37E), semantic L2 (37F) this composes with
- [`phase-39-overview.md`](phase-39-overview.md) — team back tier that shares embedding entries
- [LiteLLM embedding caching](https://github.com/BerriAI/litellm/blob/main/litellm/caching/caching.py) (`add_embedding_response_to_cache`, `_get_per_item_prompt_tokens_details`)