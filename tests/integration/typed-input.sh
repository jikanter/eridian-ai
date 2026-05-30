#!/usr/bin/env bats

# Phase 33: Typed Input. End-to-end checks against the real binary under
# --dry-run (offline). Covers the unification core (33A/33B/33E: schema defaults
# fill {{slots}}, type-aware rendering, variables as sugar) and the 33C CLI/stdin
# surface (type coercion against the schema, stdin routing into a `body` slot).

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"

setup() {
  mkdir -p "$ROLES_DIR"
}

teardown() {
  rm -f "$ROLES_DIR/p33-typed.md" "$ROLES_DIR/p33-stdin.md" "$ROLES_DIR/p33-strict.md"
}

@test "typed-input: schema defaults fill slots and arrays render compact (33A/33B)" {
  cat > "$ROLES_DIR/p33-typed.md" <<EOF
---
input_schema:
  type: object
  properties:
    target: { type: string, default: "main" }
    depth: { type: integer, default: 3 }
    tags: { type: array, default: ["security", "perf"] }
---
Review {{target}} at depth {{depth}}. Tags: {{tags}}.
EOF
  run "$AICHAT_BIN" -r p33-typed --dry-run '{"x":1}'
  [ "$status" -eq 0 ]
  [[ "$output" == *'Review main at depth 3. Tags: ["security","perf"].'* ]]
}

@test "typed-input: -v overrides a schema default (33A)" {
  cat > "$ROLES_DIR/p33-typed.md" <<EOF
---
input_schema:
  type: object
  properties:
    target: { type: string, default: "main" }
---
Target is {{target}}.
EOF
  run "$AICHAT_BIN" -r p33-typed -v target=release --dry-run '{"x":1}'
  [ "$status" -eq 0 ]
  [[ "$output" == *"Target is release."* ]]
}

@test "typed-input: bad -v for an integer slot errors (33C coercion)" {
  cat > "$ROLES_DIR/p33-typed.md" <<EOF
---
input_schema:
  type: object
  properties:
    depth: { type: integer, default: 3 }
---
Depth {{depth}}.
EOF
  run "$AICHAT_BIN" -r p33-typed -v depth=abc --dry-run '{"x":1}'
  [ "$status" -ne 0 ]
  [[ "$output" == *"depth"* ]]
  [[ "$output" == *"integer"* ]]
}

@test "typed-input: stdin routes into a source:stdin slot, no message validation (33C)" {
  cat > "$ROLES_DIR/p33-stdin.md" <<EOF
---
input_schema:
  type: object
  properties:
    target: { type: string, default: "main" }
    body: { type: string, x-aichat: { source: stdin } }
---
Review {{target}}. Input:
{{body}}
EOF
  run bash -c "printf 'free text diff' | '$AICHAT_BIN' -r p33-stdin --dry-run"
  [ "$status" -eq 0 ]
  [[ "$output" == *"Review main. Input:"* ]]
  [[ "$output" == *"free text diff"* ]]
}

@test "typed-input: a plain input_schema role still validates the message" {
  cat > "$ROLES_DIR/p33-strict.md" <<EOF
---
input_schema:
  type: object
  properties:
    name: { type: string }
  required: [name]
---
Hello {{name}}.
EOF
  run bash -c "printf 'not json' | '$AICHAT_BIN' -r p33-strict --dry-run"
  [ "$status" -ne 0 ]
  [[ "$output" == *"validation failed"* ]]
}
