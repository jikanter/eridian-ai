# Phase 22: DAG Observability & Budget

*2026-05-30T00:45:34Z by Showboat 0.6.1*
<!-- showboat-id: 1288095d-565e-4f93-9e28-644328e3e695 -->

Phase 22 completes Epic 7's observability and budget surface for pipeline DAGs. Five items shipped:

- **22A** — DAG trace tree: `--pipe ... --trace` prints a tree of the executed DAG on stderr, with per-stage model, tokens, cost, and latency. Under `-o json` the run emits a single clean `{output, trace}` document (the last stage no longer prints ahead of the envelope).
- **22B** — Per-branch cost: each fan-out branch is labelled `[Na]/[Nb]/…`; the JSON envelope carries `node_index` + `branch` so consumers can group cost by branch.
- **22C** — Budget-aware fan-out: a parallel node's pre-allocated dollar budget is split evenly across its branches; a switch arm inherits the node's full budget.
- **22D** — Stage caching observability: a cache-replayed stage is flagged `cached` and rendered `(cached)` with $0 / 0ms.
- **22E** — Fixed the historically flaky `test_load_mcp_servers_file_rejects_neither_command_nor_url` (a temp-dir name collision under parallel test threads) and removed the `--skip` that masked it.

Prerequisite fix: the `--pipe` path now honors the global `--dry-run` / `--trace` / `--no-cache` flags, which were previously applied *after* the pipe short-circuit and silently dropped. This makes the trace tree demoable fully offline (each stage echoes under `--dry-run`).

## New surface

`StageTrace` gains `node_index` (top-level DAG node grouping) and `cached` (22D). Three pure helpers back the renderer and budget split:

```bash
grep -E "^    pub (node_index|cached):|^fn (split_branch_budget|pipeline_timing|render_trace_tree)\(" src/pipe.rs
```

```output
    pub node_index: usize,
    pub cached: bool,
fn split_branch_budget(node_budget_usd: Option<f64>, branch_count: usize) -> Option<f64> {
fn pipeline_timing(traces: &[StageTrace]) -> (u64, u64) {
fn render_trace_tree(label: &str, nodes: &[PipelineNode], traces: &[StageTrace]) -> String {
```

## 22A/22B — the trace tree

A pipeline with a sequential stage, a 2-branch fan-out (`concatenate` merge), and a final stage. Run under `--dry-run` (each stage echoes; no model call) with `--trace`. Model ids and latencies are normalized below so the demo is reproducible across machines; under `--dry-run` tokens and cost are deterministically zero.

```bash
set -e
ROLES_DIR="$HOME/Library/Application Support/aichat/roles"
mkdir -p "$ROLES_DIR"
for r in p22-a p22-b p22-c p22-d; do printf "You are %s.\n" "$r" > "$ROLES_DIR/$r.md"; done
cat > /tmp/phase22-dag.yaml <<EOF
pipeline:
  - role: p22-a
  - parallel:
      - role: p22-b
      - role: p22-c
    merge: concatenate
  - role: p22-d
EOF
./target/debug/aichat --pipe --pipe-def /tmp/phase22-dag.yaml --trace --dry-run "review" </dev/null 2>&1 >/dev/null \
  | sed -E "s#ollama:[A-Za-z0-9._:-]+#MODEL#g; s/[0-9]+ms/Nms/g; s/[0-9]+\.[0-9]+s/N.Ns/g"
rm -f "$ROLES_DIR"/p22-a.md "$ROLES_DIR"/p22-b.md "$ROLES_DIR"/p22-c.md "$ROLES_DIR"/p22-d.md /tmp/phase22-dag.yaml
```

```output
[pipeline] phase22-dag (4 stages, 1 parallel)
  [1] p22-a  MODEL  0→0tok  $0.0000  Nms
  [2] parallel (2 branches)
    [2a] p22-b  MODEL  0→0tok  $0.0000  Nms
    [2b] p22-c  MODEL  0→0tok  $0.0000  Nms
    merge: concatenate
  [3] p22-d  MODEL  0→0tok  $0.0000  Nms
  total: $0.0000  N.Ns (wall) vs N.Ns (sequential)
```

Under `-o json` the run emits a single clean JSON document — `{output, trace}` — and nothing else: the last stage no longer prints its own text ahead of the envelope (and streaming is suppressed for the same reason). The `trace` exposes the machine-readable surface: `wall_latency_ms` (fan-out concurrency) alongside the sequential `total_latency_ms`, plus `node_index` + `branch` on every stage so a consumer can roll cost up per branch. Parsing the whole of stdout with a strict JSON reader succeeds:

