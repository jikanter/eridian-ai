#!/usr/bin/env bats
#
# Phase 54A — grouped `--help`.
# `aichat --help` must organize its ~90 flags into named sections (clap
# help_heading) instead of one flat `Options:` wall. Each section heading
# renders as `<Heading>:` on its own line in the help output.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
AICHAT="$AICHAT_BIN"

# The section headings every grouped flag must live under.
HEADINGS=(Core Input Output Execution Discovery Roles Knowledge Memory RAG MCP Server REPL Sessions Setup)

@test "help renders every expected section heading" {
  run "${AICHAT}" --help
  [ "$status" -eq 0 ]
  for h in "${HEADINGS[@]}"; do
    echo "$output" | grep -qE "^${h}:" || {
      echo "missing section heading: ${h}:" >&2
      return 1
    }
  done
}

# clap separates entries within a section by a blank line, so sections are
# delimited by the next heading (`^Word:`), not by blank lines.
@test "help groups knowledge flags under the Knowledge heading" {
  run "${AICHAT}" --help
  [ "$status" -eq 0 ]
  section=$(echo "$output" | awk '/^[A-Z][A-Za-z ]*:$/{sec=$0} sec=="Knowledge:"{print}')
  echo "$section" | grep -q -- "--knowledge-compile"
}

@test "help groups output flags under the Output heading" {
  run "${AICHAT}" --help
  [ "$status" -eq 0 ]
  section=$(echo "$output" | awk '/^[A-Z][A-Za-z ]*:$/{sec=$0} sec=="Output:"{print}')
  echo "$section" | grep -q -- "--output"
}

@test "help still lists representative flags after grouping" {
  run "${AICHAT}" --help
  [ "$status" -eq 0 ]
  for flag in --model --role --session --knowledge-compile --memory-reflect --serve --output; do
    echo "$output" | grep -q -- "$flag" || {
      echo "flag dropped from help: $flag" >&2
      return 1
    }
  done
}
