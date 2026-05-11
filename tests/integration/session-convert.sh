#!/usr/bin/env bats
#
# Phase 3: `aichat --convert-session <path> --to pi [--out PATH]`. Exercises
# the path-input form, the --out flag, and stdout streaming. The actual
# JSONL shape is covered by Rust unit tests in src/config/session.rs; here
# we just confirm the CLI plumbing wires the converter in and produces a
# file pi could ingest.
#
# `jq` is used to validate every line as JSON without trusting whitespace.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
case "$AICHAT_BIN" in
  /*) ;;
  *) AICHAT_BIN="$(pwd)/$AICHAT_BIN" ;;
esac

setup() {
  cd "$BATS_TEST_TMPDIR"
  cat >demo.yaml <<'YAML'
model: openai:gpt-4o-mini
messages:
  - role: user
    content: hello pi
  - role: assistant
    content: hello aichat
YAML
}

@test "convert-session: streams pi JSONL to stdout when --out is omitted" {
  run "$AICHAT_BIN" --convert-session demo.yaml --to pi
  [ "$status" -eq 0 ]

  # Three lines: header, user, assistant. Each must parse as JSON.
  count=$(printf '%s\n' "$output" | wc -l | tr -d ' ')
  [ "$count" -eq 3 ]
  printf '%s\n' "$output" | while IFS= read -r line; do
    echo "$line" | jq -e . >/dev/null
  done
}

@test "convert-session: --out writes a file with pi v3 header" {
  run "$AICHAT_BIN" --convert-session demo.yaml --to pi --out converted.jsonl
  [ "$status" -eq 0 ]
  [ -f converted.jsonl ]

  head -1 converted.jsonl | jq -e '
    .type == "session" and
    .version == 3 and
    (.id | type == "string") and
    (.cwd | type == "string")
  ' >/dev/null
}

@test "convert-session: parentId chains from null through each entry" {
  "$AICHAT_BIN" --convert-session demo.yaml --to pi --out chain.jsonl
  # Drop the header line; remaining lines are message entries.
  msgs=$(tail -n +2 chain.jsonl)

  first_parent=$(echo "$msgs" | head -1 | jq -r '.parentId')
  [ "$first_parent" = "null" ]

  # Second entry's parentId must equal first entry's id.
  first_id=$(echo "$msgs" | head -1 | jq -r '.id')
  second_parent=$(echo "$msgs" | sed -n 2p | jq -r '.parentId')
  [ "$first_id" = "$second_parent" ]
}

@test "convert-session: unknown --to target exits non-zero with an error" {
  run "$AICHAT_BIN" --convert-session demo.yaml --to claude-code
  [ "$status" -ne 0 ]
  [[ "$output" == *"is not supported"* ]]
}

@test "convert-session: missing file exits 1" {
  run "$AICHAT_BIN" --convert-session /nonexistent/path.yaml --to pi
  [ "$status" -eq 1 ]
  [[ "$output" == *"Failed to read"* ]]
}

@test "convert-session: bare name resolves under AICHAT_SESSIONS_DIR" {
  mkdir -p sessions
  cp demo.yaml sessions/by-name.yaml
  export AICHAT_SESSIONS_DIR="$(pwd)/sessions"

  run "$AICHAT_BIN" --convert-session by-name --to pi
  [ "$status" -eq 0 ]
  printf '%s\n' "$output" | head -1 | jq -e '.type == "session"' >/dev/null
}
