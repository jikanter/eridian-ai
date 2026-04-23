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
  # Actually, it's better if this function calls 'run ./target/debug/aichat ...'
  run ./target/debug/aichat "${args[@]}"
}
