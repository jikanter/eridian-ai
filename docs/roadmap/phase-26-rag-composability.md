# Phase 26: RAG — Composability

**Status:** Planned
**Epic:** 9 — RAG Evolution
**Design:** [epic-9.md](../analysis/epic-9.md)

---

> **[ADDED 2026-03-16]** Makes RAG a first-class composable component: declarable on roles,
> usable in CLI mode, searchable across multiple indices, and invokable as an LLM tool.
> Full design: [`docs/analysis/epic-9.md`](../analysis/epic-9.md)

| Item | Status | Notes |
|---|---|---|
| 26A. Role `rag:` field | — | Add `rag: Option<String>` to Role frontmatter. `use_embeddings()` checks `role.rag()` before `config.rag`. Unlocks RAG in pipelines and CLI roles. Same pattern as Phase 6C `mcp_servers:`. ~60 lines. |
| 26B. Pipeline RAG integration | — | Add `input.use_embeddings()` call in `pipe.rs:run_stage_inner()` before LLM call. Each stage's role determines its RAG (or none). ~3 lines (plus 26A). |
| 26C. CLI RAG mode | — | Fix `use_rag()` to work in CLI mode for pre-existing RAGs. Compose with `-f`, `-r`, `--stage`, `--each`. Require RAG to exist (no interactive creation in CLI). ~40 lines. |
| 26D. Search-only mode | — | `--rag-search` flag bypasses LLM entirely. Calls `Rag::search()`, formats chunks to stdout (text or `-o json`). REPL equivalent: `.rag search` + `.rag ask`. ~80 lines. |
| 26E. Multi-RAG search | — | `rag:` field accepts string or list. CLI accepts multiple `--rag` flags. Search each RAG independently, merge via existing `reciprocal_rank_fusion()`. Cross-model safe (RRF uses rank positions, not scores). ~100 lines. |
| 26F. RAG as LLM tool | — | When `rag_mode: tool`, suppress auto-injection; expose synthetic `search_knowledge` tool instead. LLM decides when to search. Follows Phase 1C `tool_search` pattern. ~80 lines. |

**Parallelization:** 26A is the foundation (26B, 26E, 26F depend on it). 26C and 26D are independent of everything. 26E and 26F are independent of each other but need 26A.

**Recommended order:** 26A → (26B + 26C + 26D in parallel) → (26E + 26F in parallel)

**Key files:** `src/config/role.rs` (26A), `src/pipe.rs` (26B), `src/config/mod.rs` (26C), `src/main.rs` + `src/cli.rs` (26D), `src/config/input.rs` (26E), `src/function.rs` (26F).
