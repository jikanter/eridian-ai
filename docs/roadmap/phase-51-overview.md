# Phase 51 — Vendor Model Extensions : Overview — Epic 17 (Federation & Scale)

**Status:** Planned (new — 2026-06-04 refresh; research-complete) · **Owner:** aichat · **Horizon:** Later

> **Goal.** Pass **vendor-specific parameters** (Ollama `num_ctx` / `repeat_penalty`, vLLM guided
> decoding, etc.) through to OpenAI-compatible providers via a **JSON-merge body hook**, at both
> client and model granularity. Realizes the research-complete proposal in
> [`docs/analysis/2026-04-23-model-extensions.md`](../analysis/2026-04-23-model-extensions.md).
> Serves the **local-model pillar** — *"runs as well on local models as on frontier models"* — at
> ~150 LOC and **zero new dependencies**.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 51A | **`extensions: Option<serde_json::Value>`** on `ModelData` + `OpenAICompatibleConfig` (client-level defaults, model-level overrides, documented merge strategy) | Planned |
| 51B | **Body-builder merge hook** in [`src/client/openai.rs`](../../src/client/openai.rs) — deep-merge extensions into the outbound request body | Planned |
| 51C | **Role frontmatter + REPL surface** (`.extensions set <key> <value>`) | Planned |

## Cross-repo seams

None — purely aichat-internal. The payoff is **local-runtime parity**: Ollama/vLLM knobs that
the OpenAI-compatible body otherwise can't reach.

## Dependencies

- **Upstream:** none — independent, droppable into any release.
- **Realizes:** [`docs/analysis/2026-04-23-model-extensions.md`](../analysis/2026-04-23-model-extensions.md) (research complete, proposal drafted).

## Acceptance criteria

1. A model declares `extensions: { num_ctx: 8192 }`; the value **merges into the outbound body**, verified against a mock.
2. **Client-level defaults are overridden by model-level keys** per the documented merge strategy.
3. No new default dependency; the hook is inert when no `extensions` are declared.

## Grounding docs

[`2026-04-23-model-extensions.md`](../analysis/2026-04-23-model-extensions.md) (research-complete proposal)
