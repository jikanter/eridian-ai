# Phase 6: Metadata Framework Enhancements

**Status:** Done

---

| Item | Status | Notes |
|---|---|---|
| 6A. Shell-injective variables | Done | `VariableDefault` union type: `Value(String)` or `Shell { shell }`. Executed via `sh -c` at invocation time. Failures warn instead of crashing (Phase 4A pattern). |
| 6B. Lifecycle hooks | Done | `pipe_to:` pipes output to shell command via stdin. `save_to:` writes to file with `{{timestamp}}` interpolation. Fires in `start_directive` and pipeline last stage. |
| 6C. Unified resource binding | Done | `mcp_servers:` field per-role (list of server names from global config). Auto-expands `use_tools` with `server:*` wildcards. Warns on unknown server names. |

Phase 6A turns roles into self-contained context-gathering units that leverage existing CLI tools (`git`, `grep`, `find`) as context providers. Phase 6B enables zero-friction output routing. Phase 6C means selecting a role configures its entire tool environment. See [Junie metadata plan](../2026-03-10-junie-plan.md).

**YAML examples:**
```yaml
# 6A: Shell-injective variable
variables:
  - name: git_diff
    default: { shell: "git diff --cached" }

# 6B: Lifecycle hooks
pipe_to: "pbcopy"
save_to: "./logs/{{timestamp}}.md"

# 6C: Per-role MCP server binding
mcp_servers:
  - sqlite-server
```
