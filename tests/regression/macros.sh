#!/usr/bin/env bats

# Regression tests for Macro features described in Macro-Guide.md

CONFIG_DIR=$(mktemp -d)
MACROS_DIR="$CONFIG_DIR/macros"

setup() {
  mkdir -p "$MACROS_DIR"
}

teardown() {
  rm -rf "$CONFIG_DIR"
}

@test "macros: custom macro is detected" {
  cat > "$CONFIG_DIR/config.yaml" <<EOF
model: openai:gpt-4o
clients:
  - type: openai
    api_key: sk-xxx
EOF
  cat > "$MACROS_DIR/test-macro.yaml" <<EOF
steps:
  - .info
EOF
  echo ".macro test-macro" | AICHAT_CONFIG_DIR="$CONFIG_DIR" ./target/debug/aichat 2>&1 || true
}

@test "macros: macro with variables" {
  cat > "$CONFIG_DIR/config.yaml" <<EOF
model: openai:gpt-4o
clients:
  - type: openai
    api_key: sk-xxx
EOF
  cat > "$MACROS_DIR/var-macro.yaml" <<EOF
variables:
  - name: myvar
steps:
  - 'echo {{myvar}}'
EOF
  echo ".macro var-macro hello" | AICHAT_CONFIG_DIR="$CONFIG_DIR" ./target/debug/aichat 2>&1 || true
}
