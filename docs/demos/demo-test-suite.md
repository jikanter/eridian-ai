# Comprehensive Test Suite

*2026-03-30T15:42:27Z by Showboat 0.6.1*
<!-- showboat-id: 406e23bd-d189-4543-a6db-d5ab60e56b4a -->

This document verifies the full test suite passes — both unit tests and compatibility tests.

## Unit Tests

```bash
cargo test --bin aichat 2>&1 | grep -E "(running|test result)" | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
running 144 tests
test result: ok. 144 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
```

## Compatibility Tests

```bash
cargo test --test compatibility 2>&1 | grep -E "(running|test result)" | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
running 173 tests
test result: ok. 173 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
```

## Module Breakdown

Unit test modules:

```bash
cargo test --bin aichat 2>&1 | grep "^test " | grep " \.\.\. " | sed "s/::tests::.*//" | sed "s/::test_.*//" | sort -u
```

```output
test cli
test client::common
test client::stream
test config::role
test rag::splitter
test render::markdown
test repl
test repl::completer
test utils
test utils::exit_code
test utils::path
test utils::render_prompt
test utils::trace
test utils::variables
```

Compatibility test modules:

```bash
cargo test --test compatibility 2>&1 | grep "^test " | grep " \.\.\. " | sed "s/::.*//" | sort -u
```

```output
test argc_contract
test builtin_roles
test config_paths
test dehoist_input
test env_resolution
test error_classification
test function_declaration
test mcp_config
test mcp_output_format
test mcp_tool_conversion
test output_format
test phase7_tool_errors
test phase8_timeout_and_concurrency
test pipeline_parsing
test role_parsing
test schema_cache
test schema_validation
test tool_call_dedup
test tool_search
test tool_selection
test typed_errors
test variable_expansion
```

## Integration Tests

Verify the CLI binary runs and basic commands work:

```bash
aichat --help 2>&1 | head -3
```

```output
All-in-one LLM CLI Tool

Usage: aichat [OPTIONS] [TEXT]...
```

```bash
aichat --list-roles 2>&1 | head -6
```

```output
%code%
%create-prompt%
%create-title%
%explain-shell%
%functions%
%shell%
```

```bash
echo "test" | aichat --dry-run -r %code% 2>/dev/null
```

````output
Provide only code without comments or explanations.
### INPUT:
async sleep in js
### OUTPUT:
```javascript
async function timeout(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}
```

test
````

All tests pass and the CLI binary is functional.
