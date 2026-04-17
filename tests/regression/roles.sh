#!/usr/bin/env bats

# Regression tests for Role features described in Role-Guide.md

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
  run ./target/debug/aichat -r test-embedded --dry-run "angry"
  [ "$status" -eq 0 ]
  [[ "$output" == *"convert angry to emoji"* ]]
  rm "$ROLES_DIR/test-embedded.md"
}

@test "roles: system prompt" {
  cat > "$ROLES_DIR/test-system.md" <<EOF
---
---
convert my words to emoji
EOF
  run ./target/debug/aichat -r test-system --dry-run "angry"
  [ "$status" -eq 0 ]
  [[ "$output" == *"convert my words to emoji"* ]]
  [[ "$output" == *"angry"* ]]
  rm "$ROLES_DIR/test-system.md"
}

@test "roles: built-in %code% role works" {
  run ./target/debug/aichat -r %code% --dry-run "print hello"
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
