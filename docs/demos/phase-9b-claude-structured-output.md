# Phase 9B: Claude Tool-Use-as-Schema

*2026-04-16T23:21:01Z by Showboat 0.6.1*
<!-- showboat-id: 20adfd3d-98cb-40ed-85cb-43f07cabacdd -->

Phase 9B enables provider-native structured output for Claude via the tool-use pattern. When a role declares `output_schema` and the model has `supports_response_format_json_schema: true`, AIChat injects a synthetic `structured_output` tool whose `input_schema` IS the schema, forces it via `tool_choice`, and extracts the tool call's `input` as the JSON output. The redundant prompt-injected schema suffix is stripped so we don't double-pay tokens.

## Capability flag added to ModelData

```bash
grep -n "supports_response_format_json_schema" src/client/model.rs
```

```output
321:    pub supports_response_format_json_schema: bool,
```

## Claude body builder injects the synthetic tool

```bash
grep -n "CLAUDE_STRUCTURED_OUTPUT_TOOL\|use_native_schema" src/client/claude.rs
```

```output
16:pub const CLAUDE_STRUCTURED_OUTPUT_TOOL: &str = "structured_output";
102:                            && function_name != CLAUDE_STRUCTURED_OUTPUT_TOOL
133:                        if function_name == CLAUDE_STRUCTURED_OUTPUT_TOOL {
146:                        && function_name != CLAUDE_STRUCTURED_OUTPUT_TOOL
327:    let use_native_schema = model.data().supports_response_format_json_schema
329:    if use_native_schema {
332:            "name": CLAUDE_STRUCTURED_OUTPUT_TOOL,
338:            "name": CLAUDE_STRUCTURED_OUTPUT_TOOL,
376:                        if name == CLAUDE_STRUCTURED_OUTPUT_TOOL {
```

## Unit tests: body, extract, and streaming

```bash
cargo test --bin aichat -- claude::tests 2>&1 | grep -E "^test client::claude|^test result" | sed "s/finished in [0-9.]*s/finished in Xs/" | sort
```

```output
test client::claude::tests::body_does_not_inject_tool_when_capability_off ... ok
test client::claude::tests::body_does_not_inject_tool_when_schema_missing ... ok
test client::claude::tests::body_injects_synthetic_tool_when_native_schema_active ... ok
test client::claude::tests::body_merges_synthetic_tool_with_existing_functions ... ok
test client::claude::tests::extract_preserves_real_tool_calls_alongside_plain_text ... ok
test client::claude::tests::extract_returns_structured_output_tool_args_as_text ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 207 filtered out; finished in Xs
```

## Schema suffix suppression

```bash
grep -n "OUTPUT_SCHEMA_SUFFIX_MARKER\|strip_output_schema_suffix" src/config/role.rs src/config/input.rs
```

```output
src/config/role.rs:24:pub const OUTPUT_SCHEMA_SUFFIX_MARKER: &str =
src/config/role.rs:765:                    "{OUTPUT_SCHEMA_SUFFIX_MARKER}\n```json\n{schema_str}\n```\nDo not include any text outside the JSON object."
src/config/input.rs:277:            strip_output_schema_suffix(&mut messages);
src/config/input.rs:604:/// Remove the schema-injected suffix (starting at `OUTPUT_SCHEMA_SUFFIX_MARKER`)
src/config/input.rs:608:fn strip_output_schema_suffix(messages: &mut Vec<Message>) {
src/config/input.rs:609:    use crate::config::role::OUTPUT_SCHEMA_SUFFIX_MARKER;
src/config/input.rs:614:                if let Some(pos) = text.find(OUTPUT_SCHEMA_SUFFIX_MARKER) {
src/config/input.rs:678:        strip_output_schema_suffix(&mut messages);
src/config/input.rs:698:        strip_output_schema_suffix(&mut messages);
src/config/input.rs:708:        strip_output_schema_suffix(&mut messages);
```

```bash
cargo test --bin aichat -- schema_suffix_tests 2>&1 | grep -E "^test config|^test result" | sed "s/finished in [0-9.]*s/finished in Xs/" | sort
```

```output
test config::input::schema_suffix_tests::noop_when_no_suffix_present ... ok
test config::input::schema_suffix_tests::removes_system_message_entirely_when_only_suffix_was_present ... ok
test config::input::schema_suffix_tests::strips_suffix_and_keeps_original_system_prompt ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 210 filtered out; finished in Xs
```

## Capability enabled in models.yaml

```bash
grep -n -B1 "supports_response_format_json_schema" models.yaml
```

```output
147-      supports_function_calling: true
148:      supports_response_format_json_schema: true
--
172-      supports_function_calling: true
173:      supports_response_format_json_schema: true
--
197-      supports_function_calling: true
198:      supports_response_format_json_schema: true
--
222-      supports_function_calling: true
223:      supports_response_format_json_schema: true
--
247-      supports_function_calling: true
248:      supports_response_format_json_schema: true
--
593-      supports_function_calling: true
594:      supports_response_format_json_schema: true
--
617-      supports_function_calling: true
618:      supports_response_format_json_schema: true
--
641-      supports_function_calling: true
642:      supports_response_format_json_schema: true
```

## Full test suite

```bash
cargo test --bin aichat 2>&1 | grep "^test result" | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
test result: ok. 213 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
```
