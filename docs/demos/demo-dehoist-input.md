# De-hoisting __INPUT__ in Extended Roles

*2026-03-10T17:27:51Z by Showboat 0.6.1*
<!-- showboat-id: 0b590044-1d0f-4b8e-8331-0373802f1c41 -->

When a child role extends a parent that contains `__INPUT__`, the placeholder previously stayed at the parent's original position — in the middle of the combined prompt. This meant child instructions ended up *after* the user input, which broke prompt ordering on `--dry-run` and during actual inference.

The de-hoist feature fixes this by relocating `__INPUT__` during role resolution:

1. **Auto-tail (default):** If the child does *not* declare `__INPUT__`, the parent's token is stripped and re-appended at the end of the combined prompt — after all instructions.
2. **Child-wins:** If the child *does* declare `__INPUT__` in its own body, the parent's token is stripped and the child's position is used instead.

This ensures child instructions always precede user input, regardless of where the parent placed `__INPUT__`.

## The Problem

Consider a parent role `%create-prompt%` that embeds `__INPUT__` mid-prompt:

```bash
tail -2 assets/roles/%create-prompt%.md
```

```output

My first request is: __INPUT__
```

And a child role `prompt-designer` that extends it:

```bash
cat ~/Library/Application\ Support/aichat/roles/prompt-designer.md
```

```output
---
extends: "%create-prompt%"
---
Assume the persona of an expert prompt engineer specializing in AI alignment. Your task is to rewrite the provided system instruction to enhance its clarity, precision, and effectiveness. The revised instruction must preserve the original intent and adhere to established AI communication best practices. Your response must consist solely of the refined system instruction, with no additional commentary, analysis, or introductory text.
```

**Before de-hoisting**, the concatenated prompt was: parent instructions, then "My first request is: __INPUT__", then child instructions. The `__INPUT__` token sat in the middle — user input got injected before the child's instructions, putting them out of order.

## The Fix

**After de-hoisting**, `__INPUT__` is stripped from the parent and relocated to the end. The resolved prompt becomes: parent instructions (token removed), then child instructions, then `__INPUT__` at the very end. All instructions now precede the user input.

## Tests: Auto-Tail (child has no __INPUT__)

```bash
cargo test test_dehoist_input_placeholder_auto_tail -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-36ac9b2d8a5415a1)

running 1 test
test config::role::tests::test_dehoist_input_placeholder_auto_tail ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 92 filtered out; finished in 0.00s

```

When the parent has `__INPUT__` and the child does not, the token is stripped from the parent and appended at the very end of the combined prompt. The test asserts that `__INPUT__` appears after all child instructions, and that only one `__INPUT__` exists in the result.

## Tests: Child-Wins (child re-declares __INPUT__)

```bash
cargo test test_dehoist_input_placeholder_child_wins -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-36ac9b2d8a5415a1)

running 1 test
test config::role::tests::test_dehoist_input_placeholder_child_wins ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 92 filtered out; finished in 0.00s

```

When the child re-declares `__INPUT__` in its own body, the parent's token is stripped and the child's position is used. The test asserts that only one `__INPUT__` exists, that it appears at the child's chosen location ("Rewrite this: __INPUT__"), and that the parent's original location ("My request is: __INPUT__") is gone.

## Full Test Suite

```bash
cargo test config::role::tests -- --nocapture 2>&1
```

```output
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/main.rs (target/debug/deps/aichat-36ac9b2d8a5415a1)

running 25 tests
test config::role::tests::test_compose_role_content_no_metadata ... ok
test config::role::tests::test_cycle_detection ... ok
test config::role::tests::test_dehoist_input_placeholder_child_wins ... ok
test config::role::tests::test_dehoist_input_placeholder_auto_tail ... ok
test config::role::tests::test_metadata_merge ... ok
test config::role::tests::test_parse_structure_prompt1 ... ok
test config::role::tests::test_parse_structure_prompt2 ... ok
test config::role::tests::test_prompt_ordering ... ok
test config::role::tests::test_parse_structure_prompt3 ... ok
test config::role::tests::test_resolve_builtin_passthrough ... ok
test config::role::tests::test_parse_raw_frontmatter_no_frontmatter ... ok
test config::role::tests::test_parse_raw_frontmatter_basic ... ok
test config::role::tests::test_parse_raw_frontmatter_include ... ok
test config::role::tests::test_parse_raw_frontmatter_extends ... ok
test config::role::tests::test_validate_schema_not_json ... ok
test config::role::tests::test_parse_role_variables_from_frontmatter ... ok
test config::role::tests::test_role_with_schemas ... ok
test config::role::tests::test_role_variables_empty ... ok
test config::role::tests::test_role_without_schemas ... ok
test config::role::tests::test_compose_role_content_with_metadata ... ok
test config::role::tests::test_role_variable_with_default ... ok
test config::role::tests::test_role_variable_apply ... ok
test config::role::tests::test_role_variables_coexist_with_system_vars ... ok
test config::role::tests::test_validate_schema_success ... ok
test config::role::tests::test_validate_schema_failure ... ok

test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 68 filtered out; finished in 0.02s

```

All 25 role tests pass, including the 2 new de-hoist tests. Existing behavior is preserved — roles without `extends`, or parents without `__INPUT__`, are completely unaffected.

## Implementation Summary

| File | Change |
| --- | --- |
| `src/config/role.rs` | In `resolve_role_content()`: detect `__INPUT__` in parent and child prompts during `extends` resolution. Strip parent's token, let child's position win, or auto-append at end. Added 2 unit tests. |

## Usage

**Auto-tail** — just extend a parent with `__INPUT__`. No changes needed to the child; the token is automatically relocated to the end of the combined prompt.

**Child-wins** — re-declare `__INPUT__` in the child's body to control exactly where user input is interpolated. The parent's `__INPUT__` is stripped; only the child's survives.
