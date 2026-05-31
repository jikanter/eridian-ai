#!/usr/bin/env bats

# Phase 22: DAG observability & budget.
# Exercises the --trace tree (22A), per-branch labelling (22B), and the
# enriched -o json envelope. Uses --dry-run so each stage echoes its prompt
# instead of calling a model — the run is deterministic and offline.
#
# These tests also pin the fix that makes the `--pipe` path honor the global
# `--dry-run` / `--trace` flags (previously set after the pipe early-return).

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"
DAG_DEF="/tmp/aichat-phase22-dag.yaml"

setup() {
  mkdir -p "$ROLES_DIR"
  printf 'You are stage A.\n' > "$ROLES_DIR/p22-a.md"
  printf 'You are stage B.\n' > "$ROLES_DIR/p22-b.md"
  printf 'You are stage C.\n' > "$ROLES_DIR/p22-c.md"
  printf 'You are stage D.\n' > "$ROLES_DIR/p22-d.md"
  cat > "$DAG_DEF" <<EOF
pipeline:
  - role: p22-a
  - parallel:
      - role: p22-b
      - role: p22-c
    merge: concatenate
  - role: p22-d
EOF
}

teardown() {
  rm -f "$ROLES_DIR/p22-a.md" "$ROLES_DIR/p22-b.md" "$ROLES_DIR/p22-c.md" "$ROLES_DIR/p22-d.md"
  rm -f "$DAG_DEF"
}

@test "phase22: --dry-run on --pipe echoes instead of calling a model" {
  # The dry-run preview must complete fast and offline (no live model call).
  run timeout 30 "$AICHAT_BIN" --pipe --pipe-def "$DAG_DEF" --dry-run "hello"
  [ "$status" -eq 0 ]
}

@test "phase22: --trace renders a DAG tree with branches, merge, and totals" {
  run timeout 30 "$AICHAT_BIN" --pipe --pipe-def "$DAG_DEF" --trace --dry-run "hello"
  [ "$status" -eq 0 ]
  [[ "$output" == *"[pipeline] aichat-phase22-dag"* ]]
  [[ "$output" == *"[1] p22-a"* ]]
  [[ "$output" == *"parallel (2 branches)"* ]]
  [[ "$output" == *"[2a] p22-b"* ]]
  [[ "$output" == *"[2b] p22-c"* ]]
  [[ "$output" == *"merge: concatenate"* ]]
  [[ "$output" == *"[3] p22-d"* ]]
  [[ "$output" == *"(wall)"* ]]
  [[ "$output" == *"(sequential)"* ]]
}

@test "phase22: -o json envelope carries wall_latency_ms and per-node grouping" {
  run timeout 30 "$AICHAT_BIN" --pipe --pipe-def "$DAG_DEF" -o json --dry-run "hello"
  [ "$status" -eq 0 ]
  [[ "$output" == *"wall_latency_ms"* ]]
  [[ "$output" == *"total_cost_usd"* ]]
  [[ "$output" == *"node_index"* ]]
  [[ "$output" == *"\"branch\""* ]]
}

@test "phase22: -o json pipeline stdout is a single clean JSON document" {
  # stdout must be exactly the envelope — the last stage must NOT also print
  # its own output ahead of it. (Capture stdout only via the inner redirect.)
  run timeout 30 bash -c "'$AICHAT_BIN' --pipe --pipe-def '$DAG_DEF' -o json --dry-run 'hello' 2>/dev/null"
  [ "$status" -eq 0 ]
  echo "$output" | python3 -c "import sys, json; d = json.load(sys.stdin); assert 'output' in d and 'trace' in d, list(d)"
}
