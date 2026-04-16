#!/usr/bin/env bats
#
# Phase 9D: Capability-aware preflight validation.
#
# These tests confirm that preflight rejects mismatches BEFORE any LLM call is
# made. They use --dry-run so no token is spent and ollama doesn't need to be
# reachable; preflight runs ahead of the dry-run short-circuit.
#
# Required ollama models (defined in your aichat config.yaml). The user's
# config registers each ollama model with explicit capability flags; without
# those flags this test cannot distinguish "capability missing" from "model
# misconfigured":
#   ollama:gemma3:4b           — supports_function_calling: false, no vision
#   ollama:gemma4:26b          — supports_function_calling: true,  vision: true
#   ollama:llama3.2-vision:11b — vision: true (function calling not required)

ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"
ASSETS_DIR="$BATS_TEST_DIRNAME/../../assets"

setup() {
  mkdir -p "$ROLES_DIR"
  cp "$ASSETS_DIR/preflight-test-role-with-tools.md" "$ROLES_DIR/preflight-test-role-with-tools.md"
  cp "$ASSETS_DIR/preflight-test-role-no-tools.md"   "$ROLES_DIR/preflight-test-role-no-tools.md"
}

teardown() {
  rm -f "$ROLES_DIR/preflight-test-role-with-tools.md"
  rm -f "$ROLES_DIR/preflight-test-role-no-tools.md"
}

@test "preflight: rejects use_tools on non-function-calling model" {
  run bash -c "./target/debug/aichat -r preflight-test-role-with-tools -m ollama:gemma3:4b --dry-run 'test' 2>&1"
  [ "$status" -eq 3 ]
  [[ "$output" == *"Preflight:"* ]]
  [[ "$output" == *"requires tool calling"* ]]
  [[ "$output" == *"ollama:gemma3:4b"* ]]
}

@test "preflight: accepts use_tools on function-calling model" {
  run bash -c "./target/debug/aichat -r preflight-test-role-with-tools -m ollama:gemma4:26b --dry-run 'test' 2>&1"
  [ "$status" -eq 0 ]
  [[ "$output" != *"Preflight:"* ]]
}

@test "preflight: passes role without use_tools on any model" {
  run bash -c "./target/debug/aichat -r preflight-test-role-no-tools -m ollama:gemma3:4b --dry-run 'test' 2>&1"
  [ "$status" -eq 0 ]
  [[ "$output" != *"Preflight:"* ]]
}

@test "preflight: rejects image input on non-vision model" {
  run bash -c "./target/debug/aichat -m ollama:gemma3:4b -f '$ASSETS_DIR/preflight-test-pixel.png' --dry-run 'describe' 2>&1"
  [ "$status" -eq 3 ]
  [[ "$output" == *"Preflight:"* ]]
  [[ "$output" == *"does not support vision"* ]]
  [[ "$output" == *"ollama:gemma3:4b"* ]]
}

@test "preflight: accepts image input on vision-capable model" {
  run bash -c "./target/debug/aichat -m ollama:llama3.2-vision:11b -f '$ASSETS_DIR/preflight-test-pixel.png' --dry-run 'describe' 2>&1"
  [ "$status" -eq 0 ]
  [[ "$output" != *"Preflight:"* ]]
}

# ---------------------------------------------------------------------------
# Regression: the Phase 9D refactor moved preflight ahead of the dry-run
# short-circuit and placed the streaming-path call inside a tokio::select!
# arm. These tests guard against the two ways that refactor could have
# broken unrelated paths.
# ---------------------------------------------------------------------------

@test "regression: --dry-run with default model still exits 0" {
  # The naked happy path: no role, no tools, no images. Catches the case where
  # preflight running ahead of the dry-run short-circuit accidentally fails a
  # previously-passing invocation.
  run bash -c "./target/debug/aichat --dry-run 'hello' 2>&1"
  [ "$status" -eq 0 ]
  [[ "$output" != *"Preflight:"* ]]
}

@test "regression: streaming dry-run with role does not hang on preflight failure" {
  # Before the select!-arm fix, a preflight Err on the streaming path left
  # render_stream waiting on its channel, timing out at ~10s. The explicit
  # timeout here will exit 124 if that regression comes back.
  run timeout 5 ./target/debug/aichat -r preflight-test-role-with-tools -m ollama:gemma3:4b --dry-run "test"
  [ "$status" -eq 3 ]     # preflight failure, config error
  [ "$status" -ne 124 ]   # not a timeout
}

@test "regression: streaming dry-run happy path completes promptly" {
  # Mirror of the above but with a compatible model — verifies the normal
  # streaming flow still terminates quickly with preflight inside the select.
  run timeout 5 ./target/debug/aichat -r preflight-test-role-no-tools -m ollama:gemma4:26b --dry-run "test"
  [ "$status" -eq 0 ]
  [ "$status" -ne 124 ]
}
