#!/usr/bin/env bats

# Phase 13: Authoring & Teaching
#   13A  --fork-role <source> <new-name>
#   13B  schema-mismatch teaching errors in pipelines
#   13C  built-in guardrail role examples
#   13D  --explain-role <name>
#
# All artifacts are prefixed `phase13-` and removed in teardown so the tests
# leave the roles directory as they found it. No LLM calls: fork/explain are
# read-only-ish, and the 13B path bails at input-schema validation before any
# model request.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"

setup() {
  mkdir -p "$ROLES_DIR"
  cat > "$ROLES_DIR/phase13-base.md" <<EOF
---
description: A base role to fork from.
temperature: 0.3
capabilities: [analysis]
---
You are a careful analyst. __INPUT__
EOF
  cat > "$ROLES_DIR/phase13-needs-shape.md" <<EOF
---
description: Needs a structured object.
input_schema:
  type: object
  properties:
    content: { type: string }
    language: { type: string }
  required: [content, language]
---
Format __INPUT__
EOF
}

teardown() {
  rm -f "$ROLES_DIR/phase13-base.md"
  rm -f "$ROLES_DIR/phase13-needs-shape.md"
  rm -f "$ROLES_DIR/phase13-fork.md"
  rm -f "$ROLES_DIR/phase13-fork-json.md"
}

# ----- 13A: --fork-role -----

@test "fork-role: creates an extends file" {
  run "$AICHAT_BIN" --fork-role phase13-base phase13-fork
  [ "$status" -eq 0 ]
  [[ "$output" == *"Created"* ]]
  [ -f "$ROLES_DIR/phase13-fork.md" ]
  run cat "$ROLES_DIR/phase13-fork.md"
  [[ "$output" == *"extends: phase13-base"* ]]
  # Overridable fields are present but commented out.
  [[ "$output" == *"# model:"* ]]
  [[ "$output" == *"# temperature: 0.3"* ]]
}

@test "fork-role: refuses to overwrite an existing role" {
  run "$AICHAT_BIN" --fork-role phase13-base phase13-fork
  [ "$status" -eq 0 ]
  run "$AICHAT_BIN" --fork-role phase13-base phase13-fork
  [ "$status" -ne 0 ]
  [[ "$output" == *"already exists"* ]]
}

@test "fork-role: unknown source fails" {
  run "$AICHAT_BIN" --fork-role phase13-does-not-exist phase13-fork
  [ "$status" -ne 0 ]
  [[ "$output" == *"could not be resolved"* ]]
}

@test "fork-role: -o json reports the created path" {
  run "$AICHAT_BIN" --fork-role phase13-base phase13-fork-json -o json
  [ "$status" -eq 0 ]
  [[ "$output" == *'"source": "phase13-base"'* ]]
  [[ "$output" == *'"new_name": "phase13-fork-json"'* ]]
}

@test "fork-role: forked role resolves and inherits the parent" {
  run "$AICHAT_BIN" --fork-role phase13-base phase13-fork
  [ "$status" -eq 0 ]
  run "$AICHAT_BIN" --explain-role phase13-fork
  [ "$status" -eq 0 ]
  [[ "$output" == *"extends: phase13-base"* ]]
}

# ----- 13D: --explain-role -----

@test "explain-role: shows composition" {
  run "$AICHAT_BIN" --explain-role phase13-base
  [ "$status" -eq 0 ]
  [[ "$output" == *"Role: phase13-base"* ]]
  [[ "$output" == *"capabilities: [analysis]"* ]]
  [[ "$output" == *"in: "* ]]
}

@test "explain-role: -o json has composition keys" {
  run "$AICHAT_BIN" --explain-role phase13-needs-shape -o json
  [ "$status" -eq 0 ]
  [[ "$output" == *'"input": "json{content, language}"'* ]]
  [[ "$output" == *'"has_pipeline": false'* ]]
}

@test "explain-role: unknown role fails" {
  run "$AICHAT_BIN" --explain-role phase13-nope
  [ "$status" -ne 0 ]
  [[ "$output" == *"Unknown role"* ]]
}

# ----- 13C: built-in guardrail examples -----

@test "guardrails: ship as built-in roles" {
  run "$AICHAT_BIN" --list-roles
  [ "$status" -eq 0 ]
  [[ "$output" == *"guardrail-pii"* ]]
  [[ "$output" == *"guardrail-injection"* ]]
  [[ "$output" == *"guardrail-topic"* ]]
}

@test "guardrails: discoverable by capability" {
  run "$AICHAT_BIN" --find-role --capability guardrail
  [ "$status" -eq 0 ]
  [[ "$output" == *"guardrail-pii"* ]]
  [[ "$output" == *"guardrail-injection"* ]]
  [[ "$output" == *"guardrail-topic"* ]]
}

@test "guardrails: declare a structured output port" {
  run "$AICHAT_BIN" --explain-role guardrail-pii -o json
  [ "$status" -eq 0 ]
  [[ "$output" == *'"output": "json{safe, redacted, findings}"'* ]]
}

# ----- 13B: schema-mismatch teaching error -----

@test "teaching-error: pipeline input mismatch shows the field delta and hint" {
  # Feed a JSON object with the wrong shape so the producer→consumer field
  # delta is computable (missing + extra).
  run "$AICHAT_BIN" --pipe --stage phase13-needs-shape '{"text":"x","summary":"y"}'
  [ "$status" -ne 0 ]
  [[ "$output" == *"Schema input validation failed"* ]]
  [[ "$output" == *"Stage 1 expects"* ]]
  [[ "$output" == *"Missing fields: content, language"* ]]
  [[ "$output" == *"Extra fields: text, summary"* ]]
  [[ "$output" == *"--fork-role"* ]]
}
