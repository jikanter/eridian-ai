#!/usr/bin/env bats
#
# Phase 54D — "Did you mean?" suggestions.
# An unknown role name yields a Levenshtein-nearest suggestion from the real
# candidate list; a far-off name yields no noisy guess. The distance logic is
# unit-tested (nearest_match); these pin the end-to-end error text.
#
# Self-contained and CI-safe: an isolated AICHAT_CONFIG_DIR with one role file,
# exercised through --explain-role, which resolves a role on the light (info)
# init path — no model, provider, network, or live instance.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
AICHAT="$AICHAT_BIN"

setup() {
  CFG_DIR="$(mktemp -d)"
  printf 'compress_threshold: 1\n' > "${CFG_DIR}/config.yaml"
  mkdir -p "${CFG_DIR}/roles"
  printf 'hello\n' > "${CFG_DIR}/roles/summarize.md"
}

teardown() {
  rm -rf "${CFG_DIR}"
}

@test "unknown role near a real role suggests it" {
  run env AICHAT_CONFIG_DIR="${CFG_DIR}" "${AICHAT}" --explain-role summarise
  [ "$status" -ne 0 ]
  [[ "$output" == *"Unknown role"* ]]
  [[ "$output" == *"Did you mean \`summarize\`?"* ]]
}

@test "wildly unknown role gives no suggestion" {
  run env AICHAT_CONFIG_DIR="${CFG_DIR}" "${AICHAT}" --explain-role zzzzzzzzzzzzzzzz
  [ "$status" -ne 0 ]
  [[ "$output" == *"Unknown role"* ]]
  [[ "$output" != *"Did you mean"* ]]
}
