# Schema-Aware Stdin/Stdout for Validated Pipelines

*2026-02-28T19:45:08Z by Showboat 0.6.1*
<!-- showboat-id: d83e2824-daa9-4565-9d8d-21a016c54b28 -->

This feature adds `input_schema` and `output_schema` fields to aichat roles, enabling JSON Schema validation on both the input piped into a role and the output produced by the LLM. This turns aichat into a building block for validated, composable pipelines where structured data flows through roles with guaranteed contracts.

## How It Works

A role definition can now include `input_schema` and/or `output_schema` in its YAML frontmatter. When present:

1. **Input validation** — stdin is validated against `input_schema` before the prompt is sent to the LLM
2. **System prompt injection** — the `output_schema` is automatically appended to the system prompt, instructing the LLM to respond with conformant JSON
3. **Output validation** — the LLM response is validated against `output_schema` before being written to stdout

If validation fails at either end, aichat exits with an error — no silent corruption.

## Role Definition

Here's an example role with an output schema that enforces structured entity extraction:

```bash
cat <<'ROLE'
---
model: openai:gpt-4o
output_schema:
  type: object
  properties:
    entities:
      type: array
      items:
        type: object
        properties:
          name:
            type: string
          type:
            type: string
        required: [name, type]
  required: [entities]
---
Extract all named entities from the input text.
ROLE
```

```output
---
model: openai:gpt-4o
output_schema:
  type: object
  properties:
    entities:
      type: array
      items:
        type: object
        properties:
          name:
            type: string
          type:
            type: string
        required: [name, type]
  required: [entities]
---
Extract all named entities from the input text.
```

## Schema Validation in Code

The validation logic uses the `jsonschema` crate. Let's look at the unit tests that prove it works:

```bash
cargo test test_validate_schema -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 3 tests
test config::role::tests::test_validate_schema_not_json ... ok
test config::role::tests::test_validate_schema_success ... ok
test config::role::tests::test_validate_schema_failure ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 49 filtered out; finished in 0.02s

```

All three validation paths pass: valid JSON matching the schema, JSON missing a required field, and non-JSON input.

## Role Parsing Tests

The role parser correctly picks up schema fields from YAML frontmatter:

```bash
cargo test test_role_with_schemas -- --nocapture 2>&1 && cargo test test_role_without_schemas -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test config::role::tests::test_role_with_schemas ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.11s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test config::role::tests::test_role_without_schemas ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s

```

Roles with schemas parse correctly and expose the schema via `input_schema()` / `output_schema()` accessors. Roles without schemas return `None` — no behavioral change for existing roles.

## Pipeline Example

With this feature, you can build validated Unix pipelines:

    echo '{"text": "Alice met Bob in Paris"}' | aichat -r entity-extractor | aichat -r summarizer

Each stage validates its input and output against its declared schemas. If the LLM hallucinates malformed JSON or the wrong structure, the pipeline fails fast with a clear error instead of silently propagating garbage.

## Implementation Summary

| File | Change |
|------|--------|
| `src/config/role.rs` | Added `input_schema` / `output_schema` fields, schema-aware system prompt injection, `validate_schema()` function, and export serialization via `serde_yaml` |
| `src/main.rs` | Wired up input validation before LLM call and output validation after, with direct stdout printing for schema-validated output |
| `Cargo.toml` | Added `jsonschema` dependency |
