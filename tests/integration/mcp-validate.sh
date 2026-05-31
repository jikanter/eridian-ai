#!/usr/bin/env bats
#
# Phase 31E: tests for `aichat --validate-mcp-config [PATH]`.
#
# Validates a portable Claude-Code-compatible mcp.json file against the
# rules in docs/architecture/integrated-architecture/SPEC-mcp-json-artifact.md
# § Validation:
#
#   1. Parses as JSON.
#   2. Has top-level `mcpServers` object.
#   3. Each entry sets `command` (stdio) XOR `url` (http/sse).
#   4. `args` is a string array.
#   5. `env`/`headers` are string-to-string maps.
#
# Exits 0 on valid, 1 on parse/schema failure, 2 when no file is found.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"

@test "validate-mcp-config: valid stdio entry" {
  cat >"$BATS_TEST_TMPDIR/mcp.json" <<'JSON'
{ "mcpServers": { "git": { "command": "uvx", "args": ["mcp-server-git"] } } }
JSON
  run "$AICHAT_BIN" --validate-mcp-config "$BATS_TEST_TMPDIR/mcp.json"
  [ "$status" -eq 0 ]
  [[ "$output" == *"ok:"* ]]
  [[ "$output" == *"[stdio] git"* ]]
}

@test "validate-mcp-config: valid http entry" {
  cat >"$BATS_TEST_TMPDIR/mcp.json" <<'JSON'
{ "mcpServers": { "remote": { "url": "https://mcp.example.com/sse" } } }
JSON
  run "$AICHAT_BIN" --validate-mcp-config "$BATS_TEST_TMPDIR/mcp.json"
  [ "$status" -eq 0 ]
  [[ "$output" == *"[http] remote"* ]]
}

@test "validate-mcp-config: -o json emits structured payload" {
  cat >"$BATS_TEST_TMPDIR/mcp.json" <<'JSON'
{
  "mcpServers": {
    "git":    { "command": "uvx", "args": ["mcp-server-git"] },
    "remote": { "url": "https://mcp.example.com/sse" }
  }
}
JSON
  run "$AICHAT_BIN" --validate-mcp-config "$BATS_TEST_TMPDIR/mcp.json" -o json
  [ "$status" -eq 0 ]
  [ "$(echo "$output" | jq -r .valid)" = "true" ]
  [ "$(echo "$output" | jq -r .servers)" = "2" ]
  [ "$(echo "$output" | jq -r .stdio)" = "1" ]
  [ "$(echo "$output" | jq -r .http)" = "1" ]
}

@test "validate-mcp-config: rejects entry with both command and url" {
  cat >"$BATS_TEST_TMPDIR/mcp.json" <<'JSON'
{ "mcpServers": { "x": { "command": "a", "url": "b" } } }
JSON
  run "$AICHAT_BIN" --validate-mcp-config "$BATS_TEST_TMPDIR/mcp.json"
  [ "$status" -eq 1 ]
  [[ "$output" == *"both \`command\` and \`url\`"* ]]
}

@test "validate-mcp-config: rejects entry with neither command nor url" {
  cat >"$BATS_TEST_TMPDIR/mcp.json" <<'JSON'
{ "mcpServers": { "x": {} } }
JSON
  run "$AICHAT_BIN" --validate-mcp-config "$BATS_TEST_TMPDIR/mcp.json"
  [ "$status" -eq 1 ]
  [[ "$output" == *"either \`command\`"* ]]
}

@test "validate-mcp-config: rejects malformed JSON" {
  printf '%s\n' "{ not json" > "$BATS_TEST_TMPDIR/mcp.json"
  run "$AICHAT_BIN" --validate-mcp-config "$BATS_TEST_TMPDIR/mcp.json"
  [ "$status" -eq 1 ]
  [[ "$output" == *"failed to parse"* ]]
}

@test "validate-mcp-config: missing explicit path exits 2" {
  run "$AICHAT_BIN" --validate-mcp-config "$BATS_TEST_TMPDIR/does-not-exist.json"
  [ "$status" -eq 2 ]
  [[ "$output" == *"no file exists there"* ]]
}

@test "validate-mcp-config: search order picks ./mcp.json in CWD" {
  cat >"$BATS_TEST_TMPDIR/mcp.json" <<'JSON'
{ "mcpServers": { "git": { "command": "uvx", "args": ["mcp-server-git"] } } }
JSON
  # Resolve the binary to an absolute path before cd; AICHAT_BIN may be a
  # relative path like ./target/debug/aichat.
  local bin
  bin="$(cd "$(dirname "$AICHAT_BIN")" && pwd)/$(basename "$AICHAT_BIN")"
  cd "$BATS_TEST_TMPDIR"
  run "$bin" --validate-mcp-config
  [ "$status" -eq 0 ]
  [[ "$output" == *"[stdio] git"* ]]
}

@test "validate-mcp-config: empty mcpServers object is valid" {
  cat >"$BATS_TEST_TMPDIR/mcp.json" <<'JSON'
{ "mcpServers": {} }
JSON
  run "$AICHAT_BIN" --validate-mcp-config "$BATS_TEST_TMPDIR/mcp.json"
  [ "$status" -eq 0 ]
  [[ "$output" == *"0 servers"* ]]
}
