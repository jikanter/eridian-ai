# Composable Roles with Extends and Include Directives

*2026-02-28T19:53:36Z by Showboat 0.6.1*
<!-- showboat-id: ab6f6e6f-2e13-44ad-9532-265f163af0b7 -->

This feature adds single-parent inheritance (`extends`) and prompt fragment mixins (`include`) to role YAML frontmatter. Roles can now reuse prompt fragments and inherit from other roles without copy-paste. Composition resolves at load time with cycle detection, metadata merging, and deterministic prompt ordering.

## How It Works

**`extends`** — single-parent inheritance. A child role inherits its parent's prompt and metadata. Child metadata overrides parent defaults. The parent's prompt is prepended to the child's.

**`include`** — prompt fragment mixins. An array of role names whose prompts are prepended (in order) before the parent and child prompts. Useful for shared guardrails, output format instructions, etc.

**Prompt ordering:** includes -> parent -> child

**Metadata merging:** parent provides defaults, child overrides

**Cycle detection:** circular inheritance chains are caught at load time with a clear error message showing the chain.

## Example: Role Inheritance

A child role that extends the builtin `%code%` role with security-focused instructions:

```bash
cat <<'ROLE'
---
extends: "%code%"
temperature: 0.3
---
Focus on security. Flag any potential vulnerabilities.
ROLE
```

```output
---
extends: "%code%"
temperature: 0.3
---
Focus on security. Flag any potential vulnerabilities.
```

This role inherits the `%code%` prompt ("Provide only code...") and appends the security instructions. The child's `temperature: 0.3` overrides anything the parent sets.

## Example: Include Mixins

A role that includes shared prompt fragments for guardrails and output format:

```bash
cat <<'ROLE'
---
include:
  - safety-guardrails
  - output-json
---
You are a data analyst.
ROLE
```

```output
---
include:
  - safety-guardrails
  - output-json
---
You are a data analyst.
```

The prompts from `safety-guardrails` and `output-json` are prepended (in that order) before "You are a data analyst." — the child always comes last.

## Tests: Frontmatter Parsing

```bash
cargo test test_parse_raw_frontmatter -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.57s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 4 tests
test config::role::tests::test_parse_raw_frontmatter_no_frontmatter ... ok
test config::role::tests::test_parse_raw_frontmatter_basic ... ok
test config::role::tests::test_parse_raw_frontmatter_extends ... ok
test config::role::tests::test_parse_raw_frontmatter_include ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 48 filtered out; finished in 0.02s

```

Four parser tests pass: basic metadata extraction, `extends` directive parsing (stripped from metadata), `include` array parsing (stripped from metadata), and plain prompts without frontmatter.

## Tests: Metadata Merging

```bash
cargo test test_metadata_merge -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test config::role::tests::test_metadata_merge ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.00s

```

Parent sets `model: gpt-4` and `temperature: 0.5`. Child overrides `temperature: 0.8`. After merge, `temperature` is `0.8` (child wins) and `model` is `gpt-4` (inherited from parent).

## Tests: Prompt Ordering

```bash
cargo test test_prompt_ordering -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test config::role::tests::test_prompt_ordering ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.00s

```

Prompts concatenate in deterministic order: includes first ("Safety first."), then parent ("Be helpful."), then child ("Focus on code review."). Position assertions confirm the ordering is strict.

## Tests: Cycle Detection

```bash
cargo test test_cycle_detection -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test config::role::tests::test_cycle_detection ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.00s

```

Circular chains (A -> B -> A) are caught at resolve time. The error message includes the full chain so the user can identify where the cycle occurs.

## Tests: Builtin Role Resolution

```bash
cargo test test_resolve_builtin_passthrough -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 1 test
test config::role::tests::test_resolve_builtin_passthrough ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 51 filtered out; finished in 0.00s

```

Builtin roles (like `%code%`) that don't use extends or include resolve unchanged — full backward compatibility.

## Tests: Compose Round-Trip

```bash
cargo test test_compose_role_content -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-b1ef0eac464604f1)

running 2 tests
test config::role::tests::test_compose_role_content_no_metadata ... ok
test config::role::tests::test_compose_role_content_with_metadata ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 50 filtered out; finished in 0.01s

```

Composed output round-trips cleanly. A role composed with metadata serializes to valid YAML frontmatter that `Role::new()` can parse back — `temperature: 0.5` survives the compose -> parse cycle.

## Integration: Config Uses Role::resolve()

```bash
grep -n 'Role::resolve' src/config/mod.rs
```

```output
918:        let mut role = Role::resolve(name)?;
1030:            if let Ok(role) = Role::resolve(&name) {
```

The config layer now calls `Role::resolve()` instead of reading role files directly. Both `retrieve_role()` (single role lookup) and the role listing loop go through the composition pipeline. The old `Role::builtin()` and raw `read_to_string()` paths are replaced.

## Implementation Summary

| File | Change |
| --- | --- |
| `src/config/role.rs` | Added `RawRoleParts` struct, `parse_raw_frontmatter()`, `read_raw_role_content()`, `resolve_role_content()` (recursive with cycle detection), `compose_role_content()`, `Role::resolve()` entry point, and 9 unit tests (+302 lines) |
| `src/config/mod.rs` | Replaced direct file reads with `Role::resolve()` in `retrieve_role()` and role listing (-11/+3 lines) |