```bash
set -e
ROLES_DIR="$HOME/Library/Application Support/aichat/roles"
mkdir -p "$ROLES_DIR"
for r in p22-a p22-b p22-c p22-d; do printf "You are %s.\n" "$r" > "$ROLES_DIR/$r.md"; done
cat > /tmp/phase22-dag.yaml <<EOF
pipeline:
  - role: p22-a
  - parallel:
      - role: p22-b
      - role: p22-c
    merge: concatenate
  - role: p22-d
EOF
./target/debug/aichat --pipe --pipe-def /tmp/phase22-dag.yaml -o json --dry-run "review" </dev/null 2>/dev/null \
  | python3 -c "import sys,json; d=json.load(sys.stdin); t=d['trace']; print('top-level keys:', sorted(d)); print('trace keys:   ', sorted(t)); print('stage roles:  ', [s['role'] for s in t['stages']])"
rm -f "$ROLES_DIR"/p22-a.md "$ROLES_DIR"/p22-b.md "$ROLES_DIR"/p22-c.md "$ROLES_DIR"/p22-d.md /tmp/phase22-dag.yaml
```

```output
top-level keys: ['output', 'trace']
trace keys:    ['stages', 'total_cost_usd', 'total_latency_ms', 'wall_latency_ms']
stage roles:   ['p22-a', 'p22-b', 'p22-c', 'p22-d']
```

## Unit coverage

The renderer, timing model, and budget split are pure functions, unit-tested without any model. `pipeline_timing` proves wall-clock is the slowest fan-out branch plus the sequential remainder; `split_branch_budget` proves the even split (and the divide-by-zero guard); `render_trace_tree_marks_cached_stage` proves the 22D `(cached)` marker.

```bash
cargo test --bin aichat pipe::tests 2>&1 | grep -oE "pipe::tests::[a-z_]+" | sort -u
```

```output
pipe::tests::pipeline_timing_custom_merge_runs_after_branches
pipe::tests::pipeline_timing_parallel_wall_is_slowest_branch
pipe::tests::pipeline_timing_sequential_sums_all_latency
pipe::tests::render_trace_tree_custom_merge_shows_merge_role
pipe::tests::render_trace_tree_marks_cached_stage
pipe::tests::render_trace_tree_shows_nodes_branches_merge_and_totals
pipe::tests::split_branch_budget_divides_equally
pipe::tests::split_branch_budget_none_passes_through
pipe::tests::split_branch_budget_zero_branches_is_none
```

```bash
cargo test --bin aichat pipe::tests 2>&1 | grep -E "^test result:" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/"
```

```output
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```

## 22E — flaky test fixed

`write_tmp_json` keyed its temp dir on a wall-clock timestamp; two parallel test threads in the same tick collided, so `test_load_mcp_servers_file_rejects_neither_command_nor_url` could read a sibling's fixture and assert the wrong error. It now keys on `(pid, atomic counter)` — unique by construction. A new contention test (16 threads × 50 writes) guards it, and the formerly-flaky test runs in the full suite again (the `--skip` is gone).

```bash
cargo test --bin aichat 2>&1 | grep -E "(write_tmp_json_is_collision_free_across_threads|test_load_mcp_servers_file_rejects_neither_command_nor_url) \.\.\. ok|^test result:" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ passed/N passed/; s/[0-9]+ filtered out/N filtered out/"
```

```output
test mcp_client::tests::test_load_mcp_servers_file_rejects_neither_command_nor_url ... ok
test mcp_client::tests::write_tmp_json_is_collision_free_across_threads ... ok
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```

## End-to-end (offline)

The integration suite drives the real binary over a DAG pipe-def under `--dry-run`, asserting the tree, the branch labels, the merge line, the JSON keys, and that `-o json` stdout parses as a single clean document — all without a model. (This also pins the `--pipe` dry-run/trace fix.)

```bash
AICHAT_BIN=./target/debug/aichat bats tests/integration/dag-trace.sh 2>&1 | grep -E "^(ok|not ok)"
```

```output
ok 1 phase22: --dry-run on --pipe echoes instead of calling a model
ok 2 phase22: --trace renders a DAG tree with branches, merge, and totals
ok 3 phase22: -o json envelope carries wall_latency_ms and per-node grouping
ok 4 phase22: -o json pipeline stdout is a single clean JSON document
```
