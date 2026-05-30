# Phase 22: DAG Observability & Budget : Overview - Epic 7

| Item | Description | Status |
|---|---|---|
| 22A | DAG trace visualization (tree structure in `--trace` output) | **Done** |
| 22B | Per-branch cost tracking in parallel execution | **Done** |
| 22C | Budget-aware fan-out (split pipeline budget across parallel branches) | **Done** |
| 22D | DAG stage caching (cache branches independently, skip unchanged) | **Done** |
| 22E | Fix flaky `mcp_client::tests::test_load_mcp_servers_file_rejects_neither_command_nor_url` — test pollution: passes alone, fails in `cargo test --bin aichat`. Currently `--skip`'d in `docs/demos/demo-test-suite.md` and `docs/demos/phase-9a-openai-response-format.md`. Remove the skip once fixed. | **Done** |

**22A Design — DAG Trace:**

```
[pipeline] secure-review (5 stages, 2 parallel)
  [1] extract              deepseek:deepseek-chat   500→200tok  $0.0001  0.8s
  [2] parallel (3 branches)
    [2a] security-review   claude:claude-sonnet-4-6  200→300tok  $0.004   1.2s
    [2b] style-review      deepseek:deepseek-chat    200→150tok  $0.0001  0.6s
    [2c] perf-review       deepseek:deepseek-chat    200→180tok  $0.0001  0.7s
    merge: concatenate     --                        --          --       0ms
  [3] synthesize           claude:claude-sonnet-4-6  630→200tok  $0.006   1.5s
  total: $0.0103  4.3s (wall) vs 6.1s (sequential)
```

---

## Shipped (2026-05-29)

Demo: [`docs/demos/phase-22-dag-observability.md`](../demos/phase-22-dag-observability.md).

**22A — DAG trace tree.** `aichat --pipe (--stage… | --pipe-def <file>) --trace` now
renders the executed DAG as an indented tree on stderr after the run. Each leaf stage
shows `role  model  in→out tok  $cost  latency`; a fan-out is a `parallel (N branches)`
node with `[Na]/[Nb]/…` branch lines and a `merge: <strategy>` line; a switch shows the
arm that ran. The footer reports `total: $cost  Ws (wall) vs Ss (sequential)`, where wall
time models concurrency (slowest branch per node) and sequential is the latency sum. The
renderer (`render_trace_tree`) and timing model (`pipeline_timing`) are pure functions in
`src/pipe.rs`, unit-tested without a model.

**22B — Per-branch cost.** Branch lines carry their own cost; a multi-stage branch also
prints a `branch <letter>: $cost  latency` subtotal. The `-o json` envelope stamps
`node_index` (top-level DAG node) and `branch` on every `StageTrace`, so a consumer can
roll cost up per branch, and adds `wall_latency_ms` next to the existing (sequential)
`total_latency_ms`.

**22C — Budget-aware fan-out.** A parallel node's pre-allocated dollar budget
(`pipeline_budget_usd` ÷ top-level weights) is split evenly across its branches via
`split_branch_budget`; a switch arm — only one runs — inherits the node's full budget.
This replaces the Phase 11D "nested DAG budget propagation deferred" stub in `run_parallel`
/ `run_switch`. Each branch then tail-truncates its input to its sub-budget through the
existing `context_budget` path.

**22D — Stage-cache observability.** Fan-out branches already cache independently — every
leaf stage flows through `run_stage_inner`'s content-addressable `StageCache`, keyed on
`(role, model, input)`, so distinct branch roles never collide. Phase 22D surfaces the
hit: a replayed stage sets `CallMetrics.cached` → `StageTrace.cached`, rendered `(cached)`
in the tree (with `$0` / `0ms`) and emitted as `cached: true` in JSON. (Caching is disabled
under `--dry-run`, so the marker is exercised by unit test and live runs, not the offline
demo.)

**22E — Flaky test fixed.** `write_tmp_json` (mcp.json loader tests) keyed its temp dir on
a wall-clock timestamp; two parallel test threads in the same tick produced the same dir
and clobbered each other's fixture, so `test_load_mcp_servers_file_rejects_neither_command_nor_url`
intermittently read a sibling's `{ "command", "url" }` and asserted the wrong error. It now
keys on `(process id, atomic counter)` — unique by construction. A contention test
(16 threads × 50 writes) guards it, and the `--skip` was removed from
`docs/demos/demo-test-suite.md` and `docs/demos/phase-9a-openai-response-format.md`.

**Prerequisite fix.** The `--pipe` path previously short-circuited *before* the global
runtime flags (`--dry-run`, `--trace`, `--cost`, `--no-cache`, `--knowledge`) were applied,
so they were silently dropped — `--pipe --dry-run` made real model calls. The flag setup
moved into `apply_runtime_flags`, called before the `--pipe` branch, so every path observes
them. This makes the trace tree demoable fully offline.

**Output cleanup.** Under `-o json` the pipeline now emits a *single* clean JSON document.
Previously the last stage printed its own text (or streamed tokens) to stdout ahead of the
`run`-emitted envelope, so the stream wasn't valid JSON. A dedicated `pipeline_emits_envelope`
flag on `Config` (set by `run`, distinct from `output_format` so the JSON system-prompt suffix
never leaks into stage prompts) gates the four `is_last` stdout prints and forces the final
stage non-streaming. Non-`Json` formats (`Text`/`Jsonl`/`Csv`/…) keep the per-stage print.

**Tests added:** 9 unit tests in `src/pipe.rs` (`render_trace_tree`, `pipeline_timing`,
`split_branch_budget`), 1 in `src/mcp_client/mod.rs` (cross-thread tmp-dir uniqueness),
4 bats in `tests/integration/dag-trace.sh` (tree, branch labels, JSON keys, single-document JSON).
