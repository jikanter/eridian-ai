#!/usr/bin/env bats
#
# Phase 2: end-to-end bridge tests. With a stub `pi` on PATH that records
# its env then blocks on a sentinel file, we drive curl against the
# `/v1/state/*` endpoints aichat's in-process server exposes — exactly the
# routes the bundled aichat-bridge.js extension calls into from pi.
#
# Each test:
#   1. spawns aichat with --pi-repl in the background;
#   2. waits for the stub pi to capture AICHAT_BRIDGE_URL / *_TOKEN;
#   3. drives curl against the bridge;
#   4. trips the sentinel so the stub pi exits cleanly, then waits.

# Resolve AICHAT_BIN to an absolute path before tests `cd` away from the
# checkout. Without this, `./target/debug/aichat` would not be findable.
AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
case "$AICHAT_BIN" in
  /*) ;;
  *) AICHAT_BIN="$(pwd)/$AICHAT_BIN" ;;
esac

# Repo root, derived from the binary path (target/debug/aichat → ../../..),
# so source-bundle assertions work after the tests `cd` to a scratch dir.
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
  # A tiny role on disk so /v1/state/role can resolve a name and we can
  # verify the mutation through /v1/state/info. The body string is unique
  # and visible in `role.export()`, which is what /v1/state/info?of=role
  # returns.
  cat >"$cfg/roles/test-role.md" <<'YAML'
---
model: openai:gpt-4o-mini
---

DETERMINISTIC_TEST_ROLE_MARKER
YAML
  export AICHAT_CONFIG_DIR="$cfg"
  # Critical: each test runs in its own scratch dir so the launcher's
  # `.pi/extensions/` staging cannot pollute the worktree checkout.
  cd "$BATS_TEST_TMPDIR"
}

# Build a stub pi that records its env then blocks until $BATS_TEST_TMPDIR/done
# exists, then exits 0. The stub writes env atomically (mv after write) so
# the test never reads a half-written file.
make_blocking_stub() {
  local stubdir="$BATS_TEST_TMPDIR/stubs"
  mkdir -p "$stubdir"
  cat >"$stubdir/pi" <<STUB
#!/usr/bin/env bash
tmp="$BATS_TEST_TMPDIR/observed.tmp"
{
  printf 'AICHAT_BRIDGE_URL=%s\n' "\$AICHAT_BRIDGE_URL"
  printf 'AICHAT_BRIDGE_TOKEN=%s\n' "\$AICHAT_BRIDGE_TOKEN"
} >"\$tmp"
mv "\$tmp" "$BATS_TEST_TMPDIR/observed"
# Hold until the test releases us.
while [ ! -f "$BATS_TEST_TMPDIR/done" ]; do
  sleep 0.05
done
exit 0
STUB
  chmod +x "$stubdir/pi"
  PI_STUB_PATH="$stubdir"
}

# Spawn aichat in background; wait up to 5s for the stub pi to record env.
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

# Release the stub and wait for aichat to exit. Asserts a clean shutdown
# so a regression in server teardown surfaces as a test failure.
stop_aichat_bg() {
  touch "$BATS_TEST_TMPDIR/done"
  wait "$AICHAT_PID"
}

@test "bridge: missing Authorization header → 401" {
  make_blocking_stub
  start_aichat_bg

  run curl -sS -o /dev/null -w '%{http_code}' "$BRIDGE_URL/v1/state/info"
  [ "$status" -eq 0 ]
  [ "$output" = "401" ]

  stop_aichat_bg
}

@test "bridge: wrong token → 401" {
  make_blocking_stub
  start_aichat_bg

  run curl -sS -o /dev/null -w '%{http_code}' \
    -H 'Authorization: Bearer not-the-right-token' \
    "$BRIDGE_URL/v1/state/info"
  [ "$output" = "401" ]

  stop_aichat_bg
}

@test "bridge: GET /v1/state/info returns JSON with valid token" {
  make_blocking_stub
  start_aichat_bg

  body=$(curl -sS -H "Authorization: Bearer $BRIDGE_TOKEN" "$BRIDGE_URL/v1/state/info")
  # The default Config::info path returns sysinfo when no role/agent is
  # active; the JSON wrapper must include an `info` string field.
  echo "$body" | jq -e '.info | type == "string"' >/dev/null

  stop_aichat_bg
}

@test "bridge: POST /v1/state/role flips state seen by subsequent /info" {
  make_blocking_stub
  start_aichat_bg

  http_code=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer $BRIDGE_TOKEN" \
    -H 'Content-Type: application/json' \
    --data '{"name":"test-role"}' \
    "$BRIDGE_URL/v1/state/role")
  [ "$http_code" = "200" ]

  # After the role switch, /v1/state/info?of=role exports the role we set.
  # We match a unique marker in the role body, not the name (export format
  # is not guaranteed to repeat the slug).
  body=$(curl -sS -H "Authorization: Bearer $BRIDGE_TOKEN" \
    "$BRIDGE_URL/v1/state/info?of=role")
  echo "$body" | jq -e '.info | contains("DETERMINISTIC_TEST_ROLE_MARKER")' >/dev/null

  stop_aichat_bg
}

@test "bridge: unknown route under /v1/state/ → 404 (auth still enforced)" {
  make_blocking_stub
  start_aichat_bg

  # Without token: bridge auth check fires first, even on unknown routes.
  # That's intentional — we don't want unauthenticated probes to learn the
  # route surface.
  code_noauth=$(curl -sS -o /dev/null -w '%{http_code}' \
    "$BRIDGE_URL/v1/state/does-not-exist")
  [ "$code_noauth" = "401" ]

  # With token: 404 for an unknown bridge route.
  code_authed=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer $BRIDGE_TOKEN" \
    "$BRIDGE_URL/v1/state/does-not-exist")
  [ "$code_authed" = "404" ]

  stop_aichat_bg
}

@test "bridge: wrong method on a known route → 405" {
  make_blocking_stub
  start_aichat_bg

  # /v1/state/role is POST-only.
  code=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer $BRIDGE_TOKEN" \
    "$BRIDGE_URL/v1/state/role")
  [ "$code" = "405" ]

  stop_aichat_bg
}

@test "bridge: bundled extension registers the /aichat-edit command" {
  # The committed, embedded bundle must expose the edit surface so the REPL
  # can drive /v1/state/edit. Grep the source-of-truth bundle directly — this
  # is independent of where the launcher stages it.
  bundle="$AICHAT_REPO_ROOT/assets/pi-extensions/aichat-bridge.js"
  [ -f "$bundle" ]
  grep -q 'registerCommand("aichat-edit"' "$bundle"
  # And it must call the edit endpoint for both read (GET) and write (POST).
  grep -q '/v1/state/edit' "$bundle"
}

@test "bridge: edit read returns the live config file content" {
  make_blocking_stub
  start_aichat_bg

  body=$(curl -sS -H "Authorization: Bearer $BRIDGE_TOKEN" \
    "$BRIDGE_URL/v1/state/edit?target=config")
  echo "$body" | jq -e '.target == "config"' >/dev/null
  # The content field carries the actual config.yaml text the test wrote.
  echo "$body" | jq -e '.content | contains("openai:gpt-4o-mini")' >/dev/null

  stop_aichat_bg
}

@test "bridge: edit read of session is deferred to pi-native → 400" {
  make_blocking_stub
  start_aichat_bg

  # Sessions are owned by pi's native format; .edit session must not bridge.
  code=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer $BRIDGE_TOKEN" \
    "$BRIDGE_URL/v1/state/edit?target=session")
  [ "$code" = "400" ]

  stop_aichat_bg
}

@test "bridge: edit read with no target → 400" {
  make_blocking_stub
  start_aichat_bg

  code=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer $BRIDGE_TOKEN" \
    "$BRIDGE_URL/v1/state/edit")
  [ "$code" = "400" ]

  stop_aichat_bg
}

@test "bridge: edit write round-trips config content" {
  make_blocking_stub
  start_aichat_bg

  http=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer $BRIDGE_TOKEN" \
    -H 'Content-Type: application/json' \
    --data '{"target":"config","content":"model: openai:gpt-4o-mini\nclients:\n  - type: openai\n    api_key: dummy\n# ROUNDTRIP_MARKER\n"}' \
    "$BRIDGE_URL/v1/state/edit")
  [ "$http" = "200" ]

  body=$(curl -sS -H "Authorization: Bearer $BRIDGE_TOKEN" \
    "$BRIDGE_URL/v1/state/edit?target=config")
  echo "$body" | jq -e '.content | contains("ROUNDTRIP_MARKER")' >/dev/null

  stop_aichat_bg
}

@test "bridge: edit role round-trips and reload is visible via /info" {
  make_blocking_stub
  start_aichat_bg

  # Activate the role so the bridge can resolve which role file to edit.
  curl -sS -o /dev/null -H "Authorization: Bearer $BRIDGE_TOKEN" \
    -H 'Content-Type: application/json' --data '{"name":"test-role"}' \
    "$BRIDGE_URL/v1/state/role"

  # Read the role file back through the edit surface.
  body=$(curl -sS -H "Authorization: Bearer $BRIDGE_TOKEN" \
    "$BRIDGE_URL/v1/state/edit?target=role")
  echo "$body" | jq -e '.content | contains("DETERMINISTIC_TEST_ROLE_MARKER")' >/dev/null

  # Write a new body; the handler must persist AND reload the live role.
  http=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer $BRIDGE_TOKEN" \
    -H 'Content-Type: application/json' \
    --data '{"target":"role","content":"---\nmodel: openai:gpt-4o-mini\n---\n\nEDITED_ROLE_MARKER\n"}' \
    "$BRIDGE_URL/v1/state/edit")
  [ "$http" = "200" ]

  info=$(curl -sS -H "Authorization: Bearer $BRIDGE_TOKEN" \
    "$BRIDGE_URL/v1/state/info?of=role")
  echo "$info" | jq -e '.info | contains("EDITED_ROLE_MARKER")' >/dev/null

  stop_aichat_bg
}

@test "bridge: edit role with no active role → 409" {
  make_blocking_stub
  start_aichat_bg

  # No role activated: nothing to resolve, so the edit surface refuses.
  code=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer $BRIDGE_TOKEN" \
    "$BRIDGE_URL/v1/state/edit?target=role")
  [ "$code" = "409" ]

  stop_aichat_bg
}

@test "bridge: edit route rejects unsupported method → 405" {
  make_blocking_stub
  start_aichat_bg

  code=$(curl -sS -o /dev/null -w '%{http_code}' -X DELETE \
    -H "Authorization: Bearer $BRIDGE_TOKEN" \
    "$BRIDGE_URL/v1/state/edit")
  [ "$code" = "405" ]

  stop_aichat_bg
}

@test "bridge: bundled extension lands in .pi/extensions/ during launch" {
  make_blocking_stub
  start_aichat_bg

  # Sanity: the launcher staged the bundle into the CWD's .pi/extensions.
  [ -f ".pi/extensions/aichat-bridge.js" ]
  # Bundle should contain the registerCommand call and the bridge env name.
  grep -q 'registerCommand' .pi/extensions/aichat-bridge.js
  grep -q 'AICHAT_BRIDGE_URL' .pi/extensions/aichat-bridge.js

  stop_aichat_bg

  # And cleanup removed it (the launcher cleans up when AICHAT_KEEP_PI_STAGE is unset).
  [ ! -f ".pi/extensions/aichat-bridge.js" ]
}
