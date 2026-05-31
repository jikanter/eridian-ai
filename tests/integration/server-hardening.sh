#!/usr/bin/env bats
#
# Phase 16 server hardening integration tests.
#
# Exercises the production-safety surface added in Phase 16:
#   - 16A: configurable CORS origins (serve_cors_origins / serve_cors_allow_all)
#   - 16B: optional bearer-token auth (serve_api_key)
#   - 16C: GET /health (unauthenticated liveness probe)
#   - 16E: POST /v1/reload (hot-reload roles from disk)
#
# Like federation.bats, this spins up `aichat --serve` on a free port with a
# fake (never-listening) provider and issues curl requests. No real LLM calls
# are made; only the HTTP hardening plumbing is exercised.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
TEST_PORT="${AICHAT_TEST_HARDENING_PORT:-18152}"
MOCK_PORT="${AICHAT_TEST_HARDENING_MOCK_PORT:-18153}"

make_config_dir() {
  local d
  d=$(mktemp -d)
  mkdir -p "$d/roles"
  echo "$d"
}

write_role() {
  local dir="$1" name="$2" content="$3"
  printf '%s\n' "$content" > "$dir/roles/$name.md"
}

# Write a config.yaml with the given extra lines appended to the base. The
# base points the only client at a never-listening localhost port so the
# server starts cleanly without making real LLM calls.
write_config() {
  local dir="$1" extra="$2"
  cat > "$dir/config.yaml" <<EOF
model: fake:fake-model
function_calling: false
serve_addr: 127.0.0.1:$TEST_PORT
$extra
clients:
  - type: openai-compatible
    name: fake
    api_base: http://127.0.0.1:1/v1
    models:
      - name: fake-model
        max_input_tokens: 4096
EOF
}

# Poll /health for readiness. /health is unauthenticated, so this works
# whether or not serve_api_key is set (per feedback_test_no_or_true memory:
# poll for the real ready signal rather than masking with sleeps).
wait_for_server() {
  local port="$1" i
  for i in $(seq 1 50); do
    if curl -sf "http://127.0.0.1:$port/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.1
  done
  echo "server did not become reachable on port $port"
  return 1
}

start_server() {
  local cfg_dir="$1" port="$2" log="$3"
  AICHAT_CONFIG_DIR="$cfg_dir" "$AICHAT_BIN" --serve "127.0.0.1:$port" >"$log" 2>&1 &
  SERVER_PID=$!
  wait_for_server "$port" || {
    echo "server log:"
    cat "$log"
    kill "$SERVER_PID" 2>/dev/null || true
    return 1
  }
}

