#!/usr/bin/env bats
#
# Phase 54B — `-q/--quiet`.
# Quiet suppresses the spinner and the cost line without touching stdout.
# The suppression itself is TTY/runtime-bound (spinner needs a terminal, the
# cost line needs a model call), so it is covered by unit tests
# (spinner_suppressed / should_show_cost). These checks pin the CLI surface:
# the flag parses, lives under Output in help, and leaves stdout intact.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
AICHAT="$AICHAT_BIN"

@test "quiet short and long forms are accepted" {
  run "${AICHAT}" -q --list-roles
  [ "$status" -eq 0 ]
  run "${AICHAT}" --quiet --list-roles
  [ "$status" -eq 0 ]
}

@test "quiet leaves stdout payload intact" {
  plain=$("${AICHAT}" --list-roles)
  quiet=$("${AICHAT}" -q --list-roles)
  [ "$plain" = "$quiet" ]
}

@test "quiet is documented under the Output heading" {
  run "${AICHAT}" --help
  [ "$status" -eq 0 ]
  section=$(echo "$output" | awk '/^[A-Z][A-Za-z ]*:$/{sec=$0} sec=="Output:"{print}')
  echo "$section" | grep -qE -- "--quiet"
}
