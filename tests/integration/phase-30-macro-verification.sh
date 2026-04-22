#!/usr/bin/env bats

# Integration tests for Phase 30: Macro Compilation & Feedback Loop
# Verifies that client registration and listing still work correctly after macro refactor.

setup() {
    export AICHAT_CONFIG_DIR=$(mktemp -d)
    # Create a dummy config
    cat > "$AICHAT_CONFIG_DIR/config.yaml" <<EOF
model: openai:gpt-4o
clients:
  - type: openai
    api_key: sk-xxx
EOF
}

teardown() {
    rm -rf "$AICHAT_CONFIG_DIR"
}

@test "phase-30: aichat --info lists config correctly" {
    run ./target/debug/aichat --info
    [ "$status" -eq 0 ]
    [[ "$output" =~ "model" ]]
    [[ "$output" =~ "config_file" ]]
}

@test "phase-30: aichat --list-models lists some models" {
    # Check if we can list models (verifies macro-generated list_models and registry)
    run ./target/debug/aichat --list-models
    [ "$status" -eq 0 ]
    [ -n "$output" ]
}
