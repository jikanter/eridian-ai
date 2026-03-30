# Composable Roles with Extends and Include Directives

*2026-03-30T15:31:40Z by Showboat 0.6.1*
<!-- showboat-id: 46e03f44-8a1e-4fe6-a8c2-a8d94c363aaf -->

This feature adds single-parent inheritance (`extends`) and prompt fragment mixins (`include`) to role YAML frontmatter. Roles can now reuse prompt fragments and inherit from other roles without copy-paste.

**`extends`** — single-parent inheritance. A child role inherits its parent's prompt and metadata. Child metadata overrides parent defaults. The parent's prompt is prepended to the child's.

**`include`** — prompt fragment mixins. An array of role names whose prompts are prepended (in order) before the parent and child prompts.

**Prompt ordering:** includes → parent → child
**Metadata merging:** parent provides defaults, child overrides
**Cycle detection:** circular inheritance chains are caught at load time

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

The role above extends `%code%`, inheriting its prompt while adding security-focused instructions. Here is an equivalent using `include` as a mixin:

```bash
cat <<'ROLE'
---
include:
  - "%code%"
---
Always respond in markdown. Include type annotations.
ROLE
```

```output
---
include:
  - "%code%"
---
Always respond in markdown. Include type annotations.
```

## Unit Tests

Relevant unit tests covering frontmatter parsing, metadata merging, cycle detection, and prompt ordering:

```bash
cargo test -- config::role::tests::test_parse_raw_frontmatter_extends config::role::tests::test_parse_raw_frontmatter_include config::role::tests::test_metadata_merge config::role::tests::test_prompt_ordering config::role::tests::test_cycle_detection config::role::tests::test_resolve_builtin_passthrough config::role::tests::test_compose_role_content_no_metadata config::role::tests::test_compose_role_content_with_metadata 2>&1 | grep -E "^(running|test |test result)" | sort | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
running 0 tests
running 8 tests
test config::role::tests::test_compose_role_content_no_metadata ... ok
test config::role::tests::test_compose_role_content_with_metadata ... ok
test config::role::tests::test_cycle_detection ... ok
test config::role::tests::test_metadata_merge ... ok
test config::role::tests::test_parse_raw_frontmatter_extends ... ok
test config::role::tests::test_parse_raw_frontmatter_include ... ok
test config::role::tests::test_prompt_ordering ... ok
test config::role::tests::test_resolve_builtin_passthrough ... ok
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in Xs
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 136 filtered out; finished in Xs
```

## Integration Tests

Create a temporary role that extends `%code%` and verify the composed prompt via `--dry-run`:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-extends-demo.md" <<'ROLE'
---
extends: "%code%"
temperature: 0.3
---
Focus on security. Flag any potential vulnerabilities.
ROLE
echo "hello" | aichat --dry-run -r test-extends-demo 2>/dev/null
rm "$ROLES_DIR/test-extends-demo.md"
```

````output
---
temperature: 0.3
extends: '%code%'
---

Provide only code without comments or explanations.
### INPUT:
async sleep in js
### OUTPUT:
```javascript
async function timeout(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}
```

Focus on security. Flag any potential vulnerabilities.

hello
````

The dry-run output shows the parent prompt ("Provide only code...") composed with the child's security instructions — `extends` inheritance is working end-to-end.

Now test `include` as a mixin:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-include-demo.md" <<'ROLE'
---
include:
  - "%code%"
---
Always respond in markdown. Include type annotations.
ROLE
echo "hello" | aichat --dry-run -r test-include-demo 2>/dev/null
rm "$ROLES_DIR/test-include-demo.md"
```

````output
---
include:
- '%code%'
---

Provide only code without comments or explanations.
### INPUT:
async sleep in js
### OUTPUT:
```javascript
async function timeout(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}
```

Always respond in markdown. Include type annotations.

hello
````

The `include` mixin also prepends the `%code%` prompt before the child's own instructions — same composition ordering, different semantics.
