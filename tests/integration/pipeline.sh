#!/usr/bin/env bats

# Pipeline integration tests
# Using --dry-run for preflight validation without LLM calls.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"
PIPELINES_DIR="${AICHAT_CONFIG_DIR:-$HOME/Library/Application Support/aichat}/pipelines"

setup() {
  mkdir -p "$ROLES_DIR"
  mkdir -p "$PIPELINES_DIR"
}

teardown() {
  rm -f "$ROLES_DIR/pipe-test-role.md"
  rm -f "$ROLES_DIR/dag-parallel-role.md"
  rm -f "$ROLES_DIR/dag-switch-role.md"
  rm -f "$PIPELINES_DIR/test-pipeline.yaml"
  rm -f /tmp/aichat-phase21-dag-*.yaml
}

@test "pipeline: --stage with invalid role fails preflight" {
  run "$AICHAT_BIN" --pipe --stage non-existent-role --dry-run "test"
  [ "$status" -ne 0 ]
  [[ "$output" == *"references unknown entity 'non-existent-role'"* ]]
}

@test "pipeline: --pipe-def with non-existent file fails" {
  run "$AICHAT_BIN" --pipe --pipe-def non-existent-pipe.yaml --dry-run "test"
  [ "$status" -ne 0 ]
  [[ "$output" == *"Pipeline definition not found"* ]]
}

@test "pipeline: role with pipeline frontmatter" {
  cat > "$ROLES_DIR/pipe-test-role.md" <<EOF
---
pipeline:
  - role: %code%
---
Input
EOF
  # %code% is a built-in role, so this should pass preflight
  run "$AICHAT_BIN" -r pipe-test-role --dry-run "test"
  [ "$status" -eq 0 ]
}

@test "pipeline: --stage overrides model" {
  run "$AICHAT_BIN" --pipe --stage %code%@non-existent-model --dry-run "test"
  [ "$status" -ne 0 ]
  [[ "$output" == *"references unknown model 'non-existent-model'"* ]]
}

# ----- Phase 21: DAG primitives -----

@test "pipeline: DAG --pipe-def with parallel rejects unknown role inside branch" {
  cat > /tmp/aichat-phase21-dag-bad.yaml <<EOF
pipeline:
  - role: "%code%"
  - parallel:
      - role: "%code%"
      - role: phase21-bogus-branch-role
EOF
  run "$AICHAT_BIN" --pipe --pipe-def /tmp/aichat-phase21-dag-bad.yaml --dry-run "test"
  [ "$status" -ne 0 ]
  [[ "$output" == *"phase21-bogus-branch-role"* ]]
}

@test "pipeline: DAG --pipe-def with switch rejects unknown role inside branch" {
  cat > /tmp/aichat-phase21-dag-switch.yaml <<EOF
pipeline:
  - role: "%code%"
  - switch:
      - when: { contains: "bug" }
        role: phase21-bogus-switch-role
      - otherwise: true
        role: "%code%"
EOF
  run "$AICHAT_BIN" --pipe --pipe-def /tmp/aichat-phase21-dag-switch.yaml --dry-run "test"
  [ "$status" -ne 0 ]
  [[ "$output" == *"phase21-bogus-switch-role"* ]]
}

@test "pipeline: DAG --pipe-def rejects double otherwise" {
  cat > /tmp/aichat-phase21-dag-doubleow.yaml <<EOF
pipeline:
  - switch:
      - otherwise: true
        role: "%code%"
      - otherwise: true
        role: "%code%"
EOF
  run "$AICHAT_BIN" --pipe --pipe-def /tmp/aichat-phase21-dag-doubleow.yaml --dry-run "test"
  [ "$status" -ne 0 ]
  [[ "$output" == *"more than one \`otherwise:\`"* || "$output" == *"more than one"* ]]
}

@test "pipeline: DAG --pipe-def rejects unknown merge strategy" {
  cat > /tmp/aichat-phase21-dag-badmerge.yaml <<EOF
pipeline:
  - parallel:
      - role: "%code%"
    merge: weirdo
EOF
  run "$AICHAT_BIN" --pipe --pipe-def /tmp/aichat-phase21-dag-badmerge.yaml --dry-run "test"
  [ "$status" -ne 0 ]
  [[ "$output" == *"Unknown merge"* || "$output" == *"weirdo"* ]]
}

@test "pipeline: DAG --pipe-def with custom_role merger validates the merger role exists" {
  cat > /tmp/aichat-phase21-dag-custom.yaml <<EOF
pipeline:
  - parallel:
      - role: "%code%"
      - role: "%code%"
    merge:
      custom_role: phase21-bogus-merge-role
EOF
  run "$AICHAT_BIN" --pipe --pipe-def /tmp/aichat-phase21-dag-custom.yaml --dry-run "test"
  [ "$status" -ne 0 ]
  [[ "$output" == *"phase21-bogus-merge-role"* ]]
}

@test "pipeline: DAG --pipe-def rejects mixing stages and pipeline keys" {
  cat > /tmp/aichat-phase21-dag-mix.yaml <<EOF
stages:
  - role: "%code%"
pipeline:
  - role: "%code%"
EOF
  run "$AICHAT_BIN" --pipe --pipe-def /tmp/aichat-phase21-dag-mix.yaml --dry-run "test"
  [ "$status" -ne 0 ]
  [[ "$output" == *"pick one"* || "$output" == *"stages"* ]]
}

@test "pipeline: role frontmatter with parallel passes preflight" {
  cat > "$ROLES_DIR/dag-parallel-role.md" <<EOF
---
pipeline:
  - role: "%code%"
  - parallel:
      - role: "%code%"
      - role: "%code%"
    merge: concatenate
---
Body.
EOF
  run "$AICHAT_BIN" -r dag-parallel-role --dry-run "test"
  [ "$status" -eq 0 ]
}

@test "pipeline: role frontmatter with switch passes preflight" {
  cat > "$ROLES_DIR/dag-switch-role.md" <<EOF
---
pipeline:
  - switch:
      - when: { contains: "bug" }
        role: "%code%"
      - otherwise: true
        role: "%code%"
---
Body.
EOF
  run "$AICHAT_BIN" -r dag-switch-role --dry-run "test"
  [ "$status" -eq 0 ]
}

@test "pipeline: DAG --pipe-def rejects when-after-otherwise ordering" {
  cat > /tmp/aichat-phase21-dag-badorder.yaml <<EOF
pipeline:
  - switch:
      - when: { contains: "bug" }
        role: "%code%"
      - otherwise: true
        role: "%code%"
      - when: { contains: "feature" }
        role: "%code%"
EOF
  run "$AICHAT_BIN" --pipe --pipe-def /tmp/aichat-phase21-dag-badorder.yaml --dry-run "test"
  [ "$status" -ne 0 ]
  [[ "$output" == *"after \`otherwise:\`"* || "$output" == *"otherwise"* ]]
}
