# Phase 23: Role Evaluation : Overview - Epic 8

| Item | Description | Status |
|---|---|---|
| 23A | `metrics:` field on roles (shell commands that score output) | **Done** |
| 23B | `--compare` flag (run input through two roles, show results side-by-side with cost) | **Done** |
| 23C | Cost attribution by role in run log (tag each pipeline stage in JSONL) | **Done** |
| 23D | Role invocation history (append scored records to per-role ledger) | **Done** |

**23A Design — Metrics Field:**

```yaml
---
name: summarizer
metrics:
  - name: valid_json
    shell: "jq . >/dev/null 2>&1"
  - name: under_500_words
    shell: "test $(wc -w < /dev/stdin) -lt 500"
  - name: has_required_fields
    shell: "jq -e '.summary and .key_points' >/dev/null 2>&1"
---
```

Each metric receives the role's output on stdin and exits 0 (pass) or 1 (fail). Metrics run after output validation, before lifecycle hooks. Results recorded in the JSONL run log alongside cost and tokens.

**Implementation:** In `src/main.rs`, after `validate_schema("output", ...)`, iterate `role.metrics()`. For each, pipe output to the shell command. Record `{metric_name, pass: bool}` in the trace event.

**Files:** `src/config/role.rs` (add `metrics: Vec<RoleMetric>`), `src/main.rs` (evaluate metrics post-output), `src/utils/trace.rs` (emit metric events).

**23B Design — Compare Flag:**

```bash
$ echo "Review this code" | aichat --compare summarizer-v1 summarizer-v2

--- summarizer-v1 (deepseek:deepseek-chat) ---
  Output: { "summary": "...", "key_points": [...] }
  Metrics: valid_json=PASS  under_500_words=PASS  has_required_fields=PASS
  Cost: $0.0004  (892 input, 341 output tokens)

--- summarizer-v2 (claude:claude-haiku-4-5) ---
  Output: { "summary": "...", "key_points": [...] }
  Metrics: valid_json=PASS  under_500_words=PASS  has_required_fields=PASS
  Cost: $0.002  (892 input, 287 output tokens)

--- Comparison ---
  Cost ratio: summarizer-v2 is 5.0x more expensive
  Token ratio: summarizer-v2 uses 16% fewer output tokens
  Metrics: both pass all metrics
```

Manual A/B testing with zero infrastructure. Combined with the metrics field, this becomes systematic.

**Files:** `src/cli.rs` (add `--compare` flag taking two role names), `src/main.rs` (parallel execution and diff rendering).

**23C Design — Cost Attribution by Role:**

Currently the JSONL run log records the top-level role but not per-stage breakdown. Add `stage_role` and `pipeline_role` fields to each run log entry:

```jsonl
{"role":"extract","pipeline":"secure-review","stage":1,"model":"deepseek:deepseek-chat","cost_usd":0.0001,...}
{"role":"review","pipeline":"secure-review","stage":2,"model":"claude:claude-sonnet-4-6","cost_usd":0.012,...}
```

This enables downstream aggregation: `duckdb "SELECT role, SUM(cost_usd) FROM read_json('run.jsonl') GROUP BY role"`.

**Files:** `src/pipe.rs` (add `stage_role` + `pipeline_role` to trace/run log entries), `src/utils/ledger.rs` (extend run log schema).

---

## Shipped (2026-05-30)

Demo: [`docs/demos/phase-23-role-evaluation.md`](../demos/phase-23-role-evaluation.md).
User docs: [`docs/features/role-evaluation.md`](../features/role-evaluation.md).

**23A — `metrics:` field.** A role's frontmatter carries a `metrics:` list of
`{name, shell}` pairs (serde `RoleMetric`, parsed in `Role::new`, accessor
`Role::metrics()`). `evaluate_metrics(&[RoleMetric], output)` in
`src/config/metrics.rs` runs each `shell` via `sh -c` with the output on stdin —
exit 0 = pass, non-zero or spawn error = fail (never panics). In `start_directive`
metrics run after output-schema validation and before lifecycle hooks, gated on
`!is_dry_run`; results emit through `TraceEmitter::emit_metrics` and fold into the
run log + role ledger. Declared metrics are discoverable offline via
`--explain-role` (text + `-o json` `"metrics"` array).

**23B — `--compare ROLE1 ROLE2`.** Two-value CLI flag. `run_compare` (main.rs)
invokes both roles on the same input via `pipe::invoke_role`, scores each role's
metrics, then `src/compare.rs::render_comparison` prints the side-by-side block
(per-role output, `name=PASS/FAIL` metrics, cost) and a `--- Comparison ---`
footer (cost ratio with $0-baseline guard, output-token delta, metrics
agreement). `-o json` emits one `{roleA, roleB, comparison}` document. The
renderer is a pure, unit-tested function over a `CompareResult` struct. Empty
input bails with a clear error (exit 1).

**23C — Cost attribution by role.** The single-role run-log record gains `role`
and (when metrics ran) a `metrics:[{name,pass}]` array. The pipeline path writes
one record per `StageTrace` (`stage_run_log_record`) with `pipeline`, `stage`,
`stage_role`, model, tokens, `cost_usd`, latency, `cached` under one pipeline
`run_id`, so `duckdb … GROUP BY stage_role` aggregates cost per role.

**23D — Per-role invocation ledger.** Opt-in via `AICHAT_ROLE_LEDGER=<dir>`
(→ `Config.role_ledger_dir`). Each invocation appends a scored record to
`<dir>/<role>.jsonl` (`role_ledger_record`): truncated input/output summaries,
model, tokens, `cost_usd`, latency, `schema_retries`, metric results. The
filename is sanitized (`sanitize_role_name`). `--compare` ledgers both roles.

**Tests added:** `src/config/metrics.rs` (4); `src/config/role.rs` (3 — metrics
parse, empty-when-absent, `explain` surfaces metrics); `src/compare.rs` (the
render/ratio/json suite); `src/utils/ledger.rs` (stage + role-ledger record
shapes, summary truncation); `src/utils/trace.rs` (`emit_metrics`). Full
`cargo test --bin aichat` green (636 passed, 0 failed).

**Coverage note.** The pure cores (metric eval, comparison render, record
builders, trace emit, frontmatter parse, explain surfacing) are unit-tested and
verified offline against the built binary (`--explain-role`, `--compare` help +
empty-input bail). End-to-end runtime wiring (metrics scored against live model
output, run-log/ledger writes, a real `--compare`) needs a configured model and
is not exercised by the offline demo.
