# Phase 15: RAG — Structured Retrieval

**Status:** Planned
**Epic:** 4 — RAG
**Design:** [epic-4.md](../analysis/epic-4.md)

---

> **[ADDED 2026-03-16]** Transforms RAG from flat chunk retrieval to structure-aware search.
> The built-in RAG is already well-engineered (hybrid HNSW+BM25, RRF, language-aware splitting).
> These changes improve retrieval quality and operational performance without adding LLM cost.
> Full design: [`docs/analysis/epic-4.md`](../analysis/epic-4.md)

| Item | Status | Notes |
|---|---|---|
| 15A. Sibling chunk expansion | — | Record `prev_sibling`/`next_sibling` per chunk during indexing. At search time, expand top-k by including adjacent chunks from same file. Deduplicate, re-rank, truncate to budget. ~50 lines, zero new deps. |
| 15B. Metadata-enriched chunks | — | Populate `RagDocument.metadata` during splitting: heading hierarchy (Markdown), function/class name (code), line range, chunk index. Prefix chunks with source attribution in assembled context. ~95 lines. |
| 15C. Incremental HNSW insertion | — | On append-only sync: `hnsw.parallel_insert(&new_points)` instead of rebuild. Full rebuild only on deletion. BM25 always rebuilds (CPU-only, fast). Add `.rag add <path>`, `.rag rm <path>`, `.rag sync` commands. ~160 lines. |
| 15D. Binary vector storage | — | Split `rag.yaml` into metadata YAML + binary `rag.bin` sidecar. Eliminates Base64 serialization bottleneck. Backward compatible (falls back to YAML if no `.bin`). New dep: `bytemuck` (tiny, zero-copy cast). ~80 lines. |

**Parallelization:** All 4 items are independently implementable. 15A and 15B modify the splitter/search paths. 15C modifies the sync path. 15D modifies the storage path. No conflicts.

**Key files:** `src/rag/mod.rs` (all items), `src/rag/splitter/mod.rs` (15B), `src/rag/serde_vectors.rs` (15D legacy), `Cargo.toml` (15D).
