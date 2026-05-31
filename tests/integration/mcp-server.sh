#!/usr/bin/env bats
#
# MCP server-side and mcp_servers: config protocol tests.
#
# Covers the gap between client-side coverage (tests/integration/mcp-client
# scenarios in compatibility.rs and docs/demos/demo-mcp-client.md) and the
# runtime behavior when:
#
#   1. aichat runs as `aichat --mcp` (stdio MCP server exposing functions)
#   2. tools are sourced from a `mcp_servers:` config block
#   3. multiple stdio MCP servers boot concurrently via the native pool
#
# Each test builds an isolated AICHAT_CONFIG_DIR inside $BATS_TEST_TMPDIR so it
# does not touch the user's production config.
#
# Fixture choice: `mcp-server-git` (uvx) is the smallest fast offline server.
# Tests that require additional servers prefer sqlite (uvx) and memory (node).

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
UVX="${UVX:-/Users/admin/.local/bin/uvx}"
AICHAT_SERVER_PORT="${AICHAT_SERVER_PORT:-8001}"

# Send a JSON-RPC sequence to `aichat --mcp` and capture stdout.
# $1 = AICHAT_CONFIG_DIR
# $2 = path to write stdout to
# Subsequent args = JSON-RPC message lines to send (one per arg).
mcp_exchange() {
  local cfg_dir="$1" out="$2"
  shift 2
  local messages=("$@")
  # Extract the last request ID to poll for.
  local last_id=$(printf "%s\n" "${messages[@]}" | jq -r 'select(.id != null) | .id' | tail -n 1)

  {
    for msg in "${messages[@]}"; do
      printf '%s\n' "$msg"
      sleep 0.3
    done

    if [ -n "$last_id" ]; then
      # Poll the output file for the last_id to ensure aichat has processed it
      # before we close stdin. This avoids the "connection closed" error.
      local timeout=30
      local count=0
      while [ $count -lt 150 ]; do
        if [ -f "$out" ] && grep -q "\"id\":$last_id" "$out"; then
          break
        fi
        sleep 0.2
        count=$((count + 1))
      done
    fi
    sleep 0.2
  } | AICHAT_CONFIG_DIR="$cfg_dir" timeout 40 "$AICHAT_BIN" --mcp >"$out" 2>"$out.err"
}

# Build a minimal AICHAT_CONFIG_DIR with an empty model client and the supplied
# trailing YAML appended (e.g., a `mcp_servers:` block).
write_config() {
  local cfg_dir="$1"
  mkdir -p "$cfg_dir"
  cat >"$cfg_dir/config.yaml" <<'YAML'
model: ollama:gemma4:26b
function_calling: true
clients:
- type: openai-compatible
  name: ollama
  api_base: http://localhost:11434/v1
  models:
    - name: gemma4:26b
      max_input_tokens: 160000
      max_output_tokens: 8942
      supports_function_calling: true
YAML
  if [ -n "$2" ]; then
    printf '\n%s\n' "$2" >>"$cfg_dir/config.yaml"
  fi
}

INIT_MSG='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bats","version":"0.0.1"}}}'
INITIALIZED_MSG='{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'
LIST_MSG='{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'

@test "mcp-server: initialize returns serverInfo and protocol version" {
  cfg="$BATS_TEST_TMPDIR/aichat"
  write_config "$cfg" ""
  # Always send the full handshake; aichat --mcp exits non-zero if stdin closes
  # mid-handshake (after only `initialize`).
  mcp_exchange "$cfg" "$BATS_TEST_TMPDIR/out" "$INIT_MSG" "$INITIALIZED_MSG" "$LIST_MSG"
  run jq -r 'select(.id==1) | .result.serverInfo.name' "$BATS_TEST_TMPDIR/out"
  [ "$status" -eq 0 ]
  [[ "$output" == *"aichat"* ]]
  run jq -r 'select(.id==1) | .result.protocolVersion' "$BATS_TEST_TMPDIR/out"
  [ "$output" = "2024-11-05" ]
}

@test "mcp-server: empty config advertises empty tools list" {
  cfg="$BATS_TEST_TMPDIR/aichat"
  write_config "$cfg" ""
  mcp_exchange "$cfg" "$BATS_TEST_TMPDIR/out" "$INIT_MSG" "$INITIALIZED_MSG" "$LIST_MSG"
  run jq -r 'select(.id==2) | .result.tools | length' "$BATS_TEST_TMPDIR/out"
  [ "$status" -eq 0 ]
  [ "$output" = "0" ]
}

