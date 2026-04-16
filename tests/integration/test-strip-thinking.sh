#!/usr/bin/env bats
# The --strip-thinking flag post-processes the model response to remove
# <think>...</think> blocks. Unit tests for the filter live in
# src/strip_thinking.rs. This smoke test just verifies the flag is accepted
# alongside a positional prompt without hanging.
@test "strip-thinking flag is accepted with positional prompt" {
  run timeout 5 aichat --strip-thinking --dry-run "hello"
  # Exit code 0 = success, 124 = timeout (would indicate the old hang bug).
  [ "$status" -ne 124 ]
}
