# Phase 9A: OpenAI response_format

*2026-04-16T23:58:14Z by Showboat 0.6.1*
<!-- showboat-id: c4b89998-1280-40e9-9117-3a2061199ec2 -->

Phase 9A enables provider-native structured output for OpenAI-family clients. When a role declares `output_schema` and the model has `supports_response_format_json_schema: true`, AIChat injects OpenAI's `response_format: {type: "json_schema", json_schema: {name, strict: true, schema}}` into the chat completions request body. The redundant prompt-injected schema suffix is stripped upstream so we don't double-pay tokens. All OpenAI-compatible clients (Azure, Cohere, VertexAI-Mistral, openai_compatible) inherit the behavior via the shared `openai_build_chat_completions_body` helper.

## response_format injection in the OpenAI body builder

```bash
grep -n "response_format\|output_schema" src/client/openai.rs | head -15
```

```output
240:        output_schema,
353:    // `supports_response_format_json_schema` and the role has an `output_schema`,
354:    // use OpenAI's `response_format: json_schema` so conformance is enforced by
358:    if let Some(schema) = output_schema {
359:        if model.data().supports_response_format_json_schema {
360:            body["response_format"] = json!({
447:        m.data_mut().supports_response_format_json_schema = native_schema;
476:            output_schema: schema,
481:    fn body_injects_response_format_when_native_schema_active() {
487:        let rf = body.get("response_format").expect("response_format present");
495:    fn body_omits_response_format_when_capability_off() {
500:            body.get("response_format").is_none(),
501:            "no response_format when model doesn't support it"
506:    fn body_omits_response_format_when_schema_missing() {
510:        assert!(body.get("response_format").is_none());
```

## Unit tests: injection, capability gating, schema gating, tool coexistence

```bash
cargo test --bin aichat -- client::openai::tests 2>&1 | grep -E "^test client::openai|^test result" | sed "s/finished in [0-9.]*s/finished in Xs/" | sort
```

```output
test client::openai::tests::body_injects_response_format_when_native_schema_active ... ok
test client::openai::tests::body_omits_response_format_when_capability_off ... ok
test client::openai::tests::body_omits_response_format_when_schema_missing ... ok
test client::openai::tests::body_preserves_tools_alongside_response_format ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 219 filtered out; finished in Xs
```

## Capability enabled in models.yaml for supported OpenAI models

```bash
awk "/- name:/{n=\$0} /supports_response_format_json_schema: true/{print n}" models.yaml
```

```output
    - name: gpt-5.2
    - name: gpt-5
    - name: gpt-5-mini
    - name: gpt-5-nano
    - name: gpt-4.1
    - name: gpt-4o
    - name: claude-opus-4-6
    - name: claude-sonnet-4-6
    - name: claude-opus-4-5-20251101
    - name: claude-sonnet-4-5-20250929
    - name: claude-haiku-4-5-20251001
    - name: claude-opus-4-5@20251101
    - name: claude-sonnet-4-5@20250929
    - name: claude-haiku-4-5@20251001
```

## Compatible clients inherit the behavior via the shared body builder

```bash
grep -l "openai_build_chat_completions_body" src/client/*.rs
```

```output
src/client/azure_openai.rs
src/client/cohere.rs
src/client/openai_compatible.rs
src/client/openai.rs
src/client/vertexai.rs
```

## Full test suite

```bash
cargo test --bin aichat 2>&1 | grep "^test result" | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
test result: ok. 223 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
```
