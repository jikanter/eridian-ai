# Phase 6: Metadata Framework Enhancements

*2026-03-30T15:36:23Z by Showboat 0.6.1*
<!-- showboat-id: ce1df082-2886-43cb-b1be-1c0ccc68f4df -->

Phase 6 adds three features that make roles self-contained workflow units:

- **6A: Shell-injective variables** — `{ shell: "cmd" }` defaults gather context at invocation time
- **6B: Lifecycle hooks** — `pipe_to` and `save_to` route output to commands or files
- **6C: Unified resource binding** — `mcp_servers` per-role auto-binds MCP server tools

## 6A: Shell-Injective Variables

The `VariableDefault` enum supports both plain strings and shell commands:

```bash
grep -A 5 "pub enum VariableDefault" src/config/role.rs
```

```output
pub enum VariableDefault {
    Value(String),
    Shell { shell: String },
}

impl VariableDefault {
```

Shell variables execute via `sh -c` at invocation time. CLI `-v` flag overrides shell defaults.

## 6B: Lifecycle Hooks

`pipe_to` pipes output to a shell command via stdin. `save_to` writes to a file with `{{timestamp}}` interpolation.

## 6C: Unified Resource Binding

`mcp_servers` in role frontmatter references server names from global config. On activation, `retrieve_role()` auto-expands `use_tools` with `server:*` wildcards:

```bash
grep -A 8 "Phase 6C: Auto-bind" src/config/mod.rs
```

```output
        // Phase 6C: Auto-bind MCP server tools to the role's use_tools
        if !role.role_mcp_servers().is_empty() {
            let mcp_prefixes: Vec<String> = role
                .role_mcp_servers()
                .iter()
                .filter(|s| self.mcp_servers.contains_key(s.as_str()))
                .map(|s| format!("{s}:*"))
                .collect();
            if !mcp_prefixes.is_empty() {
```

## Unit Tests

```bash
cargo test -- config::role::tests::test_shell_variable config::role::tests::test_value_variable config::role::tests::test_pipe config::role::tests::test_save config::role::tests::test_mcp_servers config::role::tests::test_both_hooks config::role::tests::test_no_hooks config::role::tests::test_hooks_in config::role::tests::test_all_phase6 2>&1 | grep -E "(running|test .*\.\.\.|test result)" | sed "s/finished in [0-9.]*s/finished in Xs/" | sort
```

```output
running 0 tests
running 19 tests
test config::role::tests::test_all_phase6_fields_coexist ... ok
test config::role::tests::test_both_hooks_parsing ... ok
test config::role::tests::test_hooks_in_export ... ok
test config::role::tests::test_mcp_servers_empty_by_default ... ok
test config::role::tests::test_mcp_servers_in_export ... ok
test config::role::tests::test_mcp_servers_parsing ... ok
test config::role::tests::test_no_hooks_by_default ... ok
test config::role::tests::test_pipe_output_to_command ... ok
test config::role::tests::test_pipe_to_parsing ... ok
test config::role::tests::test_save_output_to_path ... ok
test config::role::tests::test_save_to_parsing ... ok
test config::role::tests::test_save_to_timestamp_interpolation ... ok
test config::role::tests::test_shell_variable_default_parsing ... ok
test config::role::tests::test_shell_variable_in_role_new ... ok
test config::role::tests::test_shell_variable_multiline_output ... ok
test config::role::tests::test_shell_variable_resolve_failure ... ok
test config::role::tests::test_shell_variable_resolve_success ... ok
test config::role::tests::test_shell_variable_resolve_trims_whitespace ... ok
test config::role::tests::test_value_variable_resolve ... ok
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 173 filtered out; finished in Xs
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 125 filtered out; finished in Xs
```

## Integration Tests

Test shell-injective variable resolution with `--dry-run`:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-shell-var-demo.md" <<'ROLE'
---
variables:
  - name: hostname
    default:
      shell: "echo injected-host"
  - name: date
    default:
      shell: "echo 2026-01-01"
---
System hostname: {{hostname}}
Current date: {{date}}
Analyze: __INPUT__
ROLE
echo "check status" | aichat --dry-run -r test-shell-var-demo 2>/dev/null
rm "$ROLES_DIR/test-shell-var-demo.md"
```

```output
System hostname: injected-host
Current date: 2026-01-01
Analyze: check status
```

The shell variables resolved at invocation time, injecting the actual hostname and date into the prompt.

Test CLI `-v` flag overriding shell defaults:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-shell-override.md" <<'ROLE'
---
variables:
  - name: context
    default:
      shell: "echo auto-gathered-context"
---
Context: {{context}}
__INPUT__
ROLE
echo "test" | aichat --dry-run -r test-shell-override -v context=manual-override 2>/dev/null
rm "$ROLES_DIR/test-shell-override.md"
```

```output
Context: manual-override
test
```

The `-v context=manual-override` flag took precedence over the shell default, proving the override chain works.

Test lifecycle hooks with a save_to path:

```bash
ROLES_DIR="/Users/admin/Library/Application Support/aichat/roles"
cat > "$ROLES_DIR/test-hooks-demo.md" <<'ROLE'
---
pipe_to: "cat > /dev/null"
save_to: "/tmp/aichat-test-{{timestamp}}.md"
---
Summarize: __INPUT__
ROLE
echo "test input" | aichat --dry-run -r test-hooks-demo 2>&1
rm "$ROLES_DIR/test-hooks-demo.md"
```

```output
---
pipe_to: cat > /dev/null
save_to: /tmp/aichat-test-{{timestamp}}.md
---

Summarize: test input
```

The role loaded successfully with hooks configured. In non-dry-run mode, `pipe_to` and `save_to` execute after LLM output.