stop_server() {
  if [ -n "${SERVER_PID:-}" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
    SERVER_PID=""
  fi
}

# Launch a tiny OpenAI-compatible streaming backend (Phase 16D). It replies to
# any POST with two content chunks, then a usage-only chunk (choices:[]), then
# [DONE] — exactly what an upstream that honors stream_options emits.
start_mock_provider() {
  local port="$1"
  MOCK_SCRIPT=$(mktemp)
  cat > "$MOCK_SCRIPT" <<'PY'
import sys, http.server
PORT = int(sys.argv[1])
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass
    def do_GET(self):
        self.send_response(200); self.end_headers()
    def do_POST(self):
        n = int(self.headers.get('Content-Length', 0))
        self.rfile.read(n)
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()
        chunks = [
            '{"choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"}}]}',
            '{"choices":[{"index":0,"delta":{"content":" world"}}]}',
            '{"choices":[],"usage":{"prompt_tokens":11,"completion_tokens":3,"total_tokens":14}}',
        ]
        for c in chunks:
            self.wfile.write(('data: ' + c + '\n\n').encode()); self.wfile.flush()
        self.wfile.write(b'data: [DONE]\n\n'); self.wfile.flush()
http.server.HTTPServer(('127.0.0.1', PORT), H).serve_forever()
PY
  python3 "$MOCK_SCRIPT" "$port" >/dev/null 2>&1 &
  MOCK_PID=$!
  local i
  for i in $(seq 1 50); do
    if curl -s -o /dev/null "http://127.0.0.1:$port/" 2>/dev/null; then
      return 0
    fi
    sleep 0.1
  done
  echo "mock provider did not start on port $port"
  return 1
}

stop_mock_provider() {
  if [ -n "${MOCK_PID:-}" ]; then
    kill "$MOCK_PID" 2>/dev/null || true
    wait "$MOCK_PID" 2>/dev/null || true
    MOCK_PID=""
  fi
  [ -n "${MOCK_SCRIPT:-}" ] && rm -f "$MOCK_SCRIPT"
}

setup() {
  CFG_DIR=$(make_config_dir)
  LOG_FILE=$(mktemp)
}

teardown() {
  stop_server
  stop_mock_provider
  rm -rf "$CFG_DIR"
  rm -f "$LOG_FILE"
}

# ---- 16C: health endpoint ----

@test "phase 16C: GET /health returns ok with model and role counts" {
  write_config "$CFG_DIR" ""
  write_role "$CFG_DIR" "h-role" "---
description: health role
---
BODY"
  start_server "$CFG_DIR" "$TEST_PORT" "$LOG_FILE"

  run curl -sf "http://127.0.0.1:$TEST_PORT/health"
  [ "$status" -eq 0 ]
  [[ "$output" == *'"status":"ok"'* ]]
  # The fake provider model plus the synthetic "default" entry are counted;
  # the role:* virtual models are excluded from the model count.
  [[ "$output" == *'"models":2'* ]]
  # Built-in roles (%shell% etc.) plus the user role are counted; assert the
  # field is present and numeric rather than pinning the built-in count.
  [[ "$output" =~ \"roles\":[0-9]+ ]]
}

# ---- 16B: bearer-token auth ----

@test "phase 16B: no auth required when serve_api_key is unset" {
  write_config "$CFG_DIR" ""
  start_server "$CFG_DIR" "$TEST_PORT" "$LOG_FILE"

  run curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$TEST_PORT/v1/roles"
  [ "$status" -eq 0 ]
  [ "$output" = "200" ]
}

@test "phase 16B: request without a key is 401 when serve_api_key is set" {
  write_config "$CFG_DIR" 'serve_api_key: sk-test-secret'
  start_server "$CFG_DIR" "$TEST_PORT" "$LOG_FILE"

  run curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$TEST_PORT/v1/roles"
  [ "$status" -eq 0 ]
  [ "$output" = "401" ]
}

@test "phase 16B: correct bearer token is accepted" {
  write_config "$CFG_DIR" 'serve_api_key: sk-test-secret'
  start_server "$CFG_DIR" "$TEST_PORT" "$LOG_FILE"

  run curl -s -o /dev/null -w "%{http_code}" \
    -H "Authorization: Bearer sk-test-secret" \
    "http://127.0.0.1:$TEST_PORT/v1/roles"
  [ "$status" -eq 0 ]
  [ "$output" = "200" ]
}

@test "phase 16B: wrong bearer token is rejected" {
  write_config "$CFG_DIR" 'serve_api_key: sk-test-secret'
  start_server "$CFG_DIR" "$TEST_PORT" "$LOG_FILE"

  run curl -s -o /dev/null -w "%{http_code}" \
    -H "Authorization: Bearer sk-wrong" \
    "http://127.0.0.1:$TEST_PORT/v1/roles"
  [ "$status" -eq 0 ]
  [ "$output" = "401" ]
}

@test "phase 16B + 16C: /health stays open even when serve_api_key is set" {
  write_config "$CFG_DIR" 'serve_api_key: sk-test-secret'
  start_server "$CFG_DIR" "$TEST_PORT" "$LOG_FILE"

  run curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$TEST_PORT/health"
  [ "$status" -eq 0 ]
  [ "$output" = "200" ]
}

# ---- 16A: configurable CORS ----

@test "phase 16A: configured origin receives Access-Control-Allow-Origin" {
  write_config "$CFG_DIR" 'serve_cors_origins:
  - http://host.docker.internal:3000'
  start_server "$CFG_DIR" "$TEST_PORT" "$LOG_FILE"

  run curl -s -D - -o /dev/null -X OPTIONS \
    -H "Origin: http://host.docker.internal:3000" \
    "http://127.0.0.1:$TEST_PORT/v1/chat/completions"
  [ "$status" -eq 0 ]
  [[ "$output" == *"access-control-allow-origin: http://host.docker.internal:3000"* ]]
}

@test "phase 16A: unlisted remote origin gets no CORS header by default" {
  write_config "$CFG_DIR" ""
  start_server "$CFG_DIR" "$TEST_PORT" "$LOG_FILE"

  run curl -s -D - -o /dev/null -X OPTIONS \
    -H "Origin: https://evil.example.com" \
    "http://127.0.0.1:$TEST_PORT/v1/chat/completions"
  [ "$status" -eq 0 ]
  [[ "$output" != *"access-control-allow-origin"* ]]
}

@test "phase 16A: serve_cors_allow_all echoes any origin" {
  write_config "$CFG_DIR" 'serve_cors_allow_all: true'
  start_server "$CFG_DIR" "$TEST_PORT" "$LOG_FILE"

  run curl -s -D - -o /dev/null -X OPTIONS \
    -H "Origin: https://anything.example.com" \
    "http://127.0.0.1:$TEST_PORT/v1/chat/completions"
  [ "$status" -eq 0 ]
  [[ "$output" == *"access-control-allow-origin: https://anything.example.com"* ]]
}

# ---- 16E: hot reload ----

@test "phase 16E: POST /v1/reload picks up a newly added role" {
  write_config "$CFG_DIR" ""
  start_server "$CFG_DIR" "$TEST_PORT" "$LOG_FILE"

  # The role does not exist yet.
  run curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$TEST_PORT/v1/roles/late-role"
  [ "$status" -eq 0 ]
  [ "$output" = "404" ]

  # Role count before adding anything (built-ins only).
  before=$(curl -s "http://127.0.0.1:$TEST_PORT/health" | grep -o '"roles":[0-9]*' | grep -o '[0-9]*')

  # Add it on disk, then reload.
  write_role "$CFG_DIR" "late-role" "---
description: added after boot
---
BODY"

  run curl -sf -X POST "http://127.0.0.1:$TEST_PORT/v1/reload"
  [ "$status" -eq 0 ]
  [[ "$output" =~ \"roles\":[0-9]+ ]]

  # Now it resolves...
  run curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$TEST_PORT/v1/roles/late-role"
  [ "$status" -eq 0 ]
  [ "$output" = "200" ]

  # ...and the live count went up by exactly one.
  after=$(curl -s "http://127.0.0.1:$TEST_PORT/health" | grep -o '"roles":[0-9]*' | grep -o '[0-9]*')
  [ "$after" -eq "$((before + 1))" ]
}

# ---- 16D: streaming usage ----

@test "phase 16D: stream_options.include_usage emits a usage chunk with cost" {
  command -v python3 >/dev/null || skip "python3 not available for mock provider"
  start_mock_provider "$MOCK_PORT"

  # Point the only client at the mock SSE backend, with prices so cost_usd > 0.
  cat > "$CFG_DIR/config.yaml" <<EOF
model: fake:fake-model
function_calling: false
serve_addr: 127.0.0.1:$TEST_PORT
clients:
  - type: openai-compatible
    name: fake
    api_base: http://127.0.0.1:$MOCK_PORT/v1
    models:
      - name: fake-model
        max_input_tokens: 4096
        input_price: 1.0
        output_price: 2.0
EOF
  start_server "$CFG_DIR" "$TEST_PORT" "$LOG_FILE"

  run curl -sf -N -X POST "http://127.0.0.1:$TEST_PORT/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -d '{"model":"fake:fake-model","messages":[{"role":"user","content":"hi"}],"stream":true,"stream_options":{"include_usage":true}}'
  [ "$status" -eq 0 ]
  # Content streamed through.
  [[ "$output" == *"Hello"* ]]
  # The trailing usage chunk carries token counts and our computed cost.
  [[ "$output" == *'"usage"'* ]]
  [[ "$output" == *'"total_tokens":14'* ]]
  [[ "$output" == *'"cost_usd"'* ]]
  # cost = 11*1/1e6 + 3*2/1e6 = 0.000017
  [[ "$output" == *"0.000017"* ]]
  # And the stream still terminates with [DONE].
  [[ "$output" == *"[DONE]"* ]]
}

@test "phase 16D: without include_usage no usage chunk is emitted" {
  command -v python3 >/dev/null || skip "python3 not available for mock provider"
  start_mock_provider "$MOCK_PORT"

  cat > "$CFG_DIR/config.yaml" <<EOF
model: fake:fake-model
function_calling: false
serve_addr: 127.0.0.1:$TEST_PORT
clients:
  - type: openai-compatible
    name: fake
    api_base: http://127.0.0.1:$MOCK_PORT/v1
    models:
      - name: fake-model
        max_input_tokens: 4096
        input_price: 1.0
        output_price: 2.0
EOF
  start_server "$CFG_DIR" "$TEST_PORT" "$LOG_FILE"

  run curl -sf -N -X POST "http://127.0.0.1:$TEST_PORT/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -d '{"model":"fake:fake-model","messages":[{"role":"user","content":"hi"}],"stream":true}'
  [ "$status" -eq 0 ]
  [[ "$output" == *"Hello"* ]]
  [[ "$output" == *"[DONE]"* ]]
  # No usage block when the caller didn't opt in.
  [[ "$output" != *'"cost_usd"'* ]]
}
