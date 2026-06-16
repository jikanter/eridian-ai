#!/usr/bin/env bats

# Integration tests for Phase 28: Agent Composability.
#
# These drive the real binary at the `--dry-run` surface (no model call), so
# they are deterministic. They guard the *config-resolution* path:
#   - 28B `react_max_steps:` frontmatter parses without breaking role loading.
#   - 28A an agent named in `use_tools` resolves and surfaces in the role.
# 28C `%%` macro-output substitution is covered by deterministic unit tests
# (`substitute_prev_output`); a binary-level macro test would need a live model
# for the AI-output step, so it is intentionally not driven here.
# The live delegation path (the model actually calling an agent-as-tool and the
# sub-agent's react loop executing) needs a model and lands with the Phase 43
# test harness — same deferral as the Phase 42E provider-event e2e.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"

setup() {
    export AICHAT_CONFIG_DIR=$(mktemp -d)
    export AICHAT_FUNCTIONS_DIR=$(mktemp -d)
    mkdir -p "$AICHAT_CONFIG_DIR/roles"
    cat > "$AICHAT_CONFIG_DIR/config.yaml" <<EOF
model: openai:gpt-4o
clients:
  - type: openai
    api_key: sk-xxx
EOF
    # 28A: register a known agent (list_agents reads agents.txt).
    printf 'echo-agent\n' > "$AICHAT_FUNCTIONS_DIR/agents.txt"
}

teardown() {
    rm -rf "$AICHAT_CONFIG_DIR" "$AICHAT_FUNCTIONS_DIR"
}

@test "phase-28b: react_max_steps frontmatter loads the role cleanly" {
    cat > "$AICHAT_CONFIG_DIR/roles/capped.md" <<EOF
---
react_max_steps: 2
---
You finish quickly.
EOF
    run "$AICHAT_BIN" --dry-run --role capped "hello"
    [ "$status" -eq 0 ]
    [[ "$output" =~ "Resolved Role: capped" ]]
}

@test "phase-28a: an agent named in use_tools surfaces in the resolved role" {
    cat > "$AICHAT_CONFIG_DIR/roles/delegator.md" <<EOF
---
react_max_steps: 3
use_tools: echo-agent
---
You delegate work to specialist agents.
EOF
    run "$AICHAT_BIN" --dry-run --role delegator "hello"
    [ "$status" -eq 0 ]
    [[ "$output" =~ "echo-agent" ]]
}
