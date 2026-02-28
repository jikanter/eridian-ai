# Model-Aware Variable Interpolation and Conditional Blocks

*2026-02-28T19:49:36Z by Showboat 0.6.1*
<!-- showboat-id: ea15e319-08bf-4d64-b421-4018b4b39b8a -->

This feature extends the `{{variable}}` system so that role and agent prompts can adapt to the active model's capabilities at bind time. A single role definition can now produce different system prompts depending on whether the model supports vision, function calling, or has a large context window — all without duplicating roles.

## New Variables

| Variable | Description |
| --- | --- |
| `__model_id__` | Full model identifier (e.g. `openai:gpt-4o`) |
| `__model_name__` | Short model name (e.g. `gpt-4o`) |
| `__model_client__` | Client/provider name (e.g. `openai`) |
| `__max_input_tokens__` | Input context limit |
| `__max_output_tokens__` | Output token limit |
| `__supports_vision__` | Whether the model handles images |
| `__supports_function_calling__` | Whether the model supports tool use |
| `__supports_stream__` | Whether streaming is enabled |

These join the existing system variables (`__os__`, `__arch__`, `__shell__`, etc.) and are resolved in a second pass when `set_model()` binds the model to the role.

## Conditional Blocks

Prompts can include `{{#if VAR}}` / `{{#unless VAR}}` blocks that expand or collapse based on variable values. Conditionals support:

- **Truthiness** — `{{#if __supports_vision__}}` (false/0/empty/unknown are falsy)
- **Numeric comparison** — `{{#if __max_input_tokens__ >= 64000}}`
- **String equality** — `{{#if __model_client__ == openai}}`
- **Negation** — `{{#unless __supports_function_calling__}}`

Mismatched tags and unresolved variables pass through unchanged, preserving backward compatibility.

## Example: Adaptive Role

A role that adjusts its instructions based on model capabilities:

```bash
cat <<'ROLE'
---
model: openai:gpt-4o
---
You are a helpful assistant running on {{__os__}}.
Model: {{__model_name__}} via {{__model_client__}}.

{{#if __supports_vision__}}
You can analyze images. When the user shares an image, describe it in detail.
{{/if}}

{{#unless __supports_function_calling__}}
You cannot call external tools. Answer from your training data only.
{{/unless}}

{{#if __max_input_tokens__ >= 64000}}
You have a large context window. Feel free to include extensive detail.
{{/if}}
ROLE
```

```output
---
model: openai:gpt-4o
---
You are a helpful assistant running on {{__os__}}.
Model: {{__model_name__}} via {{__model_client__}}.

{{#if __supports_vision__}}
You can analyze images. When the user shares an image, describe it in detail.
{{/if}}

{{#unless __supports_function_calling__}}
You cannot call external tools. Answer from your training data only.
{{/unless}}

{{#if __max_input_tokens__ >= 64000}}
You have a large context window. Feel free to include extensive detail.
{{/if}}
```

When bound to `gpt-4o` (vision: true, function calling: true, 128k context), the conditionals expand to include the vision instructions and the large-context note, while the `unless` block for function calling is suppressed.

## Tests: Variable Resolution

```bash
cargo test test_model_variables_resolve -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.64s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test utils::variables::tests::test_model_variables_resolve ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.03s

```

```bash
cargo test test_model_variables_without_model -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.11s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test utils::variables::tests::test_model_variables_without_model ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s

```

Model variables resolve to their values when a model is bound, and pass through as raw `{{var}}` syntax when no model is present (preserving them for later resolution).

## Tests: Conditional Blocks

```bash
cargo test test_conditional_if_truthy -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.11s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test utils::variables::tests::test_conditional_if_truthy ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s

```

```bash
cargo test test_conditional_if_falsy -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.11s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test utils::variables::tests::test_conditional_if_falsy ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s

```

```bash
cargo test test_conditional_unless -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 2 tests
test utils::variables::tests::test_conditional_unless_truthy_hides ... ok
test utils::variables::tests::test_conditional_unless ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 50 filtered out; finished in 0.01s

```

