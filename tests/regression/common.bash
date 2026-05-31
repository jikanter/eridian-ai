# Shared env contract for regression tests.
#
#   AICHAT_BIN         — path to the aichat binary under test.
#                        Default: ./target/debug/aichat
#                        Override to point at an installed build, e.g.
#                          AICHAT_BIN=$(command -v aichat) bats tests/regression/...
#
#   AICHAT_CONFIG_DIR  — aichat config directory used by spawned subprocesses.
#                        Unset by default (aichat uses its native default,
#                        which on macOS is ~/Library/Application Support/aichat).
#                        Set to a temp dir for isolation, or to your real
#                        config dir to test against a production setup.
#
#   AICHAT_TEST_MODEL  — if set, run_aichat replaces --dry-run with
#                        --model "$AICHAT_TEST_MODEL" (real-call mode).
#
#   AICHAT_API_BASE    — if set, run aichat client processes against this api base.
#
# All three are exported so child processes inherit them.
export AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
[[ -n "${AICHAT_CONFIG_DIR:-}" ]] && export AICHAT_CONFIG_DIR
[[ -n "${AICHAT_TEST_MODEL:-}" ]] && export AICHAT_TEST_MODEL

# Fail fast with a clear message if the chosen binary doesn't exist.
# (BATS swallows stderr from setup, so also emit to fd 3 when it's open.)
if [[ ! -x "$AICHAT_BIN" ]]; then
  msg="AICHAT_BIN '$AICHAT_BIN' does not exist or is not executable."
  echo "$msg" >&2
  if { true >&3; } 2>/dev/null; then
    echo "$msg" >&3
  fi
  exit 1
fi

# Helper to run aichat with optional model override instead of --dry-run
# Usage: run_aichat [args...]
run_aichat() {
  local args=()
  local has_dry_run=0
  for arg in "$@"; do
    if [[ "$arg" == "--dry-run" ]]; then
      if [[ -n "$AICHAT_TEST_MODEL" ]]; then
        # Skip adding --dry-run and instead add --model if it's not already there
        # But we only want to add --model if we're substituting.
        # Actually, let's just replace --dry-run with --model $AICHAT_TEST_MODEL
        args+=("--model" "$AICHAT_TEST_MODEL")
      else
        args+=("--dry-run")
      fi
      has_dry_run=1
    else
      args+=("$arg")
    fi
  done
  
  # Execute the command and capture output (BATS "run" will be used by the caller if they use 'run run_aichat')
  # Actually, it's better if this function calls 'run "$AICHAT_BIN" ...'
  run "$AICHAT_BIN" "${args[@]}"
}
