# Tool Execution Compatibility Tests

*2026-03-30T16:51:35Z by Showboat 0.6.1*
<!-- showboat-id: 9cbc3ae3-abb6-4cb6-98fc-bd65f2f8f79f -->

Added module 21 (tool_execution) to tests/compatibility.rs: 19 tests that execute real tool binaries from the llm-functions directory, validating the full server-response-to-tool-execution flow. Paths are configurable via AICHAT_TEST_LLM_FUNCTIONS_DIR and AICHAT_TEST_CONFIG_DIR environment variables.

```bash
cargo test --test compatibility tool_execution 2>&1 | tail -25
```

```output
     Running tests/compatibility.rs (target/debug/deps/compatibility-6c1231dc06987250)

running 19 tests
test tool_execution::test_config_dir_has_functions ... ok
test tool_execution::test_llm_functions_dir_structure ... ok
test tool_execution::test_normalize_empty_output_to_structured_null ... ok
test tool_execution::test_config_functions_resolves_to_llm_functions ... ok
test tool_execution::test_normalize_error_result ... ok
test tool_execution::test_normalize_json_output_preserved ... ok
test tool_execution::test_normalize_text_output_wrapped ... ok
test tool_execution::test_bin_entries_resolve_and_executable ... ok
test tool_execution::test_nonexistent_tool_fails ... ok
test tool_execution::test_all_declared_tools_have_bin_entry ... ok
test tool_execution::test_real_functions_json_all_tools_valid ... ok
test tool_execution::test_claude_response_to_tool_execution ... ok
test tool_execution::test_tool_call_with_arguments_from_response ... ok
test tool_execution::test_execute_no_arg_tool ... ok
test tool_execution::test_execute_tool_with_arguments ... ok
test tool_execution::test_tool_output_written_to_llm_output_file ... ok
test tool_execution::test_openai_response_to_tool_execution ... ok
test tool_execution::test_tool_result_normalizes_text_to_json ... ok
test tool_execution::test_multi_tool_call_execution_preserves_order ... ok

test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in 0.08s

```

All 19 tests pass. The full suite (192 tests) also passes with no regressions.

```bash
cargo test --test compatibility 2>&1 | grep 'test result:'
```

```output
test result: ok. 192 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
```
