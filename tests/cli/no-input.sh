#!/usr/bin/env bats
#
# Phase 54C — non-interactive safety (--no-input, --yes/--force).
# Destructive batch ops must not proceed without confirmation, and must not
# hang when stdin is not a terminal: they refuse loudly with a usage exit code.
# --yes bypasses the gate. Exercised on --migrate-sessions, which removes each
# legacy .yaml after writing its .jsonl. The confirm gate runs before any
# parsing, so a dummy .yaml suffices.
#
# The resolve_confirm / can_prompt logic is unit-tested; these pin end-to-end.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
AICHAT="$AICHAT_BIN"
USAGE_EXIT=2

setup() {
  TMP_SESSIONS="$(mktemp -d)"
  printf 'dummy: true\n' > "${TMP_SESSIONS}/foo.yaml"
}

teardown() {
  rm -rf "${TMP_SESSIONS}"
}

@test "migrate-sessions refuses without --yes when stdin is not a TTY" {
  run env AICHAT_SESSIONS_DIR="${TMP_SESSIONS}" "${AICHAT}" --migrate-sessions
  [ "$status" -eq "${USAGE_EXIT}" ]
  [[ "$output" == *"Refusing to migrate"* ]]
  # The destructive action did not happen.
  [ -f "${TMP_SESSIONS}/foo.yaml" ]
}

@test "migrate-sessions refuses under --no-input" {
  run env AICHAT_SESSIONS_DIR="${TMP_SESSIONS}" "${AICHAT}" --migrate-sessions --no-input
  [ "$status" -eq "${USAGE_EXIT}" ]
  [ -f "${TMP_SESSIONS}/foo.yaml" ]
}

@test "migrate-sessions proceeds past the gate with --yes" {
  run env AICHAT_SESSIONS_DIR="${TMP_SESSIONS}" "${AICHAT}" --migrate-sessions --yes
  # --yes bypasses the confirmation gate (it attempts the migration); it must
  # not print the refusal message regardless of per-file parse outcome.
  [[ "$output" != *"Refusing to migrate"* ]]
}

@test "--force is an accepted alias for --yes" {
  run env AICHAT_SESSIONS_DIR="${TMP_SESSIONS}" "${AICHAT}" --migrate-sessions --force
  [[ "$output" != *"Refusing to migrate"* ]]
}

@test "no-input and yes are documented under the Core heading" {
  run "${AICHAT}" --help
  [ "$status" -eq 0 ]
  section=$(echo "$output" | awk '/^[A-Z][A-Za-z ]*:$/{sec=$0} sec=="Core:"{print}')
  echo "$section" | grep -qE -- "--no-input"
  echo "$section" | grep -qE -- "--yes"
}
