# Phase 26: Knowledge Query & Composability

**Status:** Planned
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
| 26A. Tag-filter + BM25 query core | — | New `src/knowledge/query.rs`. Pipeline: `tags? → BM25(facts) → topN seeds`. Tag filter is AND of `(namespace:value)` predicates (e.g. `kind:rule AND topic:retrieval`); narrows candidate set *before* BM25 ranks, cutting work. Returns `Vec<FactHit { fact, score, source }>`. Budget-aware (accepts max tokens; truncates at EDP boundary). ~180 lines. |
| 26B. Explicit-edge graph walk | — | New `src/knowledge/graph.rs`. 1-hop expansion from seed facts along edges declared at compile time (markdown link target, shared source file, shared canonical entity). Expansion is capped at 2× seed count; re-ranked via RRF against original query. Primary retrieval path per GraphRAG research (not an afterthought). ~140 lines. |
| 26C. Role `knowledge:` frontmatter field | — | `src/config/role.rs`. New `knowledge: Option<KnowledgeBinding>` field; serde-untagged to accept `String`, `Vec<String>`, or full `Vec<{name, tags?, weight?}>`. Same pattern as Phase 6C `mcp_servers:`. Unlocks KB in roles, pipelines, CLI. ~80 lines. |
| 26D. Pipeline + CLI integration | — | `src/config/input.rs` gains `use_knowledge()` (replaces `use_embeddings()`). Called in `src/pipe.rs:run_stage_inner()` before each LLM call. `src/cli.rs` accepts `--knowledge <kb>` flags (repeatable). Composes with `-f`, `-r`, `--stage`, `--each`. CLI requires KB to exist (no interactive creation). ~100 lines. |
| 26E. Search-only & LLM-tool modes | — | `--knowledge-search "query"` bypasses the LLM, prints ranked facts (text or `-o json`). `knowledge_mode: tool` suppresses auto-injection and exposes a synthetic `search_knowledge` tool (query + optional tags); LLM decides when to invoke. Follows Phase 1C `tool_search` pattern exactly. Extends `src/function.rs` dispatch. ~160 lines. |
| 26F. Multi-KB search via RRF | — | When a role or CLI references multiple KBs, query each independently and fuse via existing `reciprocal_rank_fusion()`. Weights per KB supported. Cross-KB safe (RRF uses ranks, not raw BM25 scores). ~60 lines (RRF already exists from old RAG). |

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
