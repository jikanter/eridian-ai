#!/usr/bin/env bats

# Note: These tests use a release build. You must run 'cargo install --path=.' before running these tests
# This is different than typical, isolated test environments. Eventually I will implement an isolated test environment.
ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"

setup() {
  # Create temporary role files for schema validation tests
  mkdir -p "$ROLES_DIR"
}

teardown() {
  rm -f "$ROLES_DIR/test-trace-schema-input.md"
  rm -f "$ROLES_DIR/test-trace-schema-output.md"
}

@test "input schema validation error exits 8" {
  cat > "$ROLES_DIR/test-trace-schema-input.md" <<'ROLE'
---
input_schema:
  type: object
  properties:
    query:
      type: string
  required: [query]
---
Answer: __INPUT__
ROLE
  run bash -c 'echo "not json" | aichat -r test-trace-schema-input 2>&1'
  [ "$status" -eq 8 ]
  [[ "$output" == *"Schema input validation failed"* ]]
}

@test "input schema validation with --trace shows [schema] event" {
  cat > "$ROLES_DIR/test-trace-schema-input.md" <<'ROLE'
---
input_schema:
  type: object
  properties:
    query:
      type: string
  required: [query]
---
Answer: __INPUT__
ROLE
  # --trace output goes to stderr; capture both
  run bash -c 'echo "not json" | aichat --trace -r test-trace-schema-input 2>&1'
  [ "$status" -eq 8 ]
  [[ "$output" == *"[schema]"* ]]
  [[ "$output" == *"FAIL input"* ]]
  [[ "$output" == *"not valid JSON"* ]]
}

@test "input schema validation with --trace shows raw output" {
  cat > "$ROLES_DIR/test-trace-schema-input.md" <<'ROLE'
---
input_schema:
  type: object
  properties:
    count:
      type: integer
  required: [count]
---
Process: __INPUT__
ROLE
  run bash -c 'echo "{\"count\": \"bad\"}" | aichat --trace -r test-trace-schema-input 2>&1'
  [ "$status" -eq 8 ]
  [[ "$output" == *"[schema]"* ]]
  [[ "$output" == *"raw:"* ]]
  [[ "$output" == *"bad"* ]]
}

@test "input schema trace JSONL via AICHAT_TRACE=1" {
  cat > "$ROLES_DIR/test-trace-schema-input.md" <<'ROLE'
---
input_schema:
  type: object
  properties:
    query:
      type: string
  required: [query]
---
Answer: __INPUT__
ROLE
  run bash -c 'echo "not json" | AICHAT_TRACE=1 aichat -r test-trace-schema-input 2>&1'
  [ "$status" -eq 8 ]
  # JSONL event should contain type: schema_validation
  [[ "$output" == *'"type":"schema_validation"'* ]]
  [[ "$output" == *'"direction":"input"'* ]]
  [[ "$output" == *'"pass":false'* ]]
}

@test "valid input schema with --trace shows OK" {
  cat > "$ROLES_DIR/test-trace-schema-input.md" <<'ROLE'
---
input_schema:
  type: object
  properties:
    query:
      type: string
  required: [query]
---
Answer: __INPUT__
ROLE
  # Use --dry-run so we don't need an LLM; input validation still runs
  run bash -c 'echo "{\"query\": \"hello\"}" | aichat --trace --dry-run -r test-trace-schema-input 2>&1'
  [ "$status" -eq 0 ]
  [[ "$output" == *"[schema] OK input"* ]]
}

@test "query aichat with simple validation" {
  result=$(echo '{"query": "What is 2+2?"}' | aichat -r test-schema-demo -m ollama:llama3.1:latest --no-stream)
  [ "$(echo "$result" | jq '.result')" -eq 4 ]
}

@test "query aichat with simple validation and trace" {
  result=$(echo '{"query": "What is 2+2?"}' | aichat -r test-schema-demo -m ollama:llama3.1:latest --no-stream --trace)
  [ "$(echo "$result" | jq '.result')" -eq 4 ]
}

# requires: production install
@test "query aichat with simple json validation and web search" {
  result=$(echo "machine learning" |aichat -r data-discoverer -m ollama:llama3.1:latest --no-stream --trace)
  [ "$(echo "$result" | jq '.datasets')" -ne "" ]
}
