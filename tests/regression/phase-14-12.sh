#!/usr/bin/env bats

# Phase 14 (Capability Manifests) + Phase 12 (Discoverability) integration coverage.
# These exercise --find-role, --list-roles --verbose, and --dry-run preview
# without needing a live LLM (preview emits to stderr before any API call).
load common.bash

ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"

setup() {
  mkdir -p "$ROLES_DIR"
  cat > "$ROLES_DIR/p14-reviewer.md" <<'EOF'
---
description: Phase 14/12 reviewer fixture
capabilities: [code-review, security-audit]
input_schema:
  type: object
  properties:
    code: { type: string }
    language: { type: string }
output_schema:
  type: object
  properties:
    issues: { type: array }
    severity: { type: string }
---
You are a code reviewer.
EOF
  cat > "$ROLES_DIR/p14-summarize.md" <<'EOF'
---
description: Phase 14/12 summarize fixture
capabilities: [summarization]
---
Summarize __INPUT__.
EOF
}

teardown() {
  rm -f "$ROLES_DIR/p14-reviewer.md" "$ROLES_DIR/p14-summarize.md"
}

@test "phase-14: --find-role --capability finds role by tag" {
  run "$AICHAT_BIN" --find-role --capability code-review
  [ "$status" -eq 0 ]
  [[ "$output" == *"p14-reviewer"* ]]
  [[ "$output" != *"p14-summarize"* ]]
}

@test "phase-14: --find-role --capability is case-insensitive substring" {
  run "$AICHAT_BIN" --find-role --capability AUDIT
  [ "$status" -eq 0 ]
  [[ "$output" == *"p14-reviewer"* ]]
}

@test "phase-14: --find-role --accepts json --produces json filters by port" {
  run "$AICHAT_BIN" --find-role --accepts json --produces json
  [ "$status" -eq 0 ]
  [[ "$output" == *"p14-reviewer"* ]]
  [[ "$output" != *"p14-summarize"* ]]
}

@test "phase-14: --find-role with no filters is rejected" {
  run "$AICHAT_BIN" --find-role
  [ "$status" -ne 0 ]
  [[ "$output" == *"requires at least one"* ]]
}

@test "phase-14: --find-role --verbose shows capabilities and ports" {
  run "$AICHAT_BIN" --find-role --capability code-review --verbose
  [ "$status" -eq 0 ]
  [[ "$output" == *"in: json{code, language}"* ]]
  [[ "$output" == *"out: json{issues, severity}"* ]]
  [[ "$output" == *"capabilities: [code-review, security-audit]"* ]]
}

@test "phase-14: --find-role -o json emits structured records" {
  run "$AICHAT_BIN" --find-role --capability code-review -o json --verbose
  [ "$status" -eq 0 ]
  # The JSON should round-trip through jq if present; otherwise just substring.
  [[ "$output" == *'"name": "p14-reviewer"'* ]]
  [[ "$output" == *'"capabilities"'* ]]
  [[ "$output" == *'"input"'* ]]
  [[ "$output" == *'"output"'* ]]
}

@test "phase-12: --list-roles --verbose shows ports for plain roles" {
  run "$AICHAT_BIN" --list-roles --verbose
  [ "$status" -eq 0 ]
  [[ "$output" == *"p14-summarize"* ]]
  [[ "$output" == *"in: any"* ]]
  [[ "$output" == *"out: text"* ]]
}

@test "phase-12: --list-roles --capability narrows the list" {
  run "$AICHAT_BIN" --list-roles --capability summarization
  [ "$status" -eq 0 ]
  [[ "$output" == *"p14-summarize"* ]]
  [[ "$output" != *"p14-reviewer"* ]]
}

@test "phase-12: --dry-run preview hits stderr, not stdout" {
  # Use a role whose input_schema is satisfied by valid JSON so we reach the
  # preview point. Stdout should hold the assembled prompt; stderr the preview.
  out_file="$(mktemp)"
  err_file="$(mktemp)"
  "$AICHAT_BIN" -r p14-reviewer --dry-run '{"code":"x=1","language":"py"}' \
    >"$out_file" 2>"$err_file"
  status=$?
  [ "$status" -eq 0 ]
  # Preview lines on stderr
  grep -q "Resolved Role: p14-reviewer" "$err_file"
  grep -q "in: json{code, language}" "$err_file"
  grep -q "capabilities: \[code-review, security-audit\]" "$err_file"
  # No preview noise on stdout — assembled prompt only
  ! grep -q "Resolved Role:" "$out_file"
  rm -f "$out_file" "$err_file"
}

@test "phase-12: --dry-run preview is silent for bare prompt (no role)" {
  err_file="$(mktemp)"
  "$AICHAT_BIN" --dry-run "hello" 2>"$err_file" >/dev/null
  ! grep -q "Resolved Role:" "$err_file"
  rm -f "$err_file"
}
