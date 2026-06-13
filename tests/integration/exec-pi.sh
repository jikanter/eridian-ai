#!/usr/bin/env bats
#
# `--exec-pi` passthrough tests.
#
# When `--exec-pi` is the first argument, aichat loads its customizations and
# then execs `pi -p <remaining args...>`, propagating pi's exit status. These
# tests inject a tiny `pi` stub on PATH that records the argv it received and
# exits with a controllable status — no upstream `pi` binary required.
#
# Each test builds an isolated AICHAT_CONFIG_DIR under $BATS_TEST_TMPDIR so it
# never touches the user's production config.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"

setup() {
  cfg="$BATS_TEST_TMPDIR/aichat"
  mkdir -p "$cfg"
  # Minimal config so Config::init has a config.yaml and never drops into the
  # interactive first-run config creator. We never call the LLM here.
  cat >"$cfg/config.yaml" <<'YAML'
model: openai:gpt-4o-mini
clients:
  - type: openai
    api_key: dummy
YAML
  export AICHAT_CONFIG_DIR="$cfg"

  # Build a stub `pi` on a private PATH dir. It records each argv element on
  # its own line into $argv_file and exits with the code in $PI_STUB_EXIT.
  stubdir="$BATS_TEST_TMPDIR/stubs"
  mkdir -p "$stubdir"
  argv_file="$BATS_TEST_TMPDIR/pi_argv"
  cat >"$stubdir/pi" <<STUB
#!/usr/bin/env bash
: >"$argv_file"
for a in "\$@"; do printf '%s\n' "\$a" >>"$argv_file"; done
exit \${PI_STUB_EXIT:-0}
STUB
  chmod +x "$stubdir/pi"
}

@test "exec-pi: forwards remaining args to 'pi -p'" {
  PATH="$stubdir:$PATH" run "$AICHAT_BIN" --exec-pi "List all .ts files" in src/
  [ "$status" -eq 0 ]
  # First forwarded arg is the non-interactive flag, then our args verbatim.
  run cat "$argv_file"
  [ "${lines[0]}" = "-p" ]
  [ "${lines[1]}" = "List all .ts files" ]
  [ "${lines[2]}" = "in" ]
  [ "${lines[3]}" = "src/" ]
}

@test "exec-pi: propagates pi's exit status" {
  PI_STUB_EXIT=7 PATH="$stubdir:$PATH" run "$AICHAT_BIN" --exec-pi hello
  [ "$status" -eq 7 ]
}

@test "exec-pi: flag not in first position is a normal aichat run (no pi exec)" {
  # `--exec-pi` after a subcommand must NOT trigger passthrough. aichat parses
  # it as its own arg (and rejects it), so the pi stub is never invoked.
  PATH="$stubdir:$PATH" run "$AICHAT_BIN" --info --exec-pi
  [ ! -s "$argv_file" ]
}
