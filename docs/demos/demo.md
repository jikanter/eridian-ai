# Schema-Aware Stdin/Stdout for Validated Pipelines

*2026-03-30T15:42:55Z by Showboat 0.6.1*
<!-- showboat-id: 331f4db4-4219-4e7f-81f2-36e1f1147eaa -->

Adds `input_schema` and `output_schema` fields to roles for JSON Schema validation on both input and output. This turns aichat into a building block for validated, composable pipelines.

**`input_schema`** — validates stdin/input before the LLM call  
**`output_schema`** — validates LLM output (with auto-retry on failure)  
Both accept standard JSON Schema objects in YAML frontmatter.

## Unit Tests

```bash
cargo test -- config::role::tests::test_validate_schema_success config::role::tests::test_validate_schema_failure config::role::tests::test_validate_schema_not_json config::role::tests::test_role_with_schemas config::role::tests::test_role_without_schemas config::role::tests::test_set_input_schema config::role::tests::test_set_output_schema 2>&1 | grep "^test config" | sort
```

```output
test config::role::tests::test_role_with_schemas ... ok
test config::role::tests::test_role_without_schemas ... ok
test config::role::tests::test_set_input_schema ... ok
test config::role::tests::test_set_output_schema ... ok
test config::role::tests::test_validate_schema_failure ... ok
test config::role::tests::test_validate_schema_not_json ... ok
test config::role::tests::test_validate_schema_success ... ok
```

```bash
cargo test --test compatibility -- schema_validation 2>&1 | grep "^test " | grep -v "^test result" | sort
```

```output
test schema_validation::test_validate_invalid_json ... ok
test schema_validation::test_validate_missing_required_field ... ok
test schema_validation::test_validate_schema_with_enum ... ok
test schema_validation::test_validate_type_mismatch ... ok
test schema_validation::test_validate_valid_json_against_schema ... ok
test typed_errors::test_schema_validation_error_display ... ok
```

## Integration Tests

Create a role with schemas and test validation via `--dry-run`:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-schema-demo.md" <<'ROLE'
---
input_schema:
  type: object
  properties:
    query:
      type: string
  required: [query]
output_schema:
  type: object
  properties:
    answer:
      type: string
    confidence:
      type: number
  required: [answer, confidence]
---
Answer the query and provide confidence. __INPUT__
ROLE
echo "{\"query\": \"What is 2+2?\"}" | aichat --dry-run -r test-schema-demo 2>/dev/null
rm "$ROLES_DIR/test-schema-demo.md"
```

```output
---
input_schema:
  type: object
  properties:
    query:
      type: string
  required:
  - query
output_schema:
  type: object
  properties:
    answer:
      type: string
    confidence:
      type: number
  required:
  - answer
  - confidence
---

Answer the query and provide confidence. {"query": "What is 2+2?"}
```

The schema-enabled role loads and validates the JSON input against `input_schema` before constructing the prompt. The output_schema instructions are injected into the system prompt automatically.

Test input validation failure with invalid input:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-schema-fail.md" <<'ROLE'
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
echo "not json at all" | aichat --dry-run -r test-schema-fail 2>&1; echo "exit: $?"
rm "$ROLES_DIR/test-schema-fail.md"
```

```output
Error: Schema input validation failed: not valid JSON

Caused by:
    expected ident at line 1 column 2
exit: 8
```

Invalid input is caught by schema validation before the LLM call, providing clear error feedback.
