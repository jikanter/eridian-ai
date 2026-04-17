# Phase 27: Knowledge Evolution, Attribution & Trace

**Status:** Planned
**Epic:** 9 — Knowledge Evolution (was: RAG Evolution)
**Design:** [epic-9.md](../analysis/epic-9.md)

---

> **[REWRITTEN 2026-04-16]** The old Phase 27 (chunk-adjacency graph, RAG trace) is rewritten
> against the knowledge compilation foundation. Graph walk moved into Phase 26 as primary
> retrieval. This phase adds the **ACE generation/reflection/curation loop**, enforces
> append/patch-only mutation (per ACE's anti-collapse prescription), wires trace integration,
> and surfaces per-fact attribution in LLM output.
> Research basis: ACE paper (arxiv 2510.04618), Attributed QA atomic decomposition (arxiv 2410.16708).
> Full design: [`docs/analysis/epic-9.md`](../analysis/epic-9.md).

| Item | Status | Notes |
|---|---|---|
| 27A. Append/patch-only mutation API | — | `src/knowledge/store.rs` exposes only `append_fact`, `patch_fact`, `deprecate_fact` — no `replace_all` or `truncate`. Every mutation records a revision entry in `revisions.jsonl` (id, op, timestamp, reason). Directly enforces ACE's anti-collapse prescription: no iterative rewrites that erode detail. Deprecated facts remain queryable via `--include-deprecated`. ~120 lines. |
| 27B. ACE generation/reflection/curation | — | New `src/knowledge/evolve.rs` + subcommands. `knowledge reflect <kb>`: run the Reflector role over retrieval failures/misses logged in 27C traces; emit candidate patches. `knowledge curate <kb>`: run the Curator role over the candidate set; append accepted facts via 27A's API. Both roles are user-customizable via role frontmatter (`ace_role: reflector | curator`). The Generator role is already implicit in 25B compilation. ~200 lines. |
| 27C. Trace integration | — | Extend `src/utils/trace.rs` with `emit_knowledge_query()`: records KB name, query, tag filter, candidate count, seed fact IDs, expanded fact IDs, post-RRF ranks. Integrates with Phase 8F `--trace`. Extends `.sources knowledge` REPL command with content previews (first 200 chars per fact). ~80 lines. |
| 27D. Attributed output (per-fact citations) | — | `src/config/input.rs` + `src/client/common.rs`. When `attributed_output: true` in role frontmatter, the retrieved facts are prefixed with inline markers (`[[fact-id]]`) the LLM is instructed to carry through. Post-processing annotates the final output with provenance tables. No additional LLM call — uses the deterministic source anchors from 25A. ~120 lines. |

**Parallelization:**
- 27A lands first (mutation discipline gates everything else).
- After 27A: 27B, 27C, 27D are independent and can proceed in parallel.
- Recommended order: `27A → (27B ‖ 27C ‖ 27D)`.

**Dependencies (external):**
- **Phase 25 (knowledge compilation)** — hard dependency. Evolution mutates the fact store Phase 25 produces.
- **Phase 26 (knowledge query)** — hard dependency. 27B's Reflector analyzes real retrieval failures logged by 27C, which requires 26A/26B to be live; 27D's attribution formatting runs on retrieved fact sets from 26.
- **Phase 8F (interaction trace)** — hard dependency for 27C. Without the trace emission infrastructure, RAG events have nowhere to surface.
- **Phase 9C (schema validation retry)** — soft dependency for 27B. The Reflector emits structured JSON patches; 9C's retry loop recovers from transient malformed output.

**What NOT to build in this epic:**

| Proposal | Reason |
|---|---|
| Vector embeddings in the primary path | Research (FADER, AEVS, GraphRAG numbers) shows atomic-fact + BM25 + graph walk dominates at the token budgets AIChat cares about. Adding vectors back as an optional backend is a post-epic decision. |
| Automatic (LLM-driven) reflection loop that runs without user invocation | Costs tokens continuously; violates cost-conscious constraint. `knowledge reflect` is user-invoked, cached, and auditable. |
| Distributed / external KB backends (Qdrant, Chroma, Neo4j) | The compiled KB format is a plain directory — users who need an external backend can export via `knowledge export` and import elsewhere. AIChat's job is compile + query, not storage operations. |
| Embedding-based rerankers | The existing `rerank` Client trait still applies to 26A's BM25 output for users who want to plug in a remote reranker. Built-in reranking via embeddings stays out. |
| Query expansion / HyDE inside `src/knowledge/` | If users want HyDE, they build it as a pipeline stage feeding `--knowledge-search`. Not core. |
| Semantic (LLM-boundary) chunking on the input side | 25B's LLM-driven EDP extraction already solves this — each EDP is a semantic atom by construction. No chunker needed. |
| Real-time file watching / daemon mode | AIChat is invocation-based. `knowledge compile` on a cron or pre-commit hook is the supported pattern. |

**Key files:**
- `src/knowledge/store.rs` (27A — mutation API restrictions)
- new `src/knowledge/evolve.rs` (27B)
- `src/utils/trace.rs` (27C — `emit_knowledge_query()`)
- `src/config/input.rs` (27C trace, 27D attribution injection)
- `src/client/common.rs` (27D — post-processing pass for `[[fact-id]]` markers)
- `src/cli.rs` + `src/main.rs` (27B subcommands: `knowledge reflect`, `knowledge curate`)
- `src/repl/mod.rs` (27C — `.sources knowledge` preview)
