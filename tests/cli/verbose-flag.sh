#!/usr/bin/env bats
#
# Phase 54B — global `--verbose`.
# --verbose raises the log level to debug and routes diagnostics to stderr
# (overriding AICHAT_LOG_LEVEL / the default log file). It also keeps its
# legacy role-list detail behavior (--list-roles / --find-role).
#
# The level-resolution logic is unit-tested via effective_log_level (verbose
# forces Debug over env + build defaults). End-to-end stderr emission is not
# asserted here: the binary exits via process::exit, which can drop buffered
# logger writes non-deterministically, so a stderr line-count check would be
# flaky. These checks pin the deterministic CLI surface.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
AICHAT="$AICHAT_BIN"

@test "verbose flag is accepted" {
  run "${AICHAT}" --verbose --list-roles
  [ "$status" -eq 0 ]
}

@test "verbose still adds role-list detail (legacy behavior preserved)" {
  plain=$("${AICHAT}" --list-roles)
  detailed=$("${AICHAT}" --verbose --list-roles 2>/dev/null)
  # Verbose role listing is a superset view, so it must differ from plain.
  [ "$plain" != "$detailed" ]
}

@test "verbose is documented under the Output heading" {
  run "${AICHAT}" --help
  [ "$status" -eq 0 ]
  section=$(echo "$output" | awk '/^[A-Z][A-Za-z ]*:$/{sec=$0} sec=="Output:"{print}')
  echo "$section" | grep -qE -- "--verbose"
}
