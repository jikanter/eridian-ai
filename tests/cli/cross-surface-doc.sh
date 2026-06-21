#!/usr/bin/env bats
#
# Phase 54G — cross-surface command map doc-lint.
# Keeps docs/features/cross-surface-commands.md honest: every mapped legacy REPL
# command is present, and every CLI counterpart it claims still exists in --help
# (so a flag rename surfaces here). No model/config needed.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
AICHAT="$AICHAT_BIN"
DOC="docs/features/cross-surface-commands.md"

@test "cross-surface doc exists and links to repl-pi" {
  [ -f "$DOC" ]
  grep -q "repl-pi.md" "$DOC"
}

@test "doc maps the core legacy REPL commands" {
  for cmd in .model .role .agent .macro .rag .session .info .file; do
    grep -qF -- "${cmd}" "$DOC" || {
      echo "missing legacy command in map: ${cmd}" >&2
      return 1
    }
  done
}

@test "every CLI counterpart the doc claims exists in --help" {
  help=$("${AICHAT}" --help)
  for flag in --model --role --agent --macro --rag --session --info --file --config-get --config-path; do
    echo "$help" | grep -qF -- "${flag}" || {
      echo "doc references CLI flag absent from --help: ${flag}" >&2
      return 1
    }
  done
}
