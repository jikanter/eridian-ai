#!/usr/bin/env bats

# Regression tests for Configuration features described in Configuration-Guide.md
load common.bash

@test "config: --info shows config file path" {
  run ./target/debug/aichat --info
  [ "$status" -eq 0 ]
  [[ "$output" == *"config_file"* ]]
}

@test "config: override model via environment variable" {
  # We use a non-existent model to see if it's picked up by preflight
  # If AICHAT_TEST_MODEL is set, we bypass this or handle it carefully.
  if [[ -n "$AICHAT_TEST_MODEL" ]]; then
    skip "Skipping environment override test when AICHAT_TEST_MODEL is set"
  fi
  AICHAT_MODEL="test-env-model" run_aichat --dry-run "test" 2>&1
  # Preflight should fail if the model is not in models.yaml,
  # but it proves the environment variable was used.
  [[ "$output" == *"test-env-model"* ]]
}

@test "config: load config from custom directory" {
  CUSTOM_DIR=$(mktemp -d)
  cat > "$CUSTOM_DIR/config.yaml" <<EOF
model: custom-dir-model
EOF
  if [[ -n "$AICHAT_TEST_MODEL" ]]; then
     AICHAT_CONFIG_DIR="$CUSTOM_DIR" run_aichat --dry-run "test" 2>&1
     # If we substituted model, it might not show custom-dir-model in output if it successfully ran
     # or it might show it in preflight. 
     # Actually run_aichat will add --model $AICHAT_TEST_MODEL which will OVERRIDE the config file.
     # So this test is not very useful when AICHAT_TEST_MODEL is set.
     skip "Skipping custom config dir test when AICHAT_TEST_MODEL is set"
  fi
  AICHAT_CONFIG_DIR="$CUSTOM_DIR" run_aichat --dry-run "test" 2>&1
  [[ "$output" == *"custom-dir-model"* ]]
  rm -rf "$CUSTOM_DIR"
}
