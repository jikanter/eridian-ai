# Epic 9: Knowledge Evolution — From RAG to Compiled Atomic-Fact KB

**Created:** 2026-03-16 (as "RAG Evolution")
**Updated:** 2026-04-16 (fully reshaped: moved from vector-RAG improvements to atomic-fact knowledge compilation; research-driven rewrite)
**Status:** Planning
**Depends on:** Phase 8D (headless RAG — inherited pattern), Phase 8F (interaction trace), Phase 10B (pipeline stage cache), Phase 11C (budget-aware retrieval)

---

## Motivation

AIChat's built-in RAG system (`src/rag/mod.rs`, 1029 lines) is well-engineered — hybrid HNSW+BM25, reciprocal rank fusion, language-aware splitting, optional reranking — but the *primitive* it builds on (embedded text chunks) loses. The 2025–2026 research consensus is converging on three claims this epic accepts as load-bearing:

1. **Atomic facts beat chunks at constrained token budgets.** FADER (arXiv 2503.19574) showed that decomposing text into Entity-Description Pairs (EDPs) and retrieving via plain BM25 beats fixed-size chunks with vectors in low-token regimes — the exact regime AIChat operates in.

2. **Deterministic provenance beats vector neighborhood search.** AEVS (mdpi 2073-431X/15/3/178) demonstrated that grounding every fact back to its source text via a deterministic matching ladder (exact → fuzzy → schema → fallback) is a cheap, robust hallucination guard. GraphRAG implementations with provenance report ~90% hallucination reduction vs vector RAG.

3. **Context curation must be append/patch, not rewrite.** The ACE paper (arXiv 2510.04618) named two anti-patterns — *brevity bias* and *context collapse* — that vector-RAG improvement roadmaps routinely fall into. The prescription is structured incremental updates with detail preservation.

These findings, combined with AIChat's hard cost-conscious constraint and the governing principle that *every token sent to an LLM should be a token only an LLM can process*, make the case for a different primitive. This epic pivots from "improve RAG" to "replace RAG with **knowledge compilation**": pay LLM cost once at compile time to produce a structured, tagged, provenance-grounded fact store; retrieve deterministically at runtime.

The second pivot: compose the fact store with the rest of AIChat as a first-class participant. Roles bind to knowledge bases. Pipelines query per-stage. CLI has a retrieval-only mode. Agents can invoke knowledge as a tool. None of this works if the primitive is a flat chunk.

---

## Guiding Research

