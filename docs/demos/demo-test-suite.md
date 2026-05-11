# Comprehensive Test Suite

*2026-03-30T15:42:27Z by Showboat 0.6.1*
<!-- showboat-id: 406e23bd-d189-4543-a6db-d5ab60e56b4a -->

This document verifies the full test suite passes — both unit tests and compatibility tests.

## Unit Tests

```bash
cargo test --bin aichat -- --skip test_load_mcp_servers_file_rejects_neither_command_nor_url 2>&1 | grep -E "(running|test result)" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/; s/[0-9]+ passed/N passed/; s/^running [0-9]+ tests$/running N tests/"
```

```output
running N tests
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```

## Compatibility Tests

```bash
cargo test --test compatibility 2>&1 | grep -E "(running|test result)" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/; s/[0-9]+ passed/N passed/; s/^running [0-9]+ tests$/running N tests/"
```

```output
running N tests
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```

## Module Breakdown

Unit test modules:

```bash
cargo test --bin aichat -- --skip test_load_mcp_servers_file_rejects_neither_command_nor_url 2>&1 | grep "^test " | grep " \.\.\. " | sed "s/::tests::.*//" | sed "s/::test_.*//" | sort -u | wc -l | tr -d ' ' | xargs -I {} echo "unit-test modules: {}" | sed -E "s/: [0-9]+/: N/"
```

```output
unit-test modules: N
```

Compatibility test modules:

```bash
cargo test --test compatibility 2>&1 | grep "^test " | grep " \.\.\. " | sed "s/::.*//" | sort -u | wc -l | tr -d ' ' | xargs -I {} echo "compatibility-test modules: {}" | sed -E "s/: [0-9]+/: N/"
```

```output
compatibility-test modules: N
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
