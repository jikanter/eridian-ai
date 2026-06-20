#!/usr/bin/env bats
#
# Phase 54B — `--color=auto|always|never`.
# The override must beat TTY detection: `--color=always` emits ANSI even when
# stdout is piped (non-TTY), while `auto` stays plain when piped. Exercised on
# the error path (deterministic, no network) which colorizes via error_text.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
AICHAT="$AICHAT_BIN"
BAD_ROLE="__no_such_role_54b__"
ESC=$'\033'

@test "color=always emits ANSI through a pipe (overrides non-TTY)" {
  run bash -c "'${AICHAT}' -r ${BAD_ROLE} --color=always x 2>&1 1>/dev/null"
  [[ "$output" == *"${ESC}["* ]]
}

@test "color=auto stays plain when piped" {
  run bash -c "'${AICHAT}' -r ${BAD_ROLE} --color=auto x 2>&1 1>/dev/null"
  [[ "$output" != *"${ESC}["* ]]
}

@test "color=never stays plain through a pipe" {
  run bash -c "'${AICHAT}' -r ${BAD_ROLE} --color=never x 2>&1 1>/dev/null"
  [[ "$output" != *"${ESC}["* ]]
}

@test "color rejects an invalid value" {
  run "${AICHAT}" --color=rainbow -r ${BAD_ROLE} x
  [ "$status" -ne 0 ]
}
