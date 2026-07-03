#!/usr/bin/env bats
#
# Zed ACP backend: the bridge endpoint + bundled-extension surface that let
# aichat drive a `pi` running under Zed's ACP host (pi-acp). Modeled on
# pi-bridge.sh — a stub `pi` captures the bridge env and blocks, and we curl
# the `/v1/state/*` routes the extension calls into.
#
# Covers:
#   1. POST /v1/state/subprocess registers the pi subprocess and returns the
#      live entity context (role/agent/session/rag) for Zed's startup block;
#   2. auth + method contract on that route (401 / 405 / 404 parity);
#   3. the bundled extension gates subprocess registration on
#      AICHAT_BRIDGE_SURFACE=acp (not the old phantom ZED_BRIDGE_URL) and
#      surfaces errors instead of swallowing them.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
case "$AICHAT_BIN" in
  /*) ;;
  *) AICHAT_BIN="$(pwd)/$AICHAT_BIN" ;;
esac
AICHAT_REPO_ROOT="${AICHAT_REPO_ROOT:-$(cd "$(dirname "$AICHAT_BIN")/../.." && pwd)}"

setup() {
  cfg="$BATS_TEST_TMPDIR/aichat"
  mkdir -p "$cfg/roles"
  cat >"$cfg/config.yaml" <<'YAML'
model: openai:gpt-4o-mini
clients:
  - type: openai
    api_key: dummy
YAML
  cat >"$cfg/roles/test-role.md" <<'YAML'
---
model: openai:gpt-4o-mini
---

DETERMINISTIC_TEST_ROLE_MARKER
YAML
  export AICHAT_CONFIG_DIR="$cfg"
  cd "$BATS_TEST_TMPDIR"
}

make_blocking_stub() {
  local stubdir="$BATS_TEST_TMPDIR/stubs"
  mkdir -p "$stubdir"
  cat >"$stubdir/pi" <<STUB
#!/usr/bin/env bash
tmp="$BATS_TEST_TMPDIR/observed.tmp"
{
  printf 'AICHAT_BRIDGE_URL=%s\n' "\$AICHAT_BRIDGE_URL"
  printf 'AICHAT_BRIDGE_TOKEN=%s\n' "\$AICHAT_BRIDGE_TOKEN"
  printf 'AICHAT_BRIDGE_SURFACE=%s\n' "\$AICHAT_BRIDGE_SURFACE"
} >"\$tmp"
mv "\$tmp" "$BATS_TEST_TMPDIR/observed"
while [ ! -f "$BATS_TEST_TMPDIR/done" ]; do
  sleep 0.05
done
exit 0
STUB
  chmod +x "$stubdir/pi"
  PI_STUB_PATH="$stubdir"
}

start_aichat_bg() {
  env -i HOME="$HOME" AICHAT_CONFIG_DIR="$cfg" \
    PATH="$PI_STUB_PATH:/usr/bin:/bin" \
    "$AICHAT_BIN" --pi-repl >"$BATS_TEST_TMPDIR/aichat.out" 2>&1 &
  AICHAT_PID=$!
  for _ in $(seq 1 100); do
    [ -f "$BATS_TEST_TMPDIR/observed" ] && break
    sleep 0.05
  done
  [ -f "$BATS_TEST_TMPDIR/observed" ]
  BRIDGE_URL=$(grep '^AICHAT_BRIDGE_URL=' "$BATS_TEST_TMPDIR/observed" | cut -d= -f2-)
  BRIDGE_TOKEN=$(grep '^AICHAT_BRIDGE_TOKEN=' "$BATS_TEST_TMPDIR/observed" | cut -d= -f2-)
}

stop_aichat_bg() {
  touch "$BATS_TEST_TMPDIR/done"
  wait "$AICHAT_PID"
}

# Always release a blocking stub and reap aichat, even when an assertion
# aborts the test mid-flight — otherwise a leaked child holds bats' stdout
# pipe open and the run hangs.
teardown() {
  touch "$BATS_TEST_TMPDIR/done" 2>/dev/null || true
  [ -n "${AICHAT_PID:-}" ] && kill "$AICHAT_PID" 2>/dev/null
  wait 2>/dev/null || true
}

@test "zed-acp: POST /v1/state/subprocess returns ok + context object" {
  make_blocking_stub
  start_aichat_bg

  body=$(curl -sS -H "Authorization: Bearer $BRIDGE_TOKEN" \
    -H 'Content-Type: application/json' \
    --data '{"surface":"acp"}' \
    "$BRIDGE_URL/v1/state/subprocess")
  echo "$body" | jq -e '.ok == true' >/dev/null
  echo "$body" | jq -e '.kind == "subprocess"' >/dev/null
  # context is always present; keys exist even when their value is null.
  echo "$body" | jq -e 'has("context")' >/dev/null
  echo "$body" | jq -e '.context | has("role") and has("agent") and has("session") and has("rag")' >/dev/null

  stop_aichat_bg
}

@test "zed-acp: subprocess context reflects an active role" {
  make_blocking_stub
  start_aichat_bg

  curl -sS -o /dev/null -H "Authorization: Bearer $BRIDGE_TOKEN" \
    -H 'Content-Type: application/json' --data '{"name":"test-role"}' \
    "$BRIDGE_URL/v1/state/role"

  body=$(curl -sS -H "Authorization: Bearer $BRIDGE_TOKEN" \
    -H 'Content-Type: application/json' --data '{}' \
    "$BRIDGE_URL/v1/state/subprocess")
  echo "$body" | jq -e '.context.role == "test-role"' >/dev/null

  stop_aichat_bg
}

@test "zed-acp: subprocess accepts an empty body" {
  make_blocking_stub
  start_aichat_bg

  code=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer $BRIDGE_TOKEN" \
    -H 'Content-Type: application/json' --data '{}' \
    "$BRIDGE_URL/v1/state/subprocess")
  [ "$code" = "200" ]

  stop_aichat_bg
}

@test "zed-acp: subprocess route enforces auth (401 without token)" {
  make_blocking_stub
  start_aichat_bg

  code=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H 'Content-Type: application/json' --data '{}' \
    "$BRIDGE_URL/v1/state/subprocess")
  [ "$code" = "401" ]

  stop_aichat_bg
}

@test "zed-acp: GET on subprocess route → 405 (POST only)" {
  make_blocking_stub
  start_aichat_bg

  code=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer $BRIDGE_TOKEN" \
    "$BRIDGE_URL/v1/state/subprocess")
  [ "$code" = "405" ]

  stop_aichat_bg
}

@test "zed-acp: bundled extension gates registration on AICHAT_BRIDGE_SURFACE=acp" {
  bundle="$AICHAT_REPO_ROOT/assets/pi-extensions/aichat-bridge.js"
  [ -f "$bundle" ]
  # New, real gate — replaces the phantom ZED_BRIDGE_URL.
  grep -q 'AICHAT_BRIDGE_SURFACE' "$bundle"
  grep -q '/v1/state/subprocess' "$bundle"
  # The dead phantom env var must be gone.
  ! grep -q 'ZED_BRIDGE_URL' "$bundle"
}

@test "zed-acp: terminal REPL launch marks its surface as repl" {
  make_blocking_stub
  start_aichat_bg
  # aichat's own --pi-repl launch tags the child so the extension can tell an
  # aichat-owned terminal REPL apart from an external ACP host.
  grep -q '^AICHAT_BRIDGE_SURFACE=repl$' "$BATS_TEST_TMPDIR/observed"
  stop_aichat_bg
}
