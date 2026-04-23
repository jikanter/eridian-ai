#!/usr/bin/env bats
#
# Integration tests for model-specific request-body extensions.
#
# These exercise the end-to-end path: config.yaml → prepare_completion_data
# → openai_build_chat_completions_body → outbound HTTP request body.
#
# Approach: stand up a tiny localhost HTTP server in Python that writes each
# request body to a file and returns a minimal OpenAI-compatible response.
# Point aichat at it via an isolated AICHAT_CONFIG_DIR, then assert against
# the captured body.
#
# The `AICHAT_BIN` variable lets the suite run against either the dev build
# (./target/debug/aichat) or whatever aichat is on PATH. Override with:
#   AICHAT_BIN=target/release/aichat bats tests/integration/extensions.sh
#   AICHAT_BIN=$(command -v aichat) bats tests/integration/extensions.sh
#
# Note: the name deliberately avoids $AICHAT because aichat itself treats
# $AICHAT as a config-dir override (see docs), so users may already have it
# exported.

# Default: prefer the local debug build if present, otherwise fall back to
# whichever aichat is on PATH.
AICHAT_BIN="${AICHAT_BIN:-}"
if [ -z "$AICHAT_BIN" ]; then
  if [ -x "./target/debug/aichat" ]; then
    AICHAT_BIN="$(cd "$(dirname ./target/debug/aichat)" && pwd)/aichat"
  else
    AICHAT_BIN="$(command -v aichat || true)"
  fi
fi

CAPTURE_SERVER="$BATS_TEST_DIRNAME/../../assets/extensions-capture-server.py"

setup() {
  AICHATC_CONFIG_DIR="~/.config/aichat"
  [ -n "$AICHAT_BIN" ] || {
    echo "aichat binary not found; set AICHAT_BIN=..." >&2
    return 1
  }
  [ -x "$AICHAT_BIN" ] || {
    echo "aichat binary $AICHAT_BIN is not executable" >&2
    return 1
  }

  export AICHAT_CONFIG_DIR="$(mktemp -d)"
  export CAPTURE_FILE="$AICHAT_CONFIG_DIR/captured.json"

  # Reserve a free port. Python picks a free one and prints it.
  PORT="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
  export PORT

  # Start the capture server in the background.
  python3 "$CAPTURE_SERVER" "$PORT" "$CAPTURE_FILE" >/dev/null 2>&1 &
  SERVER_PID=$!
  export SERVER_PID

  # Wait for the server to bind (up to ~2s).
  for _ in $(seq 1 20); do
    if curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
      break
    fi
    sleep 0.1
  done
}

teardown() {
  if [ -n "${SERVER_PID:-}" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$AICHAT_CONFIG_DIR"
}

# Write an aichat config.yaml that points at the capture server. Extra YAML
# fragments (under the single model entry) can be passed as the first arg.
write_config() {
  local model_extras="${1:-}"
  local client_extras="${2:-}"
  cat > "$AICHAT_CONFIG_DIR/config.yaml" <<EOF
model: mockvllm:test-model
save: false
keybindings: emacs
stream: false

clients:
  - type: openai-compatible
    name: mockvllm
    api_base: http://127.0.0.1:$PORT/v1
    api_key: dummy
$client_extras
    models:
      - name: test-model
        max_input_tokens: 8192
$model_extras
EOF
}

@test "extensions: client-level extensions land in the request body" {
  write_config "" "    extensions:
      num_ctx: 4096
      repeat_penalty: 1.1"

  run "$AICHAT_BIN" "hello"
  [ "$status" -eq 0 ]
  [ -s "$CAPTURE_FILE" ]

  run python3 -c "import json; d=json.load(open('$CAPTURE_FILE')); assert d['num_ctx']==4096, d; assert d['repeat_penalty']==1.1, d"
  [ "$status" -eq 0 ]
}

@test "extensions: model-level extensions land in the request body" {
  write_config "        extensions:
          top_k: 50
          mirostat: 2" ""

  run "$AICHAT_BIN" "hello"
  [ "$status" -eq 0 ]
  [ -s "$CAPTURE_FILE" ]

  run python3 -c "import json; d=json.load(open('$CAPTURE_FILE')); assert d['top_k']==50, d; assert d['mirostat']==2, d"
  [ "$status" -eq 0 ]
}

@test "extensions: model-level overrides client-level on overlap, both merge into body" {
  write_config "        extensions:
          num_ctx: 32768
          top_k: 50" "    extensions:
      num_ctx: 4096
      repeat_penalty: 1.1"

  run "$AICHAT_BIN" "hello"
  [ "$status" -eq 0 ]
  [ -s "$CAPTURE_FILE" ]

  # Model wins on num_ctx; non-overlapping keys from both levels still present.
  run python3 -c "import json; d=json.load(open('$CAPTURE_FILE')); assert d['num_ctx']==32768, d; assert d['top_k']==50, d; assert d['repeat_penalty']==1.1, d"
  [ "$status" -eq 0 ]
}

@test "extensions: no extensions configured — body has no custom fields" {
  write_config "" ""

  run "$AICHAT_BIN" "hello"
  [ "$status" -eq 0 ]
  [ -s "$CAPTURE_FILE" ]

  # Sanity: standard OpenAI fields present, none of our extension keys leaked in.
  run python3 -c "
import json
d = json.load(open('$CAPTURE_FILE'))
assert 'messages' in d
assert 'model' in d
for k in ('num_ctx','repeat_penalty','top_k','mirostat','guided_json'):
    assert k not in d, f'unexpected extension key {k!r} in body'
"
  [ "$status" -eq 0 ]
}

@test "extensions: nested object extension (vLLM guided_json) is preserved" {
  write_config "        extensions:
          guided_json:
            type: object
            properties:
              answer:
                type: string" ""

  run "$AICHAT_BIN" "hello"
  [ "$status" -eq 0 ]
  [ -s "$CAPTURE_FILE" ]

  run python3 -c "
import json
d = json.load(open('$CAPTURE_FILE'))
gj = d['guided_json']
assert gj['type'] == 'object', gj
assert gj['properties']['answer']['type'] == 'string', gj
"
  [ "$status" -eq 0 ]
}
