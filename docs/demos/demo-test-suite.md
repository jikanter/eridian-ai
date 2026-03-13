# Comprehensive Test Suite for llm-functions & argc Compatibility

*2026-03-13T15:34:58Z by Showboat 0.6.1*
<!-- showboat-id: e9811fd6-130e-47a5-b7b2-37397cdd71aa -->

This test suite validates that our changes (Phases 0-4) remain compatible with llm-functions and argc tooling. It covers: function declarations, tool dispatch, deferred tool loading, pipeline stages, MCP tool conversion, error classification, role parsing, schema validation, and CLI output formatting.

```bash
cargo test 2>&1 | grep -E "^(running|test result)"
```

```output
running 99 tests
test result: ok. 99 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
running 117 tests
test result: ok. 117 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

The test suite is organized into 20 modules covering all compatibility-critical areas:

```bash
cargo test --test compatibility 2>&1 | grep -E "^test " | sed "s/ \.\.\. ok//" | sort | awk -F"::" "{print \$1}" | uniq -c | sort -rn
```

```output
  15 test error_classification
  14 test role_parsing
   7 test typed_errors
   7 test tool_search
   7 test pipeline_parsing
   7 test mcp_tool_conversion
   6 test tool_selection
   6 test output_format
   6 test function_declaration
   5 test tool_call_dedup
   5 test schema_validation
   5 test argc_contract
   4 test variable_expansion
   4 test env_resolution
   4 test dehoist_input
   4 test config_paths
   3 test schema_cache
   3 test mcp_config
   3 test builtin_roles
   2 test mcp_output_format
   1 test result: ok. 117 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
```

**Coverage by phase:**

- **Phase 0 (Prerequisites):** Pipeline stage parsing (7 tests), tool dispatch dedup (5 tests)
- **Phase 1 (Token Efficiency):** Deferred tool loading/search (7 tests), role descriptions (2 tests), tool examples (1 test)
- **Phase 2 (Pipeline & Output):** Pipeline-as-role (7 tests), output formats (6 tests), __INPUT__ de-hoisting (4 tests)
- **Phase 3 (MCP Consumption):** MCP tool conversion (7 tests), MCP config (3 tests), MCP output (2 tests), schema cache (3 tests), env resolution (4 tests)
- **Phase 4 (Error Handling):** Error classification (15 tests), typed errors (7 tests), schema validation (5 tests)
- **Compatibility Guards:** argc contract (5 tests), built-in roles (3 tests), config paths (4 tests), role parsing (14 tests), variables (4 tests), function declarations (6 tests)
