#!/usr/bin/env bats
#
# Phase 54D — "Did you mean?" suggestions.
# An unknown role/agent name yields a Levenshtein-nearest suggestion drawn from
# the real candidate list; a far-off name yields no noisy guess. The distance
# logic is unit-tested (nearest_match); these pin the end-to-end error text.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
AICHAT="$AICHAT_BIN"

@test "unknown role near a real role suggests it" {
  # Derive a real role and introduce a one-char typo so the suggestion is exact.
  real=$("${AICHAT}" --list-roles 2>/dev/null | grep -vE '^%' | head -1)
  [ -n "$real" ]
  run bash -c "'${AICHAT}' -r '${real}x' hi 2>&1 1>/dev/null"
  [[ "$output" == *"Unknown role"* ]]
  [[ "$output" == *"Did you mean \`${real}\`?"* ]]
}

@test "wildly unknown role gives no suggestion" {
  run bash -c "'${AICHAT}' -r zzzzzzzzzzzzzzzz hi 2>&1 1>/dev/null"
  [[ "$output" == *"Unknown role"* ]]
  [[ "$output" != *"Did you mean"* ]]
}

@test "unknown agent near a real agent suggests it" {
  real=$("${AICHAT}" --list-agents 2>/dev/null | head -1)
  if [ -z "$real" ]; then
    skip "no agents installed"
  fi
  run bash -c "'${AICHAT}' -a '${real}x' hi 2>&1 1>/dev/null"
  [[ "$output" == *"Did you mean \`${real}\`?"* ]]
}
