#!/usr/bin/env bats
#
# Phase 54B — `--color=auto|always|never`.
# The override must beat TTY detection: `--color=always` emits ANSI even when
# stdout is piped (non-TTY), while `auto`/`never` stay plain when piped.
#
# Self-contained and CI-safe: exercised on an isolated --config-get error path
# (unknown key) which colorizes via error_text on the light init path — no
# model, provider, network, or live instance.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
AICHAT="$AICHAT_BIN"
ESC=$'\033'

setup() {
  CFG_DIR="$(mktemp -d)"
  printf 'compress_threshold: 1\n' > "${CFG_DIR}/config.yaml"
}

teardown() {
  rm -rf "${CFG_DIR}"
}

# stderr captured through a pipe (2>&1 1>/dev/null) so stdout is dropped and
# only the (possibly colorized) error remains.
err() {
  bash -c "AICHAT_CONFIG_DIR='${CFG_DIR}' '${AICHAT}' --config-get badkey $1 2>&1 1>/dev/null"
}

@test "color=always emits ANSI through a pipe (overrides non-TTY)" {
  run err --color=always
  [[ "$output" == *"${ESC}["* ]]
}

@test "color=auto stays plain when piped" {
  run err --color=auto
  [[ "$output" != *"${ESC}["* ]]
}

@test "color=never stays plain through a pipe" {
  run err --color=never
  [[ "$output" != *"${ESC}["* ]]
}

@test "color rejects an invalid value" {
  run env AICHAT_CONFIG_DIR="${CFG_DIR}" "${AICHAT}" --color=rainbow --config-get badkey
  [ "$status" -ne 0 ]
}
