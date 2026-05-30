# Phase 9A: OpenAI response_format

*2026-04-16T23:58:14Z by Showboat 0.6.1*
<!-- showboat-id: c4b89998-1280-40e9-9117-3a2061199ec2 -->

Phase 9A enables provider-native structured output for OpenAI-family clients. When a role declares `output_schema` and the model has `supports_response_format_json_schema: true`, AIChat injects OpenAI's `response_format: {type: "json_schema", json_schema: {name, strict: true, schema}}` into the chat completions request body. The redundant prompt-injected schema suffix is stripped upstream so we don't double-pay tokens. All OpenAI-compatible clients (Azure, Cohere, VertexAI-Mistral, openai_compatible) inherit the behavior via the shared `openai_build_chat_completions_body` helper.

## response_format injection in the OpenAI body builder

```bash
grep "response_format\|output_schema" src/client/openai.rs | head -15
```

```output
        output_schema,
    // `supports_response_format_json_schema` and the role has an `output_schema`,
    // use OpenAI's `response_format: json_schema` so conformance is enforced by
    if let Some(schema) = output_schema {
        if model.data().supports_response_format_json_schema {
            body["response_format"] = json!({
        m.data_mut().supports_response_format_json_schema = native_schema;
            output_schema: schema,
    fn body_injects_response_format_when_native_schema_active() {
        let rf = body.get("response_format").expect("response_format present");
    fn body_omits_response_format_when_capability_off() {
            body.get("response_format").is_none(),
            "no response_format when model doesn't support it"
    fn body_omits_response_format_when_schema_missing() {
        assert!(body.get("response_format").is_none());
```

## Unit tests: injection, capability gating, schema gating, tool coexistence

```bash
cargo test --bin aichat -- client::openai::tests::body 2>&1 | grep -E "response_format" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ passed/N passed/; s/[0-9]+ filtered out/N filtered out/" | sort
```

```output
test client::openai::tests::body_injects_response_format_when_native_schema_active ... ok
test client::openai::tests::body_omits_response_format_when_capability_off ... ok
test client::openai::tests::body_omits_response_format_when_schema_missing ... ok
test client::openai::tests::body_preserves_tools_alongside_response_format ... ok
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
cargo test --bin aichat 2>&1 | grep -c "^test result: FAILED" | xargs -I {} echo "FAILED test results: {}"
```

```output
FAILED test results: 0
```
