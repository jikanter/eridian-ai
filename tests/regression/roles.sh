#!/usr/bin/env bats

# Regression tests for Role features described in Role-Guide.md
load common.bash

ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"

setup() {
  mkdir -p "$ROLES_DIR"
}

@test "roles: embedded prompt replaces __INPUT__" {
  cat > "$ROLES_DIR/test-embedded.md" <<EOF
---
---
convert __INPUT__ to emoji
EOF
  # We use --dry-run and check if the output (which for dry-run shows the message) contains the replaced input.
  # Note: aichat dry-run output format might vary, but it usually shows the prompt.
  # When substituting model, the behavior changes, so we only check the output contains the prompt if NO model substitute is present.
  run_aichat -r test-embedded --dry-run "angry"
  [ "$status" -eq 0 ]
  if [[ -z "$AICHAT_TEST_MODEL" ]]; then
    [[ "$output" == *"convert angry to emoji"* ]]
  fi
  rm "$ROLES_DIR/test-embedded.md"
}

@test "roles: system prompt" {
  cat > "$ROLES_DIR/test-system.md" <<EOF
---
---
convert my words to emoji
EOF
  run_aichat -r test-system --dry-run "angry"
  [ "$status" -eq 0 ]
  if [[ -z "$AICHAT_TEST_MODEL" ]]; then
    [[ "$output" == *"convert my words to emoji"* ]]
    [[ "$output" == *"angry"* ]]
  fi
  rm "$ROLES_DIR/test-system.md"
}

@test "roles: built-in %code% role works" {
  run_aichat -r %code% --dry-run "print hello"
  [ "$status" -eq 0 ]
}

@test "roles: --list-roles shows custom role" {
  cat > "$ROLES_DIR/test-list.md" <<EOF
---
---
test
EOF
  run ./target/debug/aichat --list-roles
  [ "$status" -eq 0 ]
  [[ "$output" == *"test-list"* ]]
  rm "$ROLES_DIR/test-list.md"
}