@test "mcp-server: single mcp_servers entry advertises discover_roles meta-tool (lazy mode)" {
  cfg="$BATS_TEST_TMPDIR/aichat"
  write_config "$cfg" "mcp_servers:
  git:
    command: $UVX
    args: [\"mcp-server-git\"]"
  mcp_exchange "$cfg" "$BATS_TEST_TMPDIR/out" "$INIT_MSG" "$INITIALIZED_MSG" "$LIST_MSG"
  # Lazy mode kicks in once total tools >= 8 (mcp-server-git ships 12).
  run jq -r 'select(.id==2) | [.result.tools[].name] | join(",")' "$BATS_TEST_TMPDIR/out"
  [ "$status" -eq 0 ]
  [ "$output" = "discover_roles" ]
  # listChanged capability should be advertised under lazy mode.
  run jq -r 'select(.id==1) | .result.capabilities.tools.listChanged' "$BATS_TEST_TMPDIR/out"
  [ "$output" = "true" ]
}

@test "mcp-server: discover_roles enumerates mcp_servers tools with namespaced names" {
  cfg="$BATS_TEST_TMPDIR/aichat"
  write_config "$cfg" "mcp_servers:
  git:
    command: $UVX
    args: [\"mcp-server-git\"]"
  call='{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"discover_roles","arguments":{"query":"git"}}}'
  mcp_exchange "$cfg" "$BATS_TEST_TMPDIR/out" "$INIT_MSG" "$INITIALIZED_MSG" "$LIST_MSG" "$call"
  run jq -r 'select(.id==3) | .result.content[0].text' "$BATS_TEST_TMPDIR/out"
  [ "$status" -eq 0 ]
  # Tools are namespaced as <server>:<tool>
  [[ "$output" == *"git:git_status"* ]]
  [[ "$output" == *"git:git_log"* ]]
}

@test "mcp-server: tool-call dispatch through mcp_servers pool (probe a)" {
  # Confirms aichat --mcp can actually INVOKE an mcp_servers tool, not just list
  # it. Captured during the bridge-retirement validation pass on 2026-05-01;
  # unskipped in Phase 31A once ToolCall::eval grew the same is_mcp /
  # mcp_pool.call() routing as eval_tool_calls (`is_mcp_call` helper in
  # src/function.rs).
  cfg="$BATS_TEST_TMPDIR/aichat"
  write_config "$cfg" "mcp_servers:
  git:
    command: $UVX
    args: [\"mcp-server-git\"]"
  expand='{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"discover_roles","arguments":{"query":"git_status"}}}'
  call="{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"git:git_status\",\"arguments\":{\"repo_path\":\"$BATS_TEST_TMPDIR\"}}}"
  ( cd "$BATS_TEST_TMPDIR" && git init -q . )
  mcp_exchange "$cfg" "$BATS_TEST_TMPDIR/out" "$INIT_MSG" "$INITIALIZED_MSG" "$LIST_MSG" "$expand" "$call"
  run jq -r 'select(.id==4) | .result.content[0].text' "$BATS_TEST_TMPDIR/out"
  [ "$status" -eq 0 ]
  [[ "$output" == *"branch"* ]]
}

@test "mcp-server: 3 concurrent stdio servers register all tools (probe b small-N)" {
  # Probe (b) found that booting 5+ concurrent stdio MCP servers regresses to
  # zero registered tools or a non-responsive runtime even with bumped
  # mcp_startup_timeout. At small N the pool initializes correctly. This test
  # pins the small-N happy path; the multi-server hang at large N is tracked
  # separately (see docs/demos/demo-mcp-server.md "Known limitations").
  cfg="$BATS_TEST_TMPDIR/aichat"
  write_config "$cfg" "mcp_servers:
  sqlite:
    command: $UVX
    args: [\"mcp-server-sqlite\", \"--db-path\", \"$BATS_TEST_TMPDIR/probe.db\"]
  git:
    command: $UVX
    args: [\"mcp-server-git\"]"
  call='{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"discover_roles","arguments":{}}}'
  mcp_exchange "$cfg" "$BATS_TEST_TMPDIR/out" "$INIT_MSG" "$INITIALIZED_MSG" "$LIST_MSG" "$call"
  run jq -r 'select(.id==3) | .result.content[0].text' "$BATS_TEST_TMPDIR/out"
  [ "$status" -eq 0 ]
  [[ "$output" == *"sqlite:"* ]]
  [[ "$output" == *"git:"* ]]
}

@test "mcp-server: many concurrent stdio servers register tools without fail-fast (probe b large-N)" {
  # Phase 31B: pool init is now per-server resilient. The original symptom
  # ("0 registered tools" with 5+ entries) was fail-fast aggregation in
  # `all_tool_declarations`: one slow server's timeout aborted the loop and
  # wiped every other server's tools. The fix connects servers concurrently
  # via `join_all` and isolates per-server failures. Five stdio servers, all
  # uvx-based (fast, offline) — all five should register.
  cfg="$BATS_TEST_TMPDIR/aichat"
  write_config "$cfg" "mcp_servers:
  sqlite:
    command: $UVX
    args: [\"mcp-server-sqlite\", \"--db-path\", \"$BATS_TEST_TMPDIR/probe.db\"]
  git:
    command: $UVX
    args: [\"mcp-server-git\"]
  fetch:
    command: $UVX
    args: [\"mcp-server-fetch\"]
  ollama:
    command: $UVX
    args: [\"mcp-ollama\"]
  time:
    command: $UVX
    args: [\"mcp-server-time\"]"
  call='{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"discover_roles","arguments":{}}}'
  mcp_exchange "$cfg" "$BATS_TEST_TMPDIR/out" "$INIT_MSG" "$INITIALIZED_MSG" "$LIST_MSG" "$call"
  run jq -r 'select(.id==3) | .result.content[0].text' "$BATS_TEST_TMPDIR/out"
  [ "$status" -eq 0 ]
  [[ "$output" == *"sqlite:"* ]]
  [[ "$output" == *"git:"* ]]
  [[ "$output" == *"fetch:"* ]]
  [[ "$output" == *"ollama:"* ]]
  [[ "$output" == *"time:"* ]]
}

@test "mcp-server: portable mcp.json file is loaded via mcp_servers_file (Phase 31C)" {
  # Phase 31C: aichat reads a Claude-Code-compatible `mcp.json` and merges
  # those entries with the inline `mcp_servers:` block. This test points
  # `mcp_servers_file:` at a portable file containing a single git server
  # and asserts its tools are advertised through `discover_roles`.
  cfg="$BATS_TEST_TMPDIR/aichat"
  mkdir -p "$cfg"
  portable="$BATS_TEST_TMPDIR/portable-mcp.json"
  cat >"$portable" <<JSON
{
  "mcpServers": {
    "git": {
      "command": "$UVX",
      "args": ["mcp-server-git"]
    }
  }
}
JSON
  write_config "$cfg" "mcp_servers_file: $portable"
  call='{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"discover_roles","arguments":{"query":"git"}}}'
  mcp_exchange "$cfg" "$BATS_TEST_TMPDIR/out" "$INIT_MSG" "$INITIALIZED_MSG" "$LIST_MSG" "$call"
  run jq -r 'select(.id==3) | .result.content[0].text' "$BATS_TEST_TMPDIR/out"
  [ "$status" -eq 0 ]
  [[ "$output" == *"git:git_status"* ]]
}

@test "mcp-server: inline mcp_servers entry overrides portable file (Phase 31C)" {
  # Spec: inline `mcp_servers:` wins on key conflict. The portable file
  # lists a `git` entry that points at a non-existent binary; the inline
  # block re-defines `git` to use the real uvx binary. After load, the
  # working uvx-backed git tools must register.
  cfg="$BATS_TEST_TMPDIR/aichat"
  mkdir -p "$cfg"
  portable="$BATS_TEST_TMPDIR/portable-mcp.json"
  cat >"$portable" <<JSON
{
  "mcpServers": {
    "git": {
      "command": "/tmp/this-binary-does-not-exist",
      "args": []
    }
  }
}
JSON
  write_config "$cfg" "mcp_servers_file: $portable
mcp_startup_timeout: 5
mcp_servers:
  git:
    command: $UVX
    args: [\"mcp-server-git\"]"
  call='{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"discover_roles","arguments":{"query":"git"}}}'
  mcp_exchange "$cfg" "$BATS_TEST_TMPDIR/out" "$INIT_MSG" "$INITIALIZED_MSG" "$LIST_MSG" "$call"
  run jq -r 'select(.id==3) | .result.content[0].text' "$BATS_TEST_TMPDIR/out"
  [ "$status" -eq 0 ]
  [[ "$output" == *"git:git_status"* ]]
}

@test "mcp-server: failing server does not poison the pool (probe b isolation)" {
  # Phase 31B: a single hung/misconfigured server must not abort registration
  # for the rest. Pre-fix `all_tool_declarations` was fail-fast: the first
  # connect error returned Err and no tools registered. We pair a working
  # sqlite with a bogus command and expect sqlite's tools to still register.
  cfg="$BATS_TEST_TMPDIR/aichat"
  write_config "$cfg" "mcp_startup_timeout: 5
mcp_servers:
  bogus:
    command: /tmp/this-binary-does-not-exist
    args: []
  sqlite:
    command: $UVX
    args: [\"mcp-server-sqlite\", \"--db-path\", \"$BATS_TEST_TMPDIR/probe.db\"]"
  call='{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"discover_roles","arguments":{}}}'
  mcp_exchange "$cfg" "$BATS_TEST_TMPDIR/out" "$INIT_MSG" "$INITIALIZED_MSG" "$LIST_MSG" "$call"
  run jq -r 'select(.id==3) | .result.content[0].text' "$BATS_TEST_TMPDIR/out"
  [ "$status" -eq 0 ]
  [[ "$output" == *"sqlite:"* ]]
}
