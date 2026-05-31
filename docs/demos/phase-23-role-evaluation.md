# Phase 23 Demo — Role Evaluation

<!-- showboat:auto -->

This demo is **offline and deterministic**: no model is called. It exercises the
surfaces that need no inference — declared-metric discovery via `--explain-role`,
the `--compare` flag contract, and the run-log / ledger record shapes. The
runtime scoring of *live* model output is covered by unit tests
(`src/config/metrics.rs`, `src/compare.rs`, `src/utils/ledger.rs`), not here,
because it needs a configured model.

## Setup

```bash
export AICHAT_CONFIG_DIR=$(mktemp -d)
mkdir -p "$AICHAT_CONFIG_DIR/roles"

cat > "$AICHAT_CONFIG_DIR/roles/summarize.md" <<'EOF'
---
model: openai:gpt-4o-mini
metrics:
  - name: nonempty
    shell: test -s /dev/stdin
  - name: valid_json
    shell: jq . >/dev/null 2>&1
---
Summarize the input as JSON.
EOF

cat > "$AICHAT_CONFIG_DIR/roles/summarize-terse.md" <<'EOF'
---
model: openai:gpt-4o-mini
metrics:
  - name: nonempty
    shell: test -s /dev/stdin
---
Summarize in one line.
EOF
```

## Demo: declared metrics are discoverable (23A)

```bash
showboat note "Metrics surface in --explain-role (text)"
aichat --explain-role summarize 2>&1
```

Expected — a `Metrics:` block lists each declared metric:

```
Role: summarize

  Source: <config>/roles/summarize.md

  Composition:
    (standalone — no extends/include/pipeline)
  Metrics:
    nonempty — test -s /dev/stdin
    valid_json — jq . >/dev/null 2>&1

  Prompt:
    Summarize the input as JSON.
```

```bash
showboat note "Metrics in machine-readable --explain-role -o json"
aichat --explain-role summarize -o json 2>&1 | jq '.metrics'
```

```json
[
  { "name": "nonempty",   "shell": "test -s /dev/stdin" },
  { "name": "valid_json", "shell": "jq . >/dev/null 2>&1" }
]
```

## Demo: `--compare` contract (23B)

```bash
showboat note "--compare takes exactly two roles"
aichat --help 2>&1 | grep -A1 -- '--compare'
```

```bash
showboat note "--compare with no input fails cleanly (exit 1)"
aichat --compare summarize summarize-terse </dev/null; echo "exit=$?"
```

A real comparison (`aichat --compare summarize summarize-terse "article…"`)
renders both outputs, `name=PASS/FAIL` metric lines, and a `--- Comparison ---`
footer (cost ratio, output-token delta, metrics agreement). Add `-o json` for one
`{roleA, roleB, comparison}` document. Omitted here — it needs a live model.

## Reference: cost attribution & ledger shapes (23C / 23D)

These records are written during *live* runs; their shapes are unit-tested.

Run-log entry, single role (`AICHAT_RUN_LOG=runlog.jsonl`):

```json
{"ts":"…","run_id":"…","role":"summarize","model":"openai:gpt-4o-mini",
 "input_tokens":120,"output_tokens":80,"cost_usd":0.0001,"latency_ms":640,
 "schema_retries":0,"metrics":[{"name":"nonempty","pass":true}]}
```

Run-log entries, pipeline (one per stage):

```json
{"ts":"…","run_id":"…","pipeline":"secure-review","stage":1,
 "stage_role":"extract","model":"deepseek-chat","input_tokens":500,
 "output_tokens":200,"cost_usd":0.0001,"latency_ms":800,"cached":false}
```

```bash
showboat note "Aggregate cost per role with duckdb"
echo "SELECT stage_role, SUM(cost_usd) FROM read_json_auto('runlog.jsonl') GROUP BY stage_role"
```

Per-role ledger (`AICHAT_ROLE_LEDGER=<dir>` → `<dir>/summarize.jsonl`):

```json
{"ts":"…","run_id":"…","role":"summarize","model":"openai:gpt-4o-mini",
 "input_summary":"…","output_summary":"…","input_tokens":120,
 "output_tokens":80,"cost_usd":0.0001,"latency_ms":640,"schema_retries":0,
 "metrics":[{"name":"nonempty","pass":true}]}
```
