# Phase 25: Knowledge Compilation

**Status:** Planned
**Epic:** 9 — Knowledge Evolution (was: RAG Evolution)
**Design:** [epic-9.md](../analysis/epic-9.md)

---

> **[REWRITTEN 2026-04-16]** The old Phase 25 (vector-RAG improvements: sibling expansion,
> metadata chunks, incremental HNSW, binary storage) is killed. AIChat moves away from
> chunk+embedding retrieval toward **knowledge compilation**: inputs are compiled once into
> atomic Entity-Description Pairs (EDPs) tagged with deterministic provenance. Retrieval at
> runtime is tag-filter + BM25 + graph walk — no embeddings in the primary path.
> Research basis: FADER (EDPs + BM25), AEVS (extract-then-restore grounding), Karpathy's
> compiled-KB pattern. Full design: [`docs/analysis/epic-9.md`](../analysis/epic-9.md).

| Item | Status | Notes |
|---|---|---|
| 25A. EDP data model + tag schema | — | New `src/knowledge/edp.rs`. Defines `EntityDescriptionPair { id, entity, description, tags: Vec<Tag>, provenance: SourceAnchor, edges: Vec<EdgeRef> }`. `Tag` is typed (namespace + value, e.g. `kind:rule`, `topic:retrieval`). `SourceAnchor` records `{ path, byte_range, line_range, content_hash }`. Tag schema is declarative (`knowledge.yaml`) and validated at compile time. ~200 lines, foundation for everything else in this phase. |
| 25B. Knowledge compiler | — | New `src/knowledge/compile.rs` + `aichat knowledge compile <inputs>` subcommand. Three-phase pipeline adapted from FADER: (1) question speculation, (2) query-guided EDP extraction, (3) sample augmentation across N runs (default 2) for coverage. LLM-driven at compile time, zero cost at query time. Inputs: markdown, source files, plain text. Output: compiled KB directory (`.aichat/kb/<name>/`). ~350 lines. |
| 25C. AEVS restore-check | — | New `src/knowledge/restore.rs`. Every emitted EDP must be restorable to its source via the deterministic ladder: exact match → fuzzy (Levenshtein < 5%) → schema-normalized (entity canonicalization) → full-text search fallback. Facts failing restoration are rejected at compile time and logged. Cheap hallucination guard; no runtime cost. ~150 lines. |
| 25D. Compiled KB storage + cache integration | — | New `src/knowledge/store.rs`. On-disk format: `facts.jsonl` (one EDP per line), `tags.idx` (tag → fact-id inverted index), `bm25.idx` (BM25 term postings), `edges.jsonl` (explicit graph). Re-compilation is content-addressable per source file — unchanged files skip re-extraction. Uses Phase 10B `src/cache.rs` as the cache layer. Drop-in replacement for the `rag.yaml` format. ~220 lines. |
| 25E. CLI surface | — | `src/cli.rs` + `src/main.rs`. Subcommands: `knowledge compile <inputs> --name <kb>`, `knowledge list` (all compiled KBs), `knowledge stat <kb>` (fact count, tag distribution, per-source coverage), `knowledge show <fact-id>` (one atomic fact with provenance). Deterministic output for `showboat validate`. ~120 lines. |

**Parallelization:**
- 25A lands first (data model is shared foundation).
- 25B and 25D can proceed in parallel after 25A (compiler ↔ store format are orthogonal).
- 25C depends on 25B's extraction pipeline (it's a gate on emission).
- 25E is independent once 25A + 25D exist.

Dependency graph inside the phase: `25A → (25B ‖ 25D) → 25C, 25E`.

**Dependencies (external):**
- **Phase 10B (pipeline stage output cache)** — hard dependency. 25D reuses `src/cache.rs` as the content-addressable cache for compiled KBs. Phase 25 cannot start until 10B lands.
- **Phase 9 (schema fidelity)** — soft dependency. The compiler uses `response_format: json_schema` (9A) or Claude tool-use-as-schema (9B) to guarantee well-formed EDP output. Without them, 25B must retry on malformed JSON (cost overhead).
- **`bm25` crate** — already a dependency (retained from old RAG). Used by 25D's index.

**What is explicitly NOT done in this phase:** no queries (Phase 26), no evolution loop (Phase 27), no embeddings anywhere, no retention of `src/rag/` behavior. The old `src/rag/` module is frozen when 25A lands and deprecated; users on the old format must recompile via `aichat knowledge compile`.

**Key files:**
- new `src/knowledge/mod.rs` (module root)
- new `src/knowledge/edp.rs` (25A)
- new `src/knowledge/compile.rs` (25B)
- new `src/knowledge/restore.rs` (25C)
- new `src/knowledge/store.rs` (25D)
- `src/cache.rs` (25D reuse, comes from Phase 10B)
- `src/cli.rs` + `src/main.rs` (25E subcommands)
- new `knowledge.yaml` config schema (25A)
