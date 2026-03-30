# Phase 16: RAG — Composability

**Status:** Planned
**Epic:** 4 — RAG
**Design:** [epic-4.md](../analysis/epic-4.md)

---

> **[ADDED 2026-03-16]** Makes RAG a first-class composable component: declarable on roles,
> usable in CLI mode, searchable across multiple indices, and invokable as an LLM tool.
> Full design: [`docs/analysis/epic-4.md`](../analysis/epic-4.md)

| Item | Status | Notes |
|---|---|---|
| 16A. Role `rag:` field | — | Add `rag: Option<String>` to Role frontmatter. `use_embeddings()` checks `role.rag()` before `config.rag`. Unlocks RAG in pipelines and CLI roles. Same pattern as Phase 6C `mcp_servers:`. ~60 lines. |
| 16B. Pipeline RAG integration | — | Add `input.use_embeddings()` call in `pipe.rs:run_stage_inner()` before LLM call. Each stage's role determines its RAG (or none). ~3 lines (plus 16A). |
| 16C. CLI RAG mode | — | Fix `use_rag()` to work in CLI mode for pre-existing RAGs. Compose with `-f`, `-r`, `--stage`, `--each`. Require RAG to exist (no interactive creation in CLI). ~40 lines. |
| 16D. Search-only mode | — | `--rag-search` flag bypasses LLM entirely. Calls `Rag::search()`, formats chunks to stdout (text or `-o json`). REPL equivalent: `.rag search` + `.rag ask`. ~80 lines. |
| 16E. Multi-RAG search | — | `rag:` field accepts string or list. CLI accepts multiple `--rag` flags. Search each RAG independently, merge via existing `reciprocal_rank_fusion()`. Cross-model safe (RRF uses rank positions, not scores). ~100 lines. |
| 16F. RAG as LLM tool | — | When `rag_mode: tool`, suppress auto-injection; expose synthetic `search_knowledge` tool instead. LLM decides when to search. Follows Phase 1C `tool_search` pattern. ~80 lines. |

**Parallelization:** 16A is the foundation (16B, 16E, 16F depend on it). 16C and 16D are independent of everything. 16E and 16F are independent of each other but need 16A.

**Recommended order:** 16A → (16B + 16C + 16D in parallel) → (16E + 16F in parallel)

**Key files:** `src/config/role.rs` (16A), `src/pipe.rs` (16B), `src/config/mod.rs` (16C), `src/main.rs` + `src/cli.rs` (16D), `src/config/input.rs` (16E), `src/function.rs` (16F).
