# Phase 25: RAG — Structured Retrieval

**Status:** Planned
**Epic:** 9 — RAG Evolution
**Design:** [epic-9.md](../analysis/epic-9.md)

---

> **[ADDED 2026-03-16]** Transforms RAG from flat chunk retrieval to structure-aware search.
> The built-in RAG is already well-engineered (hybrid HNSW+BM25, RRF, language-aware splitting).
> These changes improve retrieval quality and operational performance without adding LLM cost.
> Full design: [`docs/analysis/epic-9.md`](../analysis/epic-9.md)

| Item | Status | Notes |
|---|---|---|
| 25A. Sibling chunk expansion | — | Record `prev_sibling`/`next_sibling` per chunk during indexing. At search time, expand top-k by including adjacent chunks from same file. Deduplicate, re-rank, truncate to budget. ~50 lines, zero new deps. |
| 25B. Metadata-enriched chunks | — | Populate `RagDocument.metadata` during splitting: heading hierarchy (Markdown), function/class name (code), line range, chunk index. Prefix chunks with source attribution in assembled context. ~95 lines. |
| 25C. Incremental HNSW insertion | — | On append-only sync: `hnsw.parallel_insert(&new_points)` instead of rebuild. Full rebuild only on deletion. BM25 always rebuilds (CPU-only, fast). Add `.rag add <path>`, `.rag rm <path>`, `.rag sync` commands. ~160 lines. |
| 25D. Binary vector storage | — | Split `rag.yaml` into metadata YAML + binary `rag.bin` sidecar. Eliminates Base64 serialization bottleneck. Backward compatible (falls back to YAML if no `.bin`). New dep: `bytemuck` (tiny, zero-copy cast). ~80 lines. |

**Parallelization:** All 4 items are independently implementable. 25A and 25B modify the splitter/search paths. 25C modifies the sync path. 25D modifies the storage path. No conflicts.

**Key files:** `src/rag/mod.rs` (all items), `src/rag/splitter/mod.rs` (25B), `src/rag/serde_vectors.rs` (25D legacy), `Cargo.toml` (25D).
