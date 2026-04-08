# Phase 29: Entity Evolution — Agent Dynamism

**Status:** Planned
**Epic:** 10 — Entity Evolution & Agent Dynamism
**Design:** [epic-10.md](../analysis/epic-10.md)

---

> Full design: [`docs/analysis/epic-10.md`](../analysis/epic-10.md)

| Item | Status | Notes |
|---|---|---|
| 29A. ReactPolicy trait | — | Pluggable deterministic checkpoints in `call_react`. Trait: `check(&self, context: &ReactContext) -> ReactAction`. Actions: Continue, InjectGuidance, SwitchModel, Stop. Built-in policies: CostGuard (`max_cost:`), StagnationGuard (consecutive failures), ModelEscalation. Zero token cost for happy-path. Config via `react_policy:` frontmatter. ~200 lines. |
| 29B. Agent memory (JSONL) | — | Per-agent `memory.jsonl` auto-appended from trace data at end of `call_react`. Records: invocation summaries, tool outcomes, cost. Read by `_instructions` shell function at session start. Write: ~15 lines Rust (filesystem append). Read: shell scripts (agent author decides). Zero LLM calls. |
| 29C. Macro output chaining | — | `%%` variable in macro steps resolves to previous step's output. Reads `config.last_message` between steps. Enables `extract → %% → summarize %%` patterns. ~20 lines. |

**Parallelization:** All 3 items are independently implementable. 29A modifies `call_react`'s check path. 29B modifies `call_react`'s epilogue. 29C modifies the macro runner. No conflicts.

**Note on 29A and Phases 9C/10D:** ReactPolicy generalizes what Phase 9C (schema retry) and Phase 10D (model fallback) implement as special cases. If Phase 29A lands first, those phases become one-liner policy implementations. If they land first, 29A subsumes them.

**Key files:** `src/client/common.rs` (29A/29B), `src/config/role.rs` (29A config), `src/config/mod.rs` (29C macro runner), `src/utils/ledger.rs` (29B reuse).

**What NOT to build (entity evolution scope):**

| Proposal | Reason |
|---|---|
| Multi-agent orchestration framework | Agent-as-tool + pipelines + macros compose to cover all topologies. |
| Merge Role and Agent structs | `to_role()` bridge works. llm-functions format is a separate authoring contract. |
| Give agents `extends`/`include`/`pipeline` | Agent identity is directory-based. Role inheritance doesn't map. Pipelines create two orchestration models. |
| LLM-driven planning step | Compose via pipeline: plan-role → execute-role. Costs tokens upfront. |
| Shared state between agents | Concurrency hazard. Agents communicate via tool call arguments and return values. |
| Tool synthesis (LLM generates tools) | Unbounded cost per synthesized tool. |
| Agent event bus / message passing | Wrong abstraction for single-shot CLI. Agent-as-tool is the communication channel. |
