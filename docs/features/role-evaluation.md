# Role Evaluation

Score role output, compare two roles head-to-head, and attribute cost per role.
Four small, composable surfaces (Epic 8, Phase 23).

## `metrics:` — score a role's output

Add a `metrics:` list to a role's frontmatter. Each metric is a named shell
command that receives the role's output **on stdin**; **exit 0 = pass**,
non-zero (or a command that fails to spawn) = fail.

```yaml
---
model: openai:gpt-4o-mini
metrics:
  - name: nonempty
    shell: test -s /dev/stdin
  - name: valid_json
    shell: jq . >/dev/null 2>&1
  - name: under_500_chars
    shell: test "$(wc -c)" -lt 500
---
Summarize the input as JSON.
```

Metrics run **after** output-schema validation and **before** lifecycle hooks
(`pipe_to` / `save_to`). They never run under `--dry-run`. Results appear in
`--trace`, the run log, and the per-role ledger.

Inspect a role's declared metrics without running it:

```bash
aichat --explain-role summarize          # text, includes a "Metrics:" block
aichat --explain-role summarize -o json  # { ..., "metrics": [{name, shell}], ... }
```

## `--compare` — two roles, one input

Run the same input through two roles and see them side by side:

```bash
aichat --compare summarize-v1 summarize-v2 "long article text…"
```

```
--- summarize-v1 (deepseek:deepseek-chat) ---
  Output: <output A>
  Metrics: nonempty=PASS  valid_json=PASS
  Cost: $0.0001  (892 input, 150 output tokens)

--- summarize-v2 (claude:claude-sonnet-4-6) ---
  Output: <output B>
  Metrics: nonempty=PASS  valid_json=FAIL
  Cost: $0.0040  (892 input, 200 output tokens)

--- Comparison ---
  Cost ratio: summarize-v2 is 40.0x more expensive
  Token ratio: summarize-v2 uses 33% more output tokens
  Metrics: summarize-v1 2/2  vs  summarize-v2 1/2
```

Add `-o json` for one machine-readable `{ "roleA", "roleB", "comparison" }`
document. Input may be positional or piped on stdin; with neither, `--compare`
exits non-zero with an error.

## Cost attribution by role (run log)

With a run log configured (`AICHAT_RUN_LOG=<file>`), each entry now records the
role responsible:

- **single role** — the record gains `role` and a `metrics` array.
- **pipeline** — one record **per stage**, each with `pipeline`, `stage`,
  `stage_role`, `model`, tokens, `cost_usd`, `latency_ms`, `cached`, sharing one
  pipeline `run_id`.

Aggregate spend per role with any JSONL tool:

```bash
duckdb -c "SELECT stage_role, SUM(cost_usd) AS spend, COUNT(*) AS calls \
           FROM read_json_auto('runlog.jsonl') \
           WHERE stage_role IS NOT NULL GROUP BY stage_role ORDER BY spend DESC"
```

## Per-role invocation ledger

Opt in by pointing `AICHAT_ROLE_LEDGER` at a directory:

```bash
export AICHAT_ROLE_LEDGER="$HOME/.aichat-ledger"
aichat -r summarize "…"
# appends one scored record to $AICHAT_ROLE_LEDGER/summarize.jsonl
```

Each record holds truncated input/output summaries, `model`, tokens,
`cost_usd`, `latency_ms`, `schema_retries`, and the metric pass/fail results —
a per-role history you can grep, diff, or chart over time. `--compare` ledgers
both roles it runs.

## See also

- Roadmap: [`docs/roadmap/phase-23-overview.md`](../roadmap/phase-23-overview.md)
- Offline demo: [`docs/demos/phase-23-role-evaluation.md`](../demos/phase-23-role-evaluation.md)
