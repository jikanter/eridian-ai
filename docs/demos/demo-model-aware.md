# Model-Aware Variable Interpolation and Conditional Blocks

*2026-03-30T15:42:44Z by Showboat 0.6.1*
<!-- showboat-id: 3bc83ded-0696-4392-8f51-de2c8876fa39 -->

Extends the `{{variable}}` system so roles adapt to model capabilities at bind time. Adds model variables, conditional blocks (`{{#if}}`/`{{#unless}}`), numeric comparisons, and string equality.

**Model variables** (auto-populated): `__model_id__`, `__model_name__`, `__supports_vision__`, `__supports_function_calling__`, `__max_input_tokens__`

**Conditional blocks:** `{{#if var}}...{{/if}}` and `{{#unless var}}...{{/unless}}` for capability-gated prompt sections.

## Unit Tests

```bash
cargo test -- utils::variables::tests 2>&1 | grep -E "(running|test |test result)" | sed "s/finished in [0-9.]*s/finished in Xs/" | sort
```

```output
running 0 tests
running 28 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in Xs
test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 116 filtered out; finished in Xs
test utils::variables::tests::test_combined_conditionals_and_vars ... ok
test utils::variables::tests::test_conditional_if_falsy ... ok
test utils::variables::tests::test_conditional_if_truthy ... ok
test utils::variables::tests::test_conditional_unless ... ok
test utils::variables::tests::test_conditional_unless_truthy_hides ... ok
test utils::variables::tests::test_env_variable_does_not_match_regular_vars ... ok
test utils::variables::tests::test_env_variable_mixed_with_system_vars ... ok
test utils::variables::tests::test_env_variable_non_aichat_prefix_blocked ... ok
test utils::variables::tests::test_env_variable_ordering ... ok
test utils::variables::tests::test_env_variable_substitution ... ok
test utils::variables::tests::test_env_variable_unset ... ok
test utils::variables::tests::test_env_variable_with_model_vars ... ok
test utils::variables::tests::test_interpolate_record_fields_full_json_record ... ok
test utils::variables::tests::test_interpolate_record_fields_full_record ... ok
test utils::variables::tests::test_interpolate_record_fields_json_field ... ok
test utils::variables::tests::test_interpolate_record_fields_missing_field ... ok
test utils::variables::tests::test_interpolate_record_fields_non_json ... ok
test utils::variables::tests::test_mismatched_tags_pass_through ... ok
test utils::variables::tests::test_mixed_system_and_model_vars ... ok
test utils::variables::tests::test_model_variables_resolve ... ok
test utils::variables::tests::test_model_variables_without_model ... ok
test utils::variables::tests::test_numeric_comparison_fails ... ok
test utils::variables::tests::test_numeric_comparison_gte ... ok
test utils::variables::tests::test_numeric_comparison_lt ... ok
test utils::variables::tests::test_string_equality ... ok
test utils::variables::tests::test_string_inequality ... ok
test utils::variables::tests::test_system_vars_still_work ... ok
test utils::variables::tests::test_unresolved_var_in_conditional_passes_through ... ok
```

## Integration Tests

Test model variable interpolation with `--dry-run`:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-model-vars.md" <<'ROLE'
Model: {{__model_id__}}
Supports vision: {{__supports_vision__}}
Supports functions: {{__supports_function_calling__}}
Analyze: __INPUT__
ROLE
echo "test" | aichat --dry-run -r test-model-vars 2>/dev/null
rm "$ROLES_DIR/test-model-vars.md"
```

```output
Model: vllm:qwen3-coder
Supports vision: false
Supports functions: true
Analyze: test
```

Model variables resolve to the active model's capabilities at bind time.

Test conditional blocks — content appears or hides based on model capabilities:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-conditionals.md" <<'ROLE'
You are an assistant.
{{#if __supports_vision__}}You can analyze images.{{/if}}
{{#unless __supports_vision__}}Text-only mode.{{/unless}}
{{#if __supports_function_calling__}}You can call tools.{{/if}}
__INPUT__
ROLE
echo "hello" | aichat --dry-run -r test-conditionals 2>/dev/null
rm "$ROLES_DIR/test-conditionals.md"
```

```output
You are an assistant.

Text-only mode.
You can call tools.
hello
```

Conditional blocks resolve based on the active model's capabilities — vision and function-calling sections appear or hide dynamically.
