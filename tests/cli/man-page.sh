#!/usr/bin/env bats
#
# Phase 54A — generated man page.
# `aichat --man` emits a roff(7) man page built from the live clap
# definitions (no hand-maintained duplicate), so `aichat --man > man/aichat.1`
# stays in sync with the flags.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
AICHAT="$AICHAT_BIN"

@test "man emits a roff man-page header (.TH) for aichat" {
  run "${AICHAT}" --man
  [ "$status" -eq 0 ]
  echo "$output" | grep -qE '^\.TH aichat'
}

@test "man page is generated from clap defs (mentions a known flag)" {
  run "${AICHAT}" --man
  [ "$status" -eq 0 ]
  # roff escapes every '-' as '\-'; strip backslashes before matching.
  echo "$output" | tr -d '\\' | grep -q -- "--knowledge-compile"
}

@test "man page documents the section structure" {
  run "${AICHAT}" --man
  [ "$status" -eq 0 ]
  echo "$output" | grep -q "SYNOPSIS"
}
