#!/usr/bin/env bats

# Regression tests for Pipeline features described in Pipeline-Guide.md
load common.bash

ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"
PIPES_DIR="$(pwd)/pipelines"

setup() {
  mkdir -p "$ROLES_DIR"
  mkdir -p "$PIPES_DIR"
  
  # Create some dummy roles for pipeline stages
  cat > "$ROLES_DIR/stage-a.md" <<EOF
---
---
Stage A: __INPUT__
EOF
  cat > "$ROLES_DIR/stage-b.md" <<EOF
---
---
Stage B: __INPUT__
EOF
}

teardown() {
  rm -f "$ROLES_DIR/stage-a.md"
  rm -f "$ROLES_DIR/stage-b.md"
  rm -rf "$PIPES_DIR"
}

@test "pipeline: command-line stages with --dry-run" {
  # Note: --pipe is required for --stage
  run_aichat --pipe --stage stage-a --stage stage-b --dry-run "start"
  [ "$status" -eq 0 ]
  # In dry-run for pipeline, it should show preflight validation or the execution plan
}

@test "pipeline: pipeline definition file" {
  cat > "test-pipe.yaml" <<EOF
stages:
  - role: stage-a
  - role: stage-b
EOF
  run_aichat --pipe --pipe-def test-pipe.yaml --dry-run "start"
  [ "$status" -eq 0 ]
  rm test-pipe.yaml
}

@test "pipeline: pipeline role" {
  cat > "$ROLES_DIR/pipe-role.md" <<EOF
---
pipeline:
  - role: stage-a
  - role: stage-b
---
EOF
  # Invoking a pipeline role should work
  run_aichat -r pipe-role --dry-run "start"
  [ "$status" -eq 0 ]
  rm "$ROLES_DIR/pipe-role.md"
}

@test "pipeline: json output contains trace" {
  run_aichat --pipe --stage stage-a --output json --dry-run "test"
  [ "$status" -eq 0 ]
  if [[ -z "$AICHAT_TEST_MODEL" ]]; then
    [[ "$output" == *"trace"* ]]
    [[ "$output" == *"stages"* ]]
  fi
}