`{{#if}}` blocks expand when the variable is truthy and collapse when falsy. `{{#unless}}` does the inverse. Both directions work correctly.

## Tests: Numeric Comparisons

```bash
cargo test test_numeric_comparison -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.11s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 3 tests
test utils::variables::tests::test_numeric_comparison_fails ... ok
test utils::variables::tests::test_numeric_comparison_gte ... ok
test utils::variables::tests::test_numeric_comparison_lt ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 49 filtered out; finished in 0.01s

```

Numeric comparisons (`>=`, `<`, `==`, `!=`, `>`, `<=`) work against token limits and other numeric values. A gpt-4o model with 128k input tokens matches `>= 64000`, while a 4096-token model with 2048 output tokens matches `< 4096`.

## Tests: String Equality and Edge Cases

```bash
cargo test test_string_equality -- --nocapture 2>&1 && cargo test test_string_inequality -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test utils::variables::tests::test_string_equality ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.11s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test utils::variables::tests::test_string_inequality ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s

```

```bash
cargo test test_mismatched_tags -- --nocapture 2>&1 && cargo test test_unresolved_var_in_conditional -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.11s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test utils::variables::tests::test_mismatched_tags_pass_through ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.11s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test utils::variables::tests::test_unresolved_var_in_conditional_passes_through ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s

```

String equality (`== openai`, `== anthropic`) allows provider-specific instructions. Mismatched tags (`{{#if ...}}...{{/unless}}`) and unresolved variables in conditionals both pass through unchanged — no silent breakage of existing prompts.

## Tests: Backward Compatibility

```bash
cargo test test_system_vars_still_work -- --nocapture 2>&1 && cargo test test_mixed_system_and_model_vars -- --nocapture 2>&1 && cargo test test_combined_conditionals_and_vars -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test utils::variables::tests::test_system_vars_still_work ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test utils::variables::tests::test_mixed_system_and_model_vars ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test utils::variables::tests::test_combined_conditionals_and_vars ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.01s

```

Existing system variables (`__os__`, `__arch__`, `__shell__`, etc.) continue to resolve normally. They coexist with model variables and conditional blocks in the same prompt without interference.

## Full Test Suite

```bash
cargo test utils::variables -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 16 tests
test utils::variables::tests::test_numeric_comparison_fails ... ok
test utils::variables::tests::test_conditional_unless ... ok
test utils::variables::tests::test_mismatched_tags_pass_through ... ok
test utils::variables::tests::test_conditional_if_falsy ... ok
test utils::variables::tests::test_model_variables_without_model ... ok
test utils::variables::tests::test_mixed_system_and_model_vars ... ok
test utils::variables::tests::test_model_variables_resolve ... ok
test utils::variables::tests::test_conditional_if_truthy ... ok
test utils::variables::tests::test_combined_conditionals_and_vars ... ok
test utils::variables::tests::test_conditional_unless_truthy_hides ... ok
test utils::variables::tests::test_string_equality ... ok
test utils::variables::tests::test_system_vars_still_work ... ok
test utils::variables::tests::test_unresolved_var_in_conditional_passes_through ... ok
test utils::variables::tests::test_numeric_comparison_lt ... ok
test utils::variables::tests::test_string_inequality ... ok
test utils::variables::tests::test_numeric_comparison_gte ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 36 filtered out; finished in 0.02s

```

All 16 tests pass, covering variable resolution, conditional blocks (`if`/`unless`), numeric comparisons, string equality, edge cases, and backward compatibility.

## Implementation Summary

| File | Change |
| --- | --- |
| `src/utils/variables.rs` | Refactored into two-phase interpolation: conditional block processing, then variable substitution. Added `resolve_model_variables()`, `eval_comparison()`, `is_truthy()`, and 16 unit tests. |
| `src/config/role.rs` | Added `interpolate_variables_with_model()` call in `set_model()` for second-pass model variable resolution. |
| `src/config/agent.rs` | Updated import to use `interpolate_variables_with_model`. |
| `specs/001-model-aware-variables.md` | Feature specification document (240 lines). |
