#!/usr/bin/env bats

PROMPTS_DIR="${AICHAT_PROMPTS_DIR:-$HOME/Library/Application Support/aichat/prompts}"

setup() {
  # Create temporary role files for schema validation tests
  mkdir -p "$PROMPTS_DIR"
  echo "Hello world" > "${PROMPTS_DIR}/test-prompt.md"
}

teardown() {
  rm -f "${PROMPTS_DIR}/test-prompt.md"
}

AICHAT=aichat
#AICHAT=./target/debug/aichat

@test "list roles returns some roles with json" {
  result=$(${AICHAT} --list-roles -o json)
  [ "$result" != "" ]
}

@test "list roles -o json returns both builtin and custom roles" {
  result=$(${AICHAT} --list-roles -o json)
  [ "$(echo "$result" |grep '%code%')" != "" ]
}

@test "list prompts -o json returns prompt data" {

  result=$(${AICHAT} --list-prompts -o json)
  [ "$(echo "$result" |grep "test-prompt")" != "" ]
}