| Finding | Source | Impact on this epic |
|---|---|---|
| EDPs + BM25 beat chunks + vectors in low-token regimes | FADER, [arXiv 2503.19574](https://arxiv.org/abs/2503.19574) | Primary retrieval path is BM25 over atomic EDPs; no embeddings. |
| Extract-then-restore grounding as hallucination guard | AEVS, [mdpi 2073-431X/15/3/178](https://www.mdpi.com/2073-431X/15/3/178) | Every compiled fact must pass deterministic restore-check. |
| Structured-incremental over iterative-rewrite | ACE, [arXiv 2510.04618](https://arxiv.org/abs/2510.04618) | Store mutation API is append/patch only; curation is explicit. |
| Compile once, query cheap | Karpathy's compiled-KB pattern | Compilation is a distinct phase cached per Phase 10B. |
| Graph walks reduce hallucination vs vector neighborhoods | GraphRAG industry reports (FalkorDB et al.) | 1-hop graph walk is primary retrieval expansion, not an afterthought. |
| Atomic-fact attribution is trivial | Attributed QA, [arXiv 2410.16708](https://arxiv.org/abs/2410.16708) | Phase 27 ships per-fact citation without extra LLM calls. |

---

## Feature 1: Entity-Description Pair (EDP) Data Model

### Problem

A "chunk" carries no structure, no typing, no provenance beyond `{path, chunk_index}`. It cannot be filtered deterministically, cannot be verified, cannot be cited. Every downstream feature AIChat wants — tag dispatch, graph walks, attribution, reflection — hits a wall at the chunk layer.

### Solution

Replace the chunk as the atomic retrieval unit with an **Entity-Description Pair (EDP)**:

```rust
pub struct EntityDescriptionPair {
    pub id: FactId,                    // deterministic content hash + prefix
    pub entity: String,                // noun phrase, question, or claim anchor
    pub description: String,           // factual content tied to the entity
    pub tags: Vec<Tag>,                // typed: (namespace, value)
    pub provenance: SourceAnchor,      // { path, byte_range, line_range, content_hash }
    pub edges: Vec<EdgeRef>,           // explicit 1-hop edges (link / same-file / shared-entity)
    pub revisions: Vec<RevisionId>,    // append-only mutation history
}
```

Inspired by FADER's EDP construction. Every EDP is individually queryable, filterable, and citable.

### Impact

Unlocks tag dispatch (F4), deterministic restore (F3), explicit graph (F6), attribution (F10). Landed in Phase 25A.

---

## Feature 2: Knowledge Compilation Pipeline

### Problem

The current RAG indexer is a chunker + embedder. Extraction is a side-effect of splitting, not a first-class step. No LLM-driven structure, no cross-document entity canonicalization.

### Solution

A three-phase compiler, adapted from FADER:

1. **Question speculation** — LLM generates plausible queries per input segment, priming extraction toward retrievable shape.
2. **Query-guided EDP extraction** — LLM emits EDPs keyed by the speculated questions. Output is schema-constrained (Phase 9A/9B) so the JSON shape is guaranteed.
3. **Sample augmentation** — repeat N times (default 2), deduplicate via content hash. Mitigates stochasticity of single-shot extraction.

Compiler is a subcommand: `aichat knowledge compile <inputs> --name <kb>`. Output is a directory with `facts.jsonl`, `tags.idx`, `bm25.idx`, `edges.jsonl`, `revisions.jsonl`, `manifest.yaml`.

Per-source-file content hashing means re-compilation is incremental: unchanged files skip re-extraction, reusing the Phase 10B cache.

### Impact

Pays LLM cost once, at compile time. Produces the primitive (F1) that enables everything else. Landed in Phase 25B; storage in Phase 25D.

---

## Feature 3: Deterministic Restore-Check (AEVS Grounding)

### Problem

LLM-extracted facts hallucinate. Without a grounding check, bad facts poison the store and pollute retrieval forever.

### Solution

Every emitted EDP is tested against its claimed source via a deterministic matching ladder:

1. **Exact match** — is the description string literally present in the source range?
2. **Fuzzy match** — Levenshtein distance < 5% of description length.
3. **Schema-normalized match** — canonicalize entity (strip articles, lowercase, whitespace); retry.
4. **Full-text fallback** — search the source file for a matching span.

First success commits the fact; total failure rejects it with a logged reason. Zero runtime cost; one-time compile-time gate.

### Impact

Hallucination guard without an LLM call. Makes the store trustworthy enough to drive attribution (F10). Landed in Phase 25C.

---

## Feature 4: Tag Schema and Tag Dispatch

### Problem

Flat retrieval cannot express "only give me facts about retrieval that are rules, not troubleshooting." There's no typed filter before the similarity step.

### Solution

Tags are typed `(namespace, value)` pairs. The **tag schema** is declared in `knowledge.yaml`:

```yaml
namespaces:
  kind: [rule, fact, example, caveat, decision]
  topic: [retrieval, tools, pipeline, auth, ...]
  source: [docs, code, notes, tests]
```

EDPs carry zero or more tags. Compilation validates against the schema — unknown namespaces/values fail. Query syntax: `kind:rule AND topic:retrieval`. Tag filter narrows the candidate set *before* BM25 runs, cutting ranking work proportionally.

This is the pattern the user's own ACE-formatted KB follows (`[prefix-NNNNN]` tagging in `ClaudeKb/`). Roles in this codebase can reference their own KB sections by tag prefix.

### Impact

Deterministic first-stage filter. Eliminates a class of false positives that similarity search cannot avoid. Landed in Phase 25A (schema) and Phase 26A (dispatch).

---

## Feature 5: BM25 over EDPs (no embeddings)

### Problem

Embeddings carry fixed indexing cost (API calls, wall time) and per-call inference cost when remote. They produce stale indices. They fail at exact-match, version-sensitive, or low-resource-language queries.

### Solution

BM25 over the EDP set, narrowed by tags. FADER's empirical result: at constrained token budgets, BM25 over atomic facts outperforms dense retrieval over chunks.

Existing `bm25` crate (already a dependency) powers the index. Per-term postings live in `bm25.idx`. Index rebuild is CPU-only, fast, and happens at every `knowledge compile` (no staleness).

Optional reranking remains available via the existing `rerank` Client trait — AIChat does not ship a built-in reranker.

### Impact

Zero embedding cost. Zero staleness. Deterministic reproducibility. Landed in Phase 26A.

---

## Feature 6: Explicit-Edge Graph + 1-hop Walk

### Problem

Semantic neighbors in vector space are probabilistic and opaque. "Why was this chunk retrieved?" has no tractable answer.

### Solution

During compilation, extract **explicit edges** — deterministic, regex-findable relations:

- Markdown link target (file A links to file B)
- Shared source file (two EDPs extracted from the same file)
- Shared canonical entity (two EDPs name the same normalized entity)

Stored in `edges.jsonl` as `(from_fact_id, to_fact_id, edge_kind, weight)`.

At query time: after BM25 returns seed facts, expand 1-hop along edges, cap at 2× seed count, re-rank the expanded set via RRF against the original query. This makes "why retrieved" answerable by citing the edge.

### Impact

Graph walks become the primary expansion mechanism, not an optional afterthought (as in the old Phase 27A). Explainable retrieval. Landed in Phase 26B.

---

## Feature 7: Composability — Role Binding, CLI, Pipeline, LLM-Tool Mode

### Problem

The old RAG was REPL/agent-only. Roles couldn't bind to a RAG, pipelines couldn't use one, CLI mode was broken.

### Solution

Identical in shape to the old Phase 26 plan, rewritten on the knowledge primitive:

- **Role frontmatter**: `knowledge: my-kb` | `knowledge: [kb-a, kb-b]` | `knowledge: [{name, tags, weight}]`
- **CLI**: `--knowledge my-kb` (repeatable). Composes with `-f`, `-r`, `--stage`, `--each`.
- **Pipeline**: `use_knowledge()` fires in `pipe.rs:run_stage_inner()` per stage.
- **LLM-tool mode**: `knowledge_mode: tool` suppresses auto-injection and exposes a `search_knowledge` synthetic tool; follows Phase 1C `tool_search` pattern.
- **Search-only mode**: `aichat --knowledge-search "query"` prints facts, no LLM call.
- **Multi-KB**: query each KB independently, fuse via existing RRF.

### Impact

Knowledge becomes a first-class AIChat resource alongside MCP servers, tools, and roles. Landed in Phase 26C–F.

---

## Feature 8: Append/Patch-Only Mutation (ACE Anti-Collapse)

### Problem

The ACE paper names *context collapse* as a recurring failure mode: iterative "improve the KB" passes erode detail over time. A naive "rebuild KB" loop hits this directly.

### Solution

The store API exposes only:

- `append_fact(edp)` — add a new fact
- `patch_fact(id, patch)` — modify a field (description, tags, edges); records a revision
- `deprecate_fact(id, reason)` — mark superseded; remains queryable via `--include-deprecated`

No `replace_all`, no `truncate`, no in-place rewrite. Every mutation records a line in `revisions.jsonl`.

### Impact

Detail is structurally preserved. ACE's anti-collapse prescription enforced at the API layer. Landed in Phase 27A.

---

## Feature 9: ACE Generation/Reflection/Curation Loop

### Problem

A KB built once at compile time decays as the source corpus evolves. Errors in extraction persist. Retrieval failures go unlogged.

### Solution

Wire the ACE cycle into AIChat as explicit subcommands:

- **Generator**: implicit in Phase 25B compilation.
- **Reflector**: `aichat knowledge reflect <kb>` — runs a user-defined role (`ace_role: reflector`) over traced retrieval failures/misses (27C). Output: candidate patches/additions.
- **Curator**: `aichat knowledge curate <kb>` — runs the Curator role (`ace_role: curator`) over candidate patches; accepted patches are applied via the append/patch API (F8).

Both roles are user-customizable — the loop ships with defaults but is extensible per-project.

### Impact

The KB evolves deterministically, under user control, with a full audit trail. Landed in Phase 27B.

---

## Feature 10: Trace Integration and Attributed Output

### Problem

Users can't see which facts drove a response. Attribution in LLM output is post-hoc and expensive (another LLM call to extract citations).

### Solution

Two integrations:

- **Trace (Phase 8F)**: `emit_knowledge_query` records the full retrieval pipeline (KB, query, tag filter, candidates, seeds, expanded set, post-RRF ranks, fact IDs). Surfaces via `--trace` and `.sources knowledge`.
- **Attributed output**: when role declares `attributed_output: true`, retrieved facts are prefixed with `[[fact-id]]` markers; the LLM is instructed to retain them; post-processing expands markers into a provenance table. No extra LLM call — uses the deterministic source anchors from F1.

### Impact

Every claim in the final output links to a compile-time-grounded source range. Landed in Phase 27C–D.

---

## Cross-Feature Dependency Graph

```
F1 (EDP model) ─┬─ foundation for everything ──
F2 (compiler) ──┤
F3 (restore) ───┴─ gates F2 emission ──────────
F4 (tags) ──────── extends F1, consumed by F5
F5 (BM25) ──────── consumes F1, F4
F6 (graph) ──────── consumes F1, extends F5
F7 (composability) ─ consumes F5/F6
F8 (mutation) ───── foundation for F9
F9 (ACE loop) ───── consumes F2, F8, F10
F10 (trace+attr) ── consumes F1 (provenance), F7
```

**Phase mapping:**

| Phase | Features | Focus |
|---|---|---|
| **25 Knowledge Compilation** | F1, F2, F3, F4 (schema), storage | Build the compiled KB primitive. |
| **26 Knowledge Query** | F5, F6, F7 | Deterministic retrieval + composability. |
| **27 Evolution & Trace** | F8, F9, F10 | Mutation discipline, curation loop, attribution. |

**Maximum parallelism (within a phase):**
- Phase 25: `25A → (25B ‖ 25D) → 25C, 25E` — four work streams after 25A.
- Phase 26: `26C → (26A ‖ 26B) → (26D ‖ 26E ‖ 26F)` — three after the query core.
- Phase 27: `27A → (27B ‖ 27C ‖ 27D)` — three after mutation API.

---

## Migration from Vector RAG

The old `src/rag/` module is **deprecated when Phase 25A lands** and removed at the end of Phase 27. No coexistence — users upgrade by running `aichat knowledge compile` against their old `document_paths`. A `knowledge import-rag <old-rag-path>` helper ships with Phase 25E to translate existing RAG configs into knowledge compilation manifests where possible (metadata and paths only; chunks are discarded — the compiler re-extracts EDPs from source).

Breaking changes that ship with the epic:

| Breaking change | Affected | Mitigation |
|---|---|---|
| `rag.yaml` → `knowledge.yaml` (different schema) | REPL users with saved RAGs | `knowledge import-rag` auto-translates paths and name. |
| `config.rag` role field → `knowledge` | Role authors | Compatibility shim reads both fields for one minor release, emits deprecation warning. |
| `.rag` REPL commands → `.knowledge` | REPL users | Alias `.rag` to `.knowledge` with deprecation notice for one minor release. |
| Embedding client config entries | Users of embedding-only providers | Embedding clients remain available for users who still want them (e.g., integrating external tools); AIChat just doesn't use them internally. |

---

## What NOT to Build in This Epic

| Proposal | Reason |
|---|---|
| Retain vector embeddings as an optional primary backend | Violates the research-driven primitive choice. Reintroduces staleness, per-call cost, and opaque retrieval. Can be revisited post-epic if users prove the need. |
| Automatic continuous reflection loop (daemon) | Costs tokens continuously; violates cost-conscious hard constraint. Reflection is user-invoked. |
| External backend integration (Qdrant, Chroma, Neo4j, ChromaDB) | The compiled KB is a plain directory. Users who need an external backend export via `knowledge export` and import elsewhere. AIChat's job is compile + query, not storage ops. |
| Semantic (LLM-boundary) chunking | EDP extraction (F2) is already a semantic atomization. Chunking is obsolete in this architecture. |
| AST-based code dependency graph | `tree-sitter` + grammars → significant binary bloat. Explicit edges (F6) cover enough relationships at zero cost; code dependency graphs live in an MCP tool if needed. |
| Query expansion / HyDE inside `src/knowledge/` | If a user wants HyDE, they build it as a pipeline stage feeding `--knowledge-search`. Not core. |
| Embedding-based rerankers built-in | The `rerank` Client trait remains; users plug in a remote reranker. AIChat doesn't ship one. |
| Real-time file watching (fsnotify) | AIChat is invocation-based. Recompile via cron or a pre-commit hook. |
| Multi-modal KB (image/audio) | Different product. Different extractors, different matching, different retrieval. Out of scope. |
| Distributed / sharded storage | If you need this, use Qdrant/Milvus. AIChat is the wrong layer. |
| Evaluation framework / benchmark harness | Development tool, not runtime feature. Use external benchmarks or build a dedicated subcommand later. |

---

## Relationship to Existing Roadmap

| Feature | Existing Phase | Relationship |
|---|---|---|
| F1 (EDP model) | None | **New** — replaces the chunk primitive. |
| F2 (compiler) | None | **New** — replaces the indexer. |
| F3 (restore-check) | None | **New** — no parallel in old RAG. |
| F4 (tag schema) | None | **New** — first-class typing for retrieval filters. |
| F5 (BM25 over EDPs) | Old RAG had BM25 as one leg of hybrid | **Narrowed** — BM25 is *the* path, not one leg. |
| F6 (explicit graph) | Old Phase 27A (chunk-adjacency) | **Upgraded** — moves from afterthought to primary path; uses deterministic edges, not extracted references. |
| F7 (composability) | Old Phase 26 | **Preserved in shape, rewritten against new primitive.** |
| F8 (mutation API) | None | **New** — ACE anti-collapse. |
| F9 (ACE loop) | None | **New** — first-class curation subcommands. |
| F10 (trace + attr) | Old Phase 27B (trace only) | **Extended** — adds attributed output on top of trace. |

---

## Success Metrics

| Metric | Current State (vector RAG) | Target (compiled KB) |
|---|---|---|
| Retrieval reproducibility | Probabilistic (embedding version drift) | Deterministic (same query → same ranks, always) |
| Hallucination rate on retrieved facts | Unmeasured | <5% (AEVS restore-check gates every fact) |
| Per-call embedding cost | 1 embedding API call per query | 0 |
| Index rebuild cost on source change | Full re-embed of changed files | LLM extraction *only* for changed files (cached via 10B) |
| "Why was this retrieved?" answerability | Opaque | Tag filter + BM25 terms + explicit edge — fully citable |
| Output attribution | Requires extra LLM call | Free (deterministic markers, post-processed) |
| Binary size impact | HNSW + embedding clients retained | HNSW removable after 27 ships, shrinks binary |
