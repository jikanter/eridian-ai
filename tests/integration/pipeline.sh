#!/usr/bin/env bats

# Pipeline integration tests
# Using --dry-run for preflight validation without LLM calls.

ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"
PIPELINES_DIR="${AICHAT_CONFIG_DIR:-$HOME/Library/Application Support/aichat}/pipelines"

setup() {
  mkdir -p "$ROLES_DIR"
  mkdir -p "$PIPELINES_DIR"
}

teardown() {
  rm -f "$ROLES_DIR/pipe-test-role.md"
  rm -f "$PIPELINES_DIR/test-pipeline.yaml"
}

@test "pipeline: --stage with invalid role fails preflight" {
  run aichat --pipe --stage non-existent-role --dry-run "test"
  [ "$status" -ne 0 ]
  [[ "$output" == *"references unknown role 'non-existent-role'"* ]]
}

@test "pipeline: --pipe-def with non-existent file fails" {
  run aichat --pipe --pipe-def non-existent-pipe.yaml --dry-run "test"
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
  run aichat -r pipe-test-role --dry-run "test"
  [ "$status" -eq 0 ]
}

@test "pipeline: --stage overrides model" {
  run aichat --pipe --stage %code%@non-existent-model --dry-run "test"
  [ "$status" -ne 0 ]
  [[ "$output" == *"references unknown model 'non-existent-model'"* ]]
}
