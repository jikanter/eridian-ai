# Phase 26: Knowledge Query & Composability

**Status:** Done
**Epic:** 9 — Knowledge Evolution (was: RAG Evolution)
**Design:** [epic-9.md](../analysis/epic-9.md)

---

> **[REWRITTEN 2026-04-16]** The old Phase 26 (role `rag:` field, pipeline RAG, CLI RAG, multi-RAG,
> RAG-as-tool) is rewritten against the knowledge compilation foundation from Phase 25. Retrieval
> is deterministic: tag filter narrows candidates, BM25 ranks, a 1-hop graph walk on explicit
> edges expands seeds. No embeddings, no vector neighborhood search. Composability surface
> (roles, pipelines, CLI, LLM-tool mode) is identical in shape to the old plan — only the
> retrieval substrate changes. Full design: [`docs/analysis/epic-9.md`](../analysis/epic-9.md).

| Item | Status | Notes |
|---|---|---|
| 26A. Tag-filter + BM25 query core | Done | `src/knowledge/query.rs`. `filter_by_tags` (AND-joined predicates) → `bm25_rank` (top-K) → `apply_budget` (EDP-boundary truncation). `FactHit { fact, score, source }`. `format_hits_for_injection` + `hits_to_json` formatting helpers. 16 unit tests. |
| 26B. Explicit-edge graph walk | Done | `src/knowledge/graph.rs`. `one_hop_neighbors` (dedup, excludes seeds), `reciprocal_rank_fusion` (RRF k=60, weighted), `expand_and_fuse` (cap = 2× seed count, seeds weighted 1.5× expansion's 1.0×). Primary retrieval path. 11 unit tests. |
| 26C. Role `knowledge:` frontmatter field | Done | `src/config/role.rs`. `KnowledgeBinding { name, tags, weight }`. Parses String / Vec&lt;String&gt; / Vec&lt;Object&gt; frontmatter shapes. Exports in the most compact round-trippable form. Plus `knowledge_mode: Option<String>` for 26E switching. 9 unit tests. |
| 26D. Pipeline + CLI integration | Done | `Input::use_knowledge()` (sync — retrieval is local disk I/O). Wired into `main.rs` non-pipeline path and `pipe.rs:run_stage_inner`. Phase 10B cache key updated to hash *post-injection* text so KB changes invalidate cached stages. `--knowledge <name>` CLI flag (repeatable) merges with role bindings at retrieval time. `src/knowledge/retrieve.rs` is the orchestrator. |
| 26E. Search-only & LLM-tool modes | Done | `--knowledge-search "query"` + `--knowledge <kb>` (repeatable) prints ranked facts as text (default) or JSON (`-o json`). `knowledge_mode: tool` on a role suppresses auto-injection and exposes a synthetic `search_knowledge` tool via `select_functions`; tool calls dispatch to `ToolCall::eval_search_knowledge` and return JSON hits. |
| 26F. Multi-KB search via RRF | Done | Folded into 26D's `retrieve_from_bindings`. Each binding runs its own tag+BM25+graph-expand pipeline; results fuse via `reciprocal_rank_fusion` weighted by `binding.weight`. Same-id facts across KBs dedupe via the per-id best-score map. 5 unit tests. |

**Parallelization:**
- 26C is the foundation — 26D, 26E, 26F depend on it.
- After 26C: 26A (query core) and 26B (graph walk) are independent; 26D, 26E, 26F are mostly independent of each other but all consume 26A+26B.
- Recommended order: `26C → (26A ‖ 26B) → (26D ‖ 26E ‖ 26F)`.

**Dependencies (external):**
- **Phase 25 (knowledge compilation)** — hard dependency. This phase queries the artifacts Phase 25 produces. Cannot begin until 25A/25B/25D land.
- **Phase 11A (context budget allocator)** — hard dependency for 26A's budget-aware truncation. The `ContextBudget` helper from `src/context_budget.rs` supplies the remaining-tokens number that 26A respects at retrieval time (replaces the "Phase 11C" role the old plan had — 11C was superseded; see [phase-11](./phase-11-context-budget.md)).
- **Phase 1C (deferred tool loading)** — pattern reference for 26E's synthetic `search_knowledge` tool; no code dependency, but the shape is borrowed directly.
- **Phase 8D (headless RAG)** — fixes the "RAG bails in CLI mode" issue; 26D inherits the same fix in knowledge-flavored form.

**Config:**
```yaml
# knowledge.yaml — per-KB binding config
name: codebase-docs
mode: inject          # inject (default) | tool
top_k: 8              # before graph expansion
graph_expand: true    # 1-hop walk on seeds
```

**Key files:**
- new `src/knowledge/query.rs` (26A)
- new `src/knowledge/graph.rs` (26B)
- `src/config/role.rs` (26C — `knowledge:` frontmatter)
- `src/config/input.rs` (26D — `use_knowledge()` replaces `use_embeddings()`)
- `src/pipe.rs` (26D — pipeline stage integration)
- `src/cli.rs` + `src/main.rs` (26D — `--knowledge`, 26E — `--knowledge-search`)
- `src/function.rs` (26E — `search_knowledge` tool dispatch)
