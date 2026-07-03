#!/usr/bin/env bats
#
# Phase 54E — config introspection (--config-path / --config-get KEY).
# Batch-friendly, read-only access to the resolved config: the file path and
# individual resolved values, reusing the same key/value set --info shows.
# Delivered as flags (additive), not a `config` subcommand — that is 54F's
# (Ask-First) job.
#
# Self-contained and CI-safe: runs against an isolated AICHAT_CONFIG_DIR with a
# minimal config (no model/provider/network). --config-path resolves statically
# before init; --config-get uses the light (info) init path, so neither needs a
# live model or instance.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
AICHAT="$AICHAT_BIN"

setup() {
  CFG_DIR="$(mktemp -d)"
  printf 'compress_threshold: 1234\nstream: true\n' > "${CFG_DIR}/config.yaml"
}

teardown() {
  rm -rf "${CFG_DIR}"
}

@test "config-path prints the resolved config file in the configured dir" {
  run env AICHAT_CONFIG_DIR="${CFG_DIR}" "${AICHAT}" --config-path
  [ "$status" -eq 0 ]
  [ "$output" = "${CFG_DIR}/config.yaml" ]
}

@test "config-get returns the configured value" {
  run env AICHAT_CONFIG_DIR="${CFG_DIR}" "${AICHAT}" --config-get compress_threshold
  [ "$status" -eq 0 ]
  [ "$output" = "1234" ]
}

@test "config-get agrees with the value --info reports" {
  val=$(env AICHAT_CONFIG_DIR="${CFG_DIR}" "${AICHAT}" --config-get compress_threshold)
  from_info=$(env AICHAT_CONFIG_DIR="${CFG_DIR}" "${AICHAT}" --info | awk '$1=="compress_threshold"{print $2}')
  [ "$val" = "$from_info" ]
}

@test "config-get with -o json emits an object keyed by the requested key" {
  run env AICHAT_CONFIG_DIR="${CFG_DIR}" "${AICHAT}" --config-get stream -o json
  [ "$status" -eq 0 ]
  echo "$output" | grep -q '"stream"'
}

@test "config-get on an unknown key fails with a suggestion" {
  run env AICHAT_CONFIG_DIR="${CFG_DIR}" "${AICHAT}" --config-get compress_threshhold
  [ "$status" -ne 0 ]
  [[ "$output" == *"Did you mean \`compress_threshold\`?"* ]]
}

@test "config introspection flags are documented under Discovery" {
  run "${AICHAT}" --help
  [ "$status" -eq 0 ]
  section=$(echo "$output" | awk '/^[A-Z][A-Za-z ]*:$/{sec=$0} sec=="Discovery:"{print}')
  echo "$section" | grep -qE -- "--config-path"
  echo "$section" | grep -qE -- "--config-get"
}
