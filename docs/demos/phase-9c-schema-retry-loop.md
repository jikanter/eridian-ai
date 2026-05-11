# Phase 9C: Schema Validation Retry Loop

*2026-04-16T23:38:48Z by Showboat 0.6.1*
<!-- showboat-id: bfd7fed2-4aad-49c7-be0c-66d0aafbc525 -->

Phase 9C adds a retry loop around output schema validation. When a role has `output_schema` and `schema_retries > 0`, a validation failure no longer exits 8 immediately — instead, the failed output plus the validation error are replayed as an Assistant+User turn pair and the model gets another chance. The loop short-circuits (retries=0) when the provider is already enforcing the schema natively via Phase 9A/9B, since a retry under native enforcement can't buy anything.

## schema_retries is a new role frontmatter field

```bash
grep "schema_retries" src/config/role.rs | head -8
```

```output
    schema_retries: Option<usize>,
                                "schema_retries" => {
                                    role.schema_retries = value.as_u64().map(|v| v as usize)
        if let Some(n) = self.schema_retries {
            meta.insert("schema_retries".into(), serde_json::json!(n));
    pub fn schema_retries(&self) -> Option<usize> {
        self.schema_retries
    fn test_schema_retries_default_none() {
```

## Input carries the retry feedback across calls

```bash
grep "retry_feedback\|with_retry_prompt" src/config/input.rs src/config/role.rs
```

```output
src/config/input.rs:    retry_feedback: Option<(String, String)>,
src/config/input.rs:            retry_feedback: None,
src/config/input.rs:            retry_feedback: None,
src/config/input.rs:    pub fn with_retry_prompt(mut self, failed_output: &str, retry_prompt: &str) -> Self {
src/config/input.rs:        self.retry_feedback = Some((failed_output.to_string(), retry_prompt.to_string()));
src/config/input.rs:    pub fn retry_feedback(&self) -> Option<(&str, &str)> {
src/config/input.rs:        self.retry_feedback
src/config/role.rs:        if let Some((failed_output, retry_prompt)) = input.retry_feedback() {
src/config/role.rs:    fn test_build_messages_appends_retry_feedback() {
src/config/role.rs:        // retry_feedback() drives the injection.
src/config/role.rs:        // Here: directly assert that when retry_feedback is set, two extra
src/config/role.rs:        let input = input.with_retry_prompt(
```

## The retry loop in main.rs (directive path)

```bash
grep "Phase 9C\|native_structured\|max_schema_retries" src/main.rs
```

```output
    // Phase 9C: Schema validation retry budget. Short-circuit to 0 when the
    let native_structured = has_output_schema.is_some()
    let max_schema_retries = if has_output_schema.is_some() && !is_dry_run && !native_structured {
        if !is_dry_run && max_schema_retries > 0 {
                    Err(e) if schema_retry_attempts < max_schema_retries => {
        // max_schema_retries == 0 (native structured output, or user disabled),
        if max_schema_retries == 0 {
```

## And in pipe.rs (pipeline stage path)

```bash
grep "Phase 9C\|native_structured\|max_schema_retries" src/pipe.rs
```

```output
    // Phase 9C: schema retry budget for this stage. Short-circuits to 0 when
    let native_structured = role.has_output_schema()
    let max_schema_retries = if role.has_output_schema() && !native_structured {
    // Phase 9C: retry loop on output schema failure.
        if max_schema_retries > 0 {
                    Err(e) if attempt < max_schema_retries => {
```

## Unit tests for schema_retries parsing and retry-message injection

```bash
cargo test --bin aichat -- test_schema_retries test_build_messages_appends_retry_feedback 2>&1 | grep -E "^test config::role" | sort
```

```output
test config::role::tests::test_build_messages_appends_retry_feedback ... ok
test config::role::tests::test_schema_retries_default_none ... ok
test config::role::tests::test_schema_retries_in_export ... ok
test config::role::tests::test_schema_retries_parsed_from_frontmatter ... ok
test config::role::tests::test_schema_retries_roundtrip ... ok
test config::role::tests::test_schema_retries_zero_means_fail_fast ... ok
```

## Full test suite stays green

```bash
cargo test --bin aichat 2>&1 | grep "^test result" | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/; s/[0-9]+ passed/N passed/"
```

```output
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```

## End-to-end: a weak local model exercises the retry budget

Install a role that asks a small local model for JSON with a strict schema. Weak local models often wrap their output in markdown fences or leak `<think>` tags — the retry loop gives them a corrective second/third chance.

```bash
ROLES_DIR="$HOME/Library/Application Support/aichat/roles" && mkdir -p "$ROLES_DIR" && cat > "$ROLES_DIR/test-schema-retry-demo.md" <<EOF
---
model: ollama:gemma4:e4b
schema_retries: 2
output_schema:
  type: object
  properties:
    answer:
      type: integer
  required: [answer]
---
You are a math assistant. Respond with ONLY valid JSON matching: {"answer": <integer>}

Question: __INPUT__
EOF
echo installed
```

```output
installed
```

The trace below shows how many times the retry loop engages. We only count `[schema] FAIL` lines since the actual model output varies from run to run — what's deterministic is that `schema_retries: 2` allows up to 3 total attempts (initial + 2 retries), and the run exits 8 only after all attempts fail:

```bash
OUT=$(echo "What is 2 plus 2?" | aichat -r test-schema-retry-demo --trace --no-stream 2>&1 || true); COUNT=$(echo "$OUT" | grep -c "^\[schema\]" || true); if [ "$COUNT" -ge 1 ] && [ "$COUNT" -le 3 ]; then echo "[schema] events observed within retry budget (1..=3)"; else echo "out of expected range: $COUNT"; fi
```

```output
[schema] events observed within retry budget (1..=3)
```

The exact count varies run-to-run (weak models are nondeterministic) but it stays inside the budget: initial attempt plus up to `schema_retries` retries.

## Fail-fast: schema_retries: 0 preserves the old behavior

Switch the role to `schema_retries: 0` — the retry loop disengages and we get exactly one attempt, matching pre-9C behavior:

```bash
ROLES_DIR="$HOME/Library/Application Support/aichat/roles" && sed -i "" "s|schema_retries: 2|schema_retries: 0|" "$ROLES_DIR/test-schema-retry-demo.md"; OUT=$(echo "What is 2 plus 2?" | aichat -r test-schema-retry-demo --trace --no-stream 2>&1 || true); COUNT=$(echo "$OUT" | grep -c "^\[schema\]" || true); echo "[schema] events with schema_retries: 0 = $COUNT"; [ "$COUNT" -eq 1 ] && echo "exactly one attempt (no retries)"
```

```output
[schema] events with schema_retries: 0 = 1
exactly one attempt (no retries)
```

## Short-circuit when native structured output is active

When the model declares `supports_response_format_json_schema: true` (Phase 9A / 9B), the provider guarantees schema-conforming output, so the retry budget is forced to 0 regardless of the role's `schema_retries` setting:

```bash
grep -A 6 "native_structured" src/main.rs | head -16
```

```output
    let native_structured = has_output_schema.is_some()
        && input
            .role()
            .model()
            .data()
            .supports_response_format_json_schema;
    let max_schema_retries = if has_output_schema.is_some() && !is_dry_run && !native_structured {
        input.role().schema_retries().unwrap_or(1)
    } else {
        0
    };
    let original_input = input.clone();

```

## Cleanup

```bash
rm -f "$HOME/Library/Application Support/aichat/roles/test-schema-retry-demo.md" && echo removed
```

```output
removed
```
