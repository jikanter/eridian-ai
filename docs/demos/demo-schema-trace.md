# Schema Validation Trace Events

*2026-04-05T04:23:57Z by Showboat 0.6.1*
<!-- showboat-id: 5debdb65-2dc4-4ecc-8793-6a60fb245c6a -->

Extends `--trace` to emit `[schema]` events when input or output schema validation runs. Previously, `--trace` only showed LLM call mechanics (turns, tokens, latency). On schema failure, the raw model output was silently discarded — making it impossible to debug *why* validation failed. Now `--trace` shows the raw output and per-violation paths.

## Unit Tests

```bash
cargo test -- config::role::tests::test_validate_schema_detailed 2>&1 | grep "^test " | sort
```

```output
test config::role::tests::test_validate_schema_detailed_multiple_violations ... ok
test config::role::tests::test_validate_schema_detailed_nested_array_paths ... ok
test config::role::tests::test_validate_schema_detailed_not_json ... ok
test config::role::tests::test_validate_schema_detailed_preserves_raw_text ... ok
test config::role::tests::test_validate_schema_detailed_success ... ok
test config::role::tests::test_validate_schema_detailed_terse_error_format ... ok
test config::role::tests::test_validate_schema_detailed_type_mismatch_has_paths ... ok
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 196 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 6 filtered out; finished in 0.00s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 188 filtered out; finished in 0.02s
```

```bash
cargo test -- utils::trace::tests 2>&1 | grep "^test " | sort
```

```output
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 196 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 6 filtered out; finished in 0.00s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 189 filtered out; finished in 0.00s
test utils::trace::tests::test_emit_schema_validation_fail_no_panic ... ok
test utils::trace::tests::test_emit_schema_validation_jsonl_to_file ... ok
test utils::trace::tests::test_emit_schema_validation_pass_no_panic ... ok
test utils::trace::tests::test_truncate_long ... ok
test utils::trace::tests::test_truncate_newlines ... ok
test utils::trace::tests::test_truncate_short ... ok
```

## Integration Tests

```bash
bats tests/integration/validation.sh --filter "schema"
```

```output
1..5
ok 1 input schema validation error exits 8
ok 2 input schema validation with --trace shows [schema] event
ok 3 input schema validation with --trace shows raw output
ok 4 input schema trace JSONL via AICHAT_TRACE=1
ok 5 valid input schema with --trace shows OK
```

## Live Demos

Demonstrate human-readable trace on schema failure — create a temporary role with an integer schema, then feed it a string:

```bash
ROLES_DIR="$HOME/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-trace-demo.md" <<'ROLE'
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
echo "{\"count\": \"not_a_number\"}" | aichat --trace -r test-trace-demo 2>&1; echo "exit: $?"
rm "$ROLES_DIR/test-trace-demo.md"
```

```output
[schema] FAIL input  1 violation
    raw: {"count": "not_a_number"}
    - /count: "not_a_number" is not of type "integer"
Error: Schema input validation failed:
  - "not_a_number" is not of type "integer"
exit: 8
```

The trace shows the raw input, the JSON path (`/count`), and the violation message — all before the standard error. Without `--trace`, only the terse error appears.

JSONL trace via `AICHAT_TRACE=1` emits machine-readable events:

```bash
ROLES_DIR="$HOME/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-trace-demo.md" <<'ROLE'
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
echo "{\"count\": \"bad\"}" | AICHAT_TRACE=1 aichat -r test-trace-demo 2>&1 | grep schema_validation | jq .
rm "$ROLES_DIR/test-trace-demo.md"
```

```output
{
  "type": "schema_validation",
  "direction": "input",
  "pass": false,
  "raw_output": "{\"count\": \"bad\"}",
  "violations": [
    {
      "message": "\"bad\" is not of type \"integer\"",
      "instance_path": "/count",
      "schema_path": "/properties/count/type"
    }
  ]
}
```

Nested array violations show the full JSON path:

```bash
ROLES_DIR="$HOME/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-trace-array.md" <<'ROLE'
---
input_schema:
  type: object
  properties:
    items:
      type: array
      items:
        type: object
        properties:
          name:
            type: string
          qty:
            type: integer
        required: [name, qty]
  required: [items]
---
Process: __INPUT__
ROLE
echo "{\"items\": [{\"name\": \"apple\", \"qty\": \"bad\"}]}" | aichat --trace -r test-trace-array 2>&1; echo "exit: $?"
rm "$ROLES_DIR/test-trace-array.md"
```

```output
[schema] FAIL input  1 violation
    raw: {"items": [{"name": "apple", "qty": "bad"}]}
    - /items/0/qty: "bad" is not of type "integer"
Error: Schema input validation failed:
  - "bad" is not of type "integer"
exit: 8
```

The path `/items/0/qty` tells you exactly which element in which array failed — no guesswork.
