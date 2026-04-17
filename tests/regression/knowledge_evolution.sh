#!/usr/bin/env bats

# Regression tests for Phase 27 (Knowledge Evolution).
# Covers the CLI surface only — the LLM-backed `--knowledge-reflect` and
# `--knowledge-curate` subcommands are exercised by in-vivo tests (they
# need a live model to produce candidates).

load common.bash

@test "phase27: --help advertises --knowledge-reflect" {
  run ./target/debug/aichat --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"--knowledge-reflect"* ]]
}

@test "phase27: --help advertises --knowledge-curate" {
  run ./target/debug/aichat --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"--knowledge-curate"* ]]
}

@test "phase27: --help advertises --knowledge-candidates" {
  run ./target/debug/aichat --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"--knowledge-candidates"* ]]
}

@test "phase27: --knowledge-reflect on missing KB errors clearly" {
  run ./target/debug/aichat --knowledge-reflect "kb-that-does-not-exist-phase27"
  [ "$status" -ne 0 ]
  [[ "$output" == *"Unknown knowledge base"* ]] || [[ "$output" == *"does not exist"* ]] \
    || [[ "$output" == *"not found"* ]] || [[ "$output" == *"Unknown"* ]]
}

@test "phase27: --knowledge-curate requires existing KB" {
  run ./target/debug/aichat --knowledge-curate "kb-that-does-not-exist-phase27"
  [ "$status" -ne 0 ]
}

@test "phase27: --knowledge-candidates requires --knowledge-curate" {
  run ./target/debug/aichat --knowledge-candidates /tmp/candidates.json
  [ "$status" -ne 0 ]
  [[ "$output" == *"--knowledge-curate"* ]] || [[ "$output" == *"requires"* ]]
}

@test "phase27: --knowledge-trace is accepted as optional flag" {
  # No subcommand issued → we should not crash parsing args.
  run ./target/debug/aichat --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"--knowledge-trace"* ]]
}

@test "phase27: roadmap doc present and marks rewritten status" {
  [ -f "docs/roadmap/phase-27-knowledge-evolution.md" ]
  run grep -i "Knowledge Evolution" docs/roadmap/phase-27-knowledge-evolution.md
  [ "$status" -eq 0 ]
}
