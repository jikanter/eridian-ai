# Phase 27: Knowledge Evolution, Attribution & Trace

**Status:** Done
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
| 27A. Append/patch-only mutation API | Done | `src/knowledge/store.rs` exposes `append_fact`, `append_fact_with_reason`, `patch_fact`, `deprecate_fact` — no `replace_all`/`truncate`. Every mutation records a `RevisionEntry { id, op, timestamp, reason }` in `revisions.jsonl`. Deprecated facts stay on disk and surface again via `QueryOptions.include_deprecated`/`RetrievalOptions.include_deprecated`. 6 unit tests. |
| 27B. ACE reflect/curate subcommands | Done | New `src/knowledge/evolve.rs` + `--knowledge-reflect <kb>` / `--knowledge-curate <kb>` CLI flags. Reflector emits a `CandidateSet` JSON (append/patch/deprecate) validated against a schema. Curator consumes candidates (user-supplied via `--knowledge-candidates` or auto-generated) and accepts/rejects each; accepted entries commit through the 27A mutation API. `--knowledge-trace` lets users feed retrieval-failure JSONL to the Reflector. User-defined roles named `*-reflector` or `*-curator` override the defaults. 6 unit tests. |
| 27C. Trace integration | Done | `KnowledgeQueryEvent` + `TraceEmitter::emit_knowledge_query()` in `src/utils/trace.rs`. `retrieve_from_bindings_traced()` threads a `TraceEmitter` through per-binding retrieval. `Input::use_knowledge()` emits events when `--trace`/`AICHAT_TRACE` is active and caches last events/hits on `Config` for `.sources knowledge` REPL command (with fact-description previews, 200-char cap). 1 unit test + covered by existing retrieve tests. |
| 27D. Attributed output (per-fact citations) | Done | `Role::attributed_output` frontmatter field (default false). `format_hits_for_attributed_injection()` in `src/knowledge/query.rs` surfaces `[[fact-id]]` markers with carry-through instructions. `annotate_output_with_provenance()` walks the LLM response, collects unique cited ids in order, and appends a deterministic `Sources:` table citing path + line range. No extra LLM call. 4 unit tests. |

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
