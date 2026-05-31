#!/usr/bin/env bats

PROJECT_DIR="${PROJECT_DIR:-"${HOME}/Developer/Projects/aichat"}"
AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"
AICHAT_API_BASE="${AICHAT_API_BASE:-http://localhost:8001/v1}"


test-local-server() {
  cd "${PROJECT_DIR}" || exit 1;
  result=$(argc models-openai-compatible --api-base "${AICHAT_API_BASE}" --api-key="" |jq '.data[0].owned_by')
  [ -n "$result" ]
}