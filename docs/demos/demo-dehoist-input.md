# De-hoisting __INPUT__ in Extended Roles

*2026-03-30T15:31:43Z by Showboat 0.6.1*
<!-- showboat-id: 40800ff0-25f1-4805-bf37-591b02c96aa2 -->

When a child role extends a parent that contains `__INPUT__`, the system must decide where user input appears in the combined prompt. Two strategies:

**Auto-tail (default):** If the parent has `__INPUT__` but the child does not, move `__INPUT__` to the end of the combined prompt so child instructions always precede user input.

**Child-wins:** If the child also has `__INPUT__`, the child's placement is used and the parent's is removed.

## Unit Tests

```bash
cargo test -- config::role::tests::test_dehoist 2>&1 | grep -E "(running|test |test result)" | sort
```

```output
running 0 tests
running 2 tests
test config::role::tests::test_dehoist_input_placeholder_auto_tail ... ok
test config::role::tests::test_dehoist_input_placeholder_child_wins ... ok
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 142 filtered out; finished in 0.00s
```

```bash
cargo test --test compatibility -- dehoist_input 2>&1 | grep -E "(running|test |test result)" | sort
```

```output
running 4 tests
test dehoist_input::test_dehoist_auto_tail ... ok
test dehoist_input::test_dehoist_child_wins ... ok
test dehoist_input::test_dehoist_neither_has_input ... ok
test dehoist_input::test_dehoist_only_child_has_input ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 169 filtered out; finished in 0.00s
```

## Integration Tests

Create temporary roles to demonstrate auto-tail de-hoisting with `--dry-run`:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
# Parent role with __INPUT__ in the middle
cat > "$ROLES_DIR/test-parent-input.md" <<'ROLE'
You are a translator.
My request is: __INPUT__
Translate to French.
ROLE
# Child role that extends parent (no __INPUT__)
cat > "$ROLES_DIR/test-child-dehoist.md" <<'ROLE'
---
extends: test-parent-input
---
Always use formal language.
ROLE
echo "hello world" | aichat --dry-run -r test-child-dehoist 2>/dev/null
rm "$ROLES_DIR/test-parent-input.md" "$ROLES_DIR/test-child-dehoist.md"
```

```output
---
extends: test-parent-input
---

You are a translator.
My request is: 
Translate to French.

Always use formal language.

hello world
```

The `__INPUT__` placeholder moved from the middle of the parent's prompt to the end of the combined prompt, ensuring the child's "Always use formal language" instruction precedes user input.

Now test child-wins de-hoisting — child specifies its own `__INPUT__` placement:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-parent-input2.md" <<'ROLE'
You are a translator.
My request is: __INPUT__
Translate to French.
ROLE
cat > "$ROLES_DIR/test-child-wins.md" <<'ROLE'
---
extends: test-parent-input2
---
Context: __INPUT__
Always use formal language.
ROLE
echo "hello world" | aichat --dry-run -r test-child-wins 2>/dev/null
rm "$ROLES_DIR/test-parent-input2.md" "$ROLES_DIR/test-child-wins.md"
```

```output
---
extends: test-parent-input2
---

You are a translator.
My request is: 
Translate to French.

Context: hello world

Always use formal language.
```

The child's `__INPUT__` placement is respected — parent's `__INPUT__` was removed, and the child placed it where it wanted.
