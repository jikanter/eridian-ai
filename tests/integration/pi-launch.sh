#!/usr/bin/env bats
#
# REPL launcher tests across Phases 1–4. Pi is the default REPL after the
# Phase 4 cutover; --legacy-repl and AICHAT_REPL=legacy keep the built-in
# Reedline REPL available for side-by-side testing.
#
# These tests do not require the upstream `pi` binary. Each test injects a
# tiny stub on PATH that proves the env contract holds and exits cleanly.
# When the upstream `pi` is available, run this file in addition to
# tests/integration/pi-bridge.sh for end-to-end coverage.
#
# Each test builds an isolated AICHAT_CONFIG_DIR under $BATS_TEST_TMPDIR so it
# does not touch the user's production config.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"

setup() {
  cfg="$BATS_TEST_TMPDIR/aichat"
  mkdir -p "$cfg"
  # Minimal config: no models needed; we don't actually call the LLM in
  # Phase 1 — we only verify the launcher's contract with pi.
  cat >"$cfg/config.yaml" <<'YAML'
model: openai:gpt-4o-mini
clients:
  - type: openai
    api_key: dummy
YAML
  export AICHAT_CONFIG_DIR="$cfg"
}

# Build a stub `pi` on a private PATH directory. The stub writes the bridge
# env it observed into a file the test reads, then exits with the status
# passed as $1.
#
# Usage: stub_pi <exit_code> <observed_env_file>
stub_pi() {
  local code="$1"
  local observed="$2"
  local stubdir="$BATS_TEST_TMPDIR/stubs"
  mkdir -p "$stubdir"
  cat >"$stubdir/pi" <<STUB
#!/usr/bin/env bash
{
  printf 'AICHAT_BRIDGE_URL=%s\n' "\$AICHAT_BRIDGE_URL"
  printf 'AICHAT_BRIDGE_TOKEN=%s\n' "\$AICHAT_BRIDGE_TOKEN"
} >"$observed"
exit $code
STUB
  chmod +x "$stubdir/pi"
  PI_STUB_PATH="$stubdir"
}

@test "pi-launch: missing pi binary emits an actionable install hint" {
  # Strip user PATH so `which pi` definitely fails. Keep /usr/bin and
  # /bin so the test environment retains coreutils.
  run env -i \
    HOME="$HOME" \
    AICHAT_CONFIG_DIR="$cfg" \
    PATH="/usr/bin:/bin" \
    AICHAT_REPL=pi \
    "$AICHAT_BIN"
  [ "$status" -ne 0 ]
  [[ "$output" == *"\`pi\` not found"* ]]
  [[ "$output" == *"pi.dev/install.sh"* ]] || [[ "$output" == *"@earendil-works/pi-coding-agent"* ]]
}

@test "pi-launch: --legacy-repl suppresses AICHAT_REPL=pi" {
  # With AICHAT_REPL=pi set but --legacy-repl flag, the launcher should not
  # probe for pi; instead, the built-in REPL is attempted. Because there is
  # no TTY in the bats environment, aichat bails with `No TTY for REPL`.
  # That message is the proof we routed to the legacy path, not pi.
  run env -i \
    HOME="$HOME" \
    AICHAT_CONFIG_DIR="$cfg" \
    PATH="/usr/bin:/bin" \
    AICHAT_REPL=pi \
    "$AICHAT_BIN" --legacy-repl
  [ "$status" -ne 0 ]
  [[ "$output" == *"No TTY for REPL"* ]]
}

@test "pi-launch: stub pi receives AICHAT_BRIDGE_URL and AICHAT_BRIDGE_TOKEN" {
  observed="$BATS_TEST_TMPDIR/observed-env"
  stub_pi 0 "$observed"

  run env -i \
    HOME="$HOME" \
    AICHAT_CONFIG_DIR="$cfg" \
    PATH="$PI_STUB_PATH:/usr/bin:/bin" \
    "$AICHAT_BIN" --pi-repl
  [ "$status" -eq 0 ]
  [ -f "$observed" ]

  # Token is a 32-char hex string from uuid::Uuid::simple().
  bridge_token=$(grep '^AICHAT_BRIDGE_TOKEN=' "$observed" | cut -d= -f2-)
  [[ "$bridge_token" =~ ^[0-9a-f]{32}$ ]]

  # URL must be an http://127.0.0.1:<port> bound to an ephemeral port (1024+).
  bridge_url=$(grep '^AICHAT_BRIDGE_URL=' "$observed" | cut -d= -f2-)
  [[ "$bridge_url" =~ ^http://127\.0\.0\.1:[0-9]+$ ]]
  port="${bridge_url##*:}"
  [ "$port" -ge 1024 ]
}

@test "pi-launch: aichat propagates non-zero pi exit status" {
  observed="$BATS_TEST_TMPDIR/observed-env"
  stub_pi 42 "$observed"

  run env -i \
    HOME="$HOME" \
    AICHAT_CONFIG_DIR="$cfg" \
    PATH="$PI_STUB_PATH:/usr/bin:/bin" \
    "$AICHAT_BIN" --pi-repl
  [ "$status" -ne 0 ]
  [[ "$output" == *"pi exited with status 42"* ]]
}

# --- Phase 4 cutover tests ----------------------------------------------

@test "pi-launch: bare aichat routes to pi when pi is on PATH (default)" {
  observed="$BATS_TEST_TMPDIR/observed-env"
  stub_pi 0 "$observed"

  # No --pi-repl flag, no AICHAT_REPL env — this is the default surface
  # after the Phase 4 cutover.
  run env -i \
    HOME="$HOME" \
    AICHAT_CONFIG_DIR="$cfg" \
    PATH="$PI_STUB_PATH:/usr/bin:/bin" \
    "$AICHAT_BIN"
  [ "$status" -eq 0 ]
  [ -f "$observed" ]

  # Bridge env still flows through the soft-default path; no warning
  # should appear because pi was discoverable on PATH.
  [[ "$output" != *"not on PATH"* ]]
  bridge_url=$(grep '^AICHAT_BRIDGE_URL=' "$observed" | cut -d= -f2-)
  [[ "$bridge_url" =~ ^http://127\.0\.0\.1:[0-9]+$ ]]
}

@test "pi-launch: bare aichat falls back to legacy with a note when pi is absent" {
  # Soft default: with pi missing, we route to the built-in REPL after
  # printing a one-line note. Since bats has no TTY, the legacy REPL
  # bails with "No TTY for REPL" — that's the proof we landed in the
  # legacy branch (not pi, which would have errored on the PATH probe).
  run env -i \
    HOME="$HOME" \
    AICHAT_CONFIG_DIR="$cfg" \
    PATH="/usr/bin:/bin" \
    "$AICHAT_BIN"
  [ "$status" -ne 0 ]
  [[ "$output" == *"not on PATH"* ]]
  [[ "$output" == *"using the built-in REPL"* ]]
  [[ "$output" == *"No TTY for REPL"* ]]
}

@test "pi-launch: AICHAT_REPL=legacy forces the built-in REPL even with pi installed" {
  observed="$BATS_TEST_TMPDIR/observed-env"
  stub_pi 0 "$observed"

  run env -i \
    HOME="$HOME" \
    AICHAT_CONFIG_DIR="$cfg" \
    PATH="$PI_STUB_PATH:/usr/bin:/bin" \
    AICHAT_REPL=legacy \
    "$AICHAT_BIN"
  [ "$status" -ne 0 ]
  # We never invoked the stub.
  [ ! -f "$observed" ]
  [[ "$output" == *"No TTY for REPL"* ]]
}
