#!/usr/bin/env bats
#
# Phase 15C: `--check` — validate a role or pipeline definition without
# executing it. Phase 15B's cross-stage JSON Schema containment is surfaced
# here at every adjacent stage boundary.
#
# These tests never call a model: `--check` is deterministic and zero-token.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"

setup() {
  mkdir -p "$ROLES_DIR"

  # Producer: emits {text, metadata}, both required.
  cat > "$ROLES_DIR/check-extract.md" <<'EOF'
---
output_schema:
  type: object
  properties:
    text: { type: string }
    metadata: { type: object }
  required: [text, metadata]
---
Extract.
EOF

  # Consumer: requires {content, language} — incompatible with the producer.
  cat > "$ROLES_DIR/check-review.md" <<'EOF'
---
input_schema:
  type: object
  properties:
    content: { type: string }
    language: { type: string }
  required: [content, language]
---
Review.
EOF

  # Producer whose output satisfies the consumer below.
  cat > "$ROLES_DIR/check-producer.md" <<'EOF'
---
output_schema:
  type: object
  properties:
    issues: { type: array }
    severity: { type: string }
  required: [issues, severity]
---
Produce.
EOF

  # Consumer needing only {issues}: an open superset of the producer.
  cat > "$ROLES_DIR/check-consumer.md" <<'EOF'
---
input_schema:
  type: object
  properties:
    issues: { type: array }
  required: [issues]
---
Consume.
EOF

  # Incompatible pipeline.
  cat > "$ROLES_DIR/check-bad-pipe.md" <<'EOF'
---
pipeline:
  - role: check-extract
  - role: check-review
---
EOF

  # Compatible pipeline.
  cat > "$ROLES_DIR/check-good-pipe.md" <<'EOF'
---
pipeline:
  - role: check-producer
  - role: check-consumer
---
EOF

  # Non-sequential (fan-out) pipeline over a built-in role.
  cat > "$ROLES_DIR/check-par-pipe.md" <<'EOF'
---
pipeline:
  - parallel:
      - role: "%code%"
      - role: "%code%"
---
EOF

  # Role with a malformed input_schema (type must be a keyword, not a number).
  cat > "$ROLES_DIR/check-badschema.md" <<'EOF'
---
input_schema:
  type: 12345
---
Bad.
EOF
}

teardown() {
  rm -f "$ROLES_DIR/check-extract.md" \
        "$ROLES_DIR/check-review.md" \
        "$ROLES_DIR/check-producer.md" \
        "$ROLES_DIR/check-consumer.md" \
        "$ROLES_DIR/check-bad-pipe.md" \
        "$ROLES_DIR/check-good-pipe.md" \
        "$ROLES_DIR/check-par-pipe.md" \
        "$ROLES_DIR/check-badschema.md"
}

@test "check: valid single role passes" {
  run "$AICHAT_BIN" --check -r check-extract
  [ "$status" -eq 0 ]
  [[ "$output" == *"check passed"* ]]
}

@test "check: single role reports its ports" {
  run "$AICHAT_BIN" --check -r check-extract
  [ "$status" -eq 0 ]
  [[ "$output" == *"json{text, metadata}"* ]]
}

@test "check: incompatible pipeline fails with missing fields" {
  run "$AICHAT_BIN" --check -r check-bad-pipe
  [ "$status" -eq 3 ]
  [[ "$output" == *"FAIL"* ]]
  [[ "$output" == *"content"* ]]
  [[ "$output" == *"language"* ]]
  [[ "$output" == *"check failed"* ]]
}

@test "check: compatible pipeline passes" {
  run "$AICHAT_BIN" --check -r check-good-pipe
  [ "$status" -eq 0 ]
  [[ "$output" == *"check passed"* ]]
}

@test "check: unknown stage fails preflight" {
  run "$AICHAT_BIN" --check --pipe --stage non-existent-role
  [ "$status" -ne 0 ]
  [[ "$output" == *"unknown entity"* ]]
}

@test "check: ad-hoc incompatible --stage pipeline fails" {
  run "$AICHAT_BIN" --check --pipe --stage check-extract --stage check-review
  [ "$status" -eq 3 ]
  [[ "$output" == *"FAIL"* ]]
}

@test "check: json output is machine-readable" {
  run "$AICHAT_BIN" --check -r check-bad-pipe -o json
  [ "$status" -eq 3 ]
  [[ "$output" == *"\"valid\": false"* ]] || [[ "$output" == *"\"valid\":false"* ]]
  [[ "$output" == *"check-extract"* ]]
}

@test "check: json output for a valid pipeline reports valid true" {
  run "$AICHAT_BIN" --check -r check-good-pipe -o json
  [ "$status" -eq 0 ]
  [[ "$output" == *"\"valid\": true"* ]] || [[ "$output" == *"\"valid\":true"* ]]
}

@test "check: no target is a usage error" {
  run "$AICHAT_BIN" --check
  [ "$status" -eq 2 ]
  [[ "$output" == *"requires"* ]]
}

@test "check: non-sequential pipeline skips containment but passes" {
  run "$AICHAT_BIN" --check -r check-par-pipe
  [ "$status" -eq 0 ]
  [[ "$output" == *"non-sequential"* ]]
  [[ "$output" == *"check passed"* ]]
}

@test "check: malformed schema is reported" {
  run "$AICHAT_BIN" --check -r check-badschema
  [ "$status" -eq 3 ]
  [[ "$output" == *"not a valid JSON Schema"* ]]
}
