# Phase 25: Knowledge Compilation

**Status:** Done
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
| 25A. EDP data model + tag schema | Done | New `src/knowledge/{mod,edp,tags}.rs`. `EntityDescriptionPair { id, entity, description, tags, provenance, edges }`; content-hashed `FactId` (`fact-<16 hex>`); `SourceAnchor` with byte_range + line_range + content_hash. `Tag { namespace, value }` serializes as compact `"ns:value"` string; `TagSchema` loads from `knowledge.yaml` with insertion-order preservation and validates namespace+value on emission. 24 unit tests. |
| 25B. Knowledge compiler | Done | New `src/knowledge/compile.rs`. Split into testable pure core (`commit_candidates`, `line_range_to_bytes`, `dedupe_candidates`, `parse_llm_response`) and async orchestrator (`compile_file` / `compile_files`). Per-file pipeline: manifest hash-check → StageCache hit → N-sample LLM extraction with `response_format: json_schema` (Phase 9A/9B native) → dedup by `(entity, description)` → `commit_candidates` applies restore-check gate and commits. **Scope-deferred** from the original plan: FADER "question speculation" step, edge extraction, parallel per-file compilation (all noted for Phase 26/27 pickup). 16 unit tests. |
| 25C. AEVS restore-check | Done | New `src/knowledge/restore.rs`. Deterministic matching ladder: `Exact` → `WhitespaceTolerant` → `SchemaNormalized` (lowercase + strip punctuation + strip leading articles) → `TokenOverlap` (≥70% unique-token recall). The "Fuzzy" Levenshtein step from the original design was replaced with whitespace + schema normalization — same coverage at linear cost. `restore_check(description, source)` returns `RestoreOutcome { strategy, matched_byte_range }` or `None`; `check_fact` wraps it with a fact-id-tagged error. 16 unit tests. |
| 25D. Compiled KB storage + cache integration | Done | New `src/knowledge/store.rs`. On-disk layout: `manifest.yaml` (name + per-source content hashes + fact count), `facts.jsonl` (EDPs, edges stripped), `edges.jsonl` (authoritative graph), optional `knowledge.yaml` (tag schema). Write-to-tmp + rename for atomic `save()`. Content-addressability lives in the manifest (`needs_recompile(path, hash)`); full StageCache integration deferred to 25B where per-file LLM extraction results are cached. `tag_index()` and `outbound_edges()` rebuilt on demand. `append_fact` validates tags against the schema when one is present; `remove_facts_by_source` used for recompiling changed sources. 15 unit tests. |
| 25E. CLI surface | Done | `src/cli.rs` + `src/main.rs` + `src/knowledge/cli.rs`. Flags (not subcommands — matches existing style): `--knowledge-compile <kb> -f <file>...`, `--knowledge-list`, `--knowledge-stat <kb>`, `--knowledge-show <kb>:<fact-id>`. KB dir = `<config_dir>/kb/<name>/`. Read-only ops (list/stat/show) added to `info_flag` so they skip heavy startup. Deterministic output suitable for `showboat validate`. 4 unit tests. |

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
