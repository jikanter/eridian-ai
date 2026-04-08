# Phase 27: RAG — Graph Expansion & Observability

**Status:** Planned
**Epic:** 9 — RAG Evolution
**Design:** [epic-9.md](../analysis/epic-9.md)

---

> Full design: [`docs/analysis/epic-9.md`](../analysis/epic-9.md)

| Item | Status | Notes |
|---|---|---|
| 27A. Chunk-adjacency graph | — | During indexing, extract inter-chunk references (markdown links, import statements, file cross-references) via regex. Store as `graph_edges: Vec<(DocumentId, DocumentId)>` in `RagData`. At search time, 1-hop expansion from seed results. Cap at 2× top_k. ~200 lines. Optional dep: `petgraph` (or simple adjacency list). |
| 27B. RAG trace integration | — | Emit RAG search events in `use_embeddings()`: RAG name, query, result count, per-chunk scores/sources, search method (vector/keyword/graph). Integrates with Phase 8F `--trace`. Extend `.sources rag` with content previews. ~50 lines. Depends on Phase 8F. |

**Parallelization:** 27A and 27B are independent.

**Key files:** `src/rag/mod.rs` (27A graph extraction/expansion), `src/config/input.rs` (27B trace emission), `src/utils/trace.rs` (27B).

**What NOT to build (RAG scope):**

| Proposal | Reason |
|---|---|
| Knowledge graph with entity extraction | LLM calls per chunk during indexing violates cost-conscious constraint. |
| AST-based code dependency graph | `tree-sitter` + grammars add significant binary bloat. Use MCP tool for code intelligence. |
| Semantic chunking (LLM boundaries) | Language-aware `RecursiveCharacterTextSplitter` is 95% as good at zero cost. |
| Query expansion / HyDE | LLM call before retrieval. Build as pipeline stage if needed, not core RAG. |
| External backend integration (ChromaDB, Qdrant) | Built-in HNSW covers CLI-scale workloads. If storage trait emerges from Phase 25 refactoring, external backends become easy to add later. |
| Multi-modal RAG | Different embedding models, storage, retrieval. Different product. |
| Real-time file watching | CLI is invocation-based. Use `cron` or shell loops. |
