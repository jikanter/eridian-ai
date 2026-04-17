#!/usr/bin/env bats

# Regression tests for Configuration features described in Configuration-Guide.md

@test "config: --info shows config file path" {
  run ./target/debug/aichat --info
  [ "$status" -eq 0 ]
  [[ "$output" == *"config_file"* ]]
}

@test "config: override model via environment variable" {
  # We use a non-existent model to see if it's picked up by preflight
  AICHAT_MODEL="test-env-model" run ./target/debug/aichat --dry-run "test" 2>&1
  # Preflight should fail if the model is not in models.yaml, 
  # but it proves the environment variable was used.
  [[ "$output" == *"test-env-model"* ]]
}

@test "config: load config from custom directory" {
  CUSTOM_DIR=$(mktemp -d)
  cat > "$CUSTOM_DIR/config.yaml" <<EOF
model: custom-dir-model
EOF
  AICHAT_CONFIG_DIR="$CUSTOM_DIR" run ./target/debug/aichat --dry-run "test" 2>&1
  [[ "$output" == *"custom-dir-model"* ]]
  rm -rf "$CUSTOM_DIR"
}
