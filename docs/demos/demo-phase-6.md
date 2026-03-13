# Phase 6: Metadata Framework Enhancements

*2026-03-13T16:35:08Z by Showboat 0.6.1*
<!-- showboat-id: 1ee29d4d-0d72-4446-977e-facf08039769 -->

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

Shell variables execute via `sh -c` at invocation time. CLI `-v` flag overrides shell defaults. Failed commands warn instead of crashing (Phase 4A pattern).

## 6B: Lifecycle Hooks

`pipe_to` pipes output to a shell command via stdin. `save_to` writes to a file with `{{timestamp}}` interpolation. Hooks fire in `start_directive` and pipeline last stage.

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

## Tests

112 tests pass (19 new Phase 6 tests + 93 existing):

```bash
cargo test 2>&1 | grep -c "^test .* ok$"
```

```output
112
```

## Combined Example

All three features in one self-contained code review role:

```bash
cat <<'YAML'
---
description: "Self-contained code reviewer"
variables:
  - name: diff
    default: { shell: "git diff --cached" }
  - name: files
    default: { shell: "git diff --cached --name-only" }
pipe_to: "pbcopy"
save_to: "./reviews/{{timestamp}}.md"
mcp_servers:
  - github-server
---
Review: {{files}}

{{diff}}

__INPUT__
YAML
```

```output
---
description: "Self-contained code reviewer"
variables:
  - name: diff
    default: { shell: "git diff --cached" }
  - name: files
    default: { shell: "git diff --cached --name-only" }
pipe_to: "pbcopy"
save_to: "./reviews/{{timestamp}}.md"
mcp_servers:
  - github-server
---
Review: {{files}}

{{diff}}

__INPUT__
```

Usage: `aichat -r code-reviewer 'Focus on security'` — gathers context (6A), binds MCP tools (6C), copies to clipboard + saves to file (6B). No manual piping.
