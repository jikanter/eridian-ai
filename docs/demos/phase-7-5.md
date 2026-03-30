# Phase 7.5: Macro & Agent Config Override (.set Expansion)

*2026-03-30T15:47:45Z by Showboat 0.6.1*
<!-- showboat-id: 83818cfc-b697-4508-a414-5cf62aa9bbb9 -->

Phase 7.5 extends the `.set` REPL command to cover role-level fields that previously could only be set in role frontmatter: `model`, `output_schema`, `input_schema`, `pipe_to`, and `save_to`. This lets macros configure schemas and lifecycle hooks at runtime.

## Role Setters

```bash
grep -n "pub fn set_output_schema\|pub fn set_input_schema\|pub fn set_pipe_to\|pub fn set_save_to" src/config/role.rs
```

```output
677:    pub fn set_output_schema(&mut self, value: Option<Value>) {
681:    pub fn set_input_schema(&mut self, value: Option<Value>) {
685:    pub fn set_pipe_to(&mut self, value: Option<String>) {
689:    pub fn set_save_to(&mut self, value: Option<String>) {
```

## Setter Unit Tests

```bash
cargo test test_set_ 2>&1 | grep -E "test config|test result" | sed "s/finished in [0-9.]*s/finished in Xs/" | sort
```

```output
test config::role::tests::test_set_input_schema ... ok
test config::role::tests::test_set_output_schema ... ok
test config::role::tests::test_set_pipe_to ... ok
test config::role::tests::test_set_save_to ... ok
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in Xs
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 140 filtered out; finished in Xs
```

## Guard Rails

`parse_schema_value` handles `null` (unset), `@path` (read file), and inline JSON. `validate_pipe_to_command` checks binary existence via `which`.

```bash
grep -n "fn parse_schema_value\|fn validate_json_schema\|fn validate_pipe_to_command" src/config/mod.rs
```

```output
3154:fn parse_schema_value(value: &str) -> Result<Option<serde_json::Value>> {
3171:fn validate_json_schema(schema: &serde_json::Value) -> Result<()> {
3177:fn validate_pipe_to_command(cmd: &str) -> Result<()> {
```

## .set Match Arms

```bash
grep -n "\"model\" =>\|\"output_schema\" =>\|\"input_schema\" =>\|\"pipe_to\" =>\|\"save_to\" =>" src/config/mod.rs | head -5
```

```output
789:            "model" => {
792:            "output_schema" => {
799:            "input_schema" => {
806:            "pipe_to" => {
814:            "save_to" => {
```

## Session Transient Fields

Schema/hook overrides are `#[serde(skip)]` — they live only for the current session.

```bash
grep -c "serde(skip)" src/config/session.rs
```

```output
13
```

## Tests

```bash
cargo test 2>&1 | grep "test result:" | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
test result: ok. 144 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
test result: ok. 173 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
```

## Integration Tests

Verify that roles with schemas load correctly via `--dry-run`:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-schema-set.md" <<'ROLE'
---
output_schema:
  type: object
  properties:
    result:
      type: string
  required: [result]
pipe_to: "cat > /dev/null"
save_to: "/tmp/test-output.md"
---
Answer concisely. __INPUT__
ROLE
echo "What is 2+2?" | aichat --dry-run -r test-schema-set 2>/dev/null
rm "$ROLES_DIR/test-schema-set.md"
```

```output
---
output_schema:
  type: object
  properties:
    result:
      type: string
  required:
  - result
pipe_to: cat > /dev/null
save_to: /tmp/test-output.md
---

Answer concisely. What is 2+2?
```

The role loads with output_schema, pipe_to, and save_to configured — all fields that can also be set at runtime via `.set` in the REPL.
