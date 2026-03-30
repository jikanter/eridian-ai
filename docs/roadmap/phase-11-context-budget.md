# Phase 11: Runtime Intelligence — Context Budget

**Status:** Planned
**Epic:** 2 — Runtime Intelligence
**Design:** [epic-2.md](../analysis/epic-2.md)

---

| Item | Status | Notes |
|---|---|---|
| 11A. Context budget allocator core | — | New `src/context_budget.rs`. Calculates: `remaining = max_input_tokens - output_reserve - fixed_allocations`. Fixed = system prompt + schema + user message (always included). Remaining fills greedily by priority. Replaces hard error in `guard_max_input_tokens()` with intelligent truncation + stderr warning. |
| 11B. BM25-ranked file inclusion | — | When `-f` includes multiple files/directory, score each file against user query via `bm25` crate (already in deps). Include files in descending relevance order until budget exhausted. Produces `RankedContent { path, content, relevance_score, token_estimate }`. Emits selection summary on stderr. |
| 11C. Budget-aware RAG | — | Pass remaining token budget to `Config::search_rag()`. Compute `top_k = remaining / avg_chunk_tokens` instead of fixed k. Requires modifying `Rag::search()` signature to accept budget parameter. |

**Parallelization:** 11A, 11B, 11C are independently implementable:
- **Agent A**: Core `ContextBudget` struct + integration in `Input::prepare_completion_data()`
- **Agent B**: BM25 file ranking module (standalone, returns ranked file list)
- **Agent C**: Budget-aware RAG (modify `search_rag` to accept budget)

All three merge into `prepare_completion_data()` where the budget allocator orchestrates them.

**Config:**
```yaml
context_budget:
  output_reserve: 4096     # tokens reserved for output
  file_strategy: bm25      # bm25 | truncate | all (default: bm25 when >1 file)
  warn_on_truncation: true  # emit warning to stderr when content is truncated
```

**Key files:** new `src/context_budget.rs` (11A), `src/config/input.rs` (11A integration), `src/config/mod.rs` (11C, config parsing), `src/client/model.rs` (11A soft-fail guard), `src/rag/mod.rs` (11C).

**What to kill:**
| Proposal | Reason |
|---|---|
| Token-exact counting (tiktoken) | `tiktoken-rs` only covers OpenAI tokenizers. Heuristic (~1.3 tokens/word) is sufficient for budget allocation — order-of-magnitude correctness, not precision. |
| LiteLLM integration as dependency | Python runtime conflicts with single-binary. AIChat already targets LiteLLM proxy via `openai-compatible` with zero code changes. |
| Automatic model selection (`model: auto`) | Requires quality benchmarks per model per task. Manual selection + fallback chains (10D) is more reliable. |
| Prompt caching API integration | Provider-specific (Anthropic cache_control, OpenAI implicit). Separate epic for provider-specific optimizations. |
