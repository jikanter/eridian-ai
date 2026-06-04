# Pipeline Stage Config Isolation

A pipeline stage can declare a `config_override:` block that runs that stage against a *clone* of the global config. Tool permissions, sampling, MCP bindings, and working directory set on a stage never leak to sibling stages — and the override can only **narrow** what the parent role already holds, never escalate beyond it.

> **Status (2026-06-01):** **Shipped — Phase 36 complete (36A–D).** A stage's `config_override:` parses from role frontmatter (36A), is applied via clone-and-merge at the stage boundary — config-scoped fields (`working_directory`, `mcp_servers`) on the cloned `Config`, role-scoped fields (`use_tools`, `temperature`, `top_p`, `max_output_tokens`) on the stage's resolved role (36B), is preflight-checked so it can only narrow permissions (36C), and the applied field names surface in the `-o json` stage trace as `config_overrides_applied` (36D). Stages with no `config_override:` behave exactly as before — isolation is opt-in.

See also: [Phase 36 design](../roadmap/archive/phase-36-overview.md), [architecture.md](../architecture/architecture.md), [Typed Input](./typed-input.md).

## Why

Before Phase 36, a pipeline restored only the **model** field between stages. Tool permissions, MCP credentials, and the working directory were shared across the whole run via one `GlobalConfig`. A research stage and an implementation stage in the same pipeline could read each other's MCP credentials and write to each other's working directory — the opposite of the sub-agent isolation the pipeline-as-role pattern is meant to model.

`config_override:` closes that gap: each overriding stage runs against an isolated config clone, and the override is downward-only.

## At a glance

```yaml
---
use_tools: "fs_read,grep,run_command"
pipeline:
  - role: research
    model: claude-haiku-4-5
    config_override:
      use_tools: "fs_read,grep"     # narrow: drop run_command for research
      working_directory: ./scratch  # must stay inside the parent's tree
      temperature: 0.0
  - role: implement
    model: claude-sonnet-4-6
    # no config_override: inherits the parent role's tools + cwd
---
```

What happens:

1. The `research` stage runs against a clone of the global config. `use_tools` narrows to `fs_read,grep`; `temperature` is pinned to `0.0`; commands spawned by the stage run with `cwd = ./scratch`.
2. The clone is dropped when the stage returns — nothing leaks to `implement`.
3. `implement` has no override, so it shares the parent config unchanged (no clone cost).

## `PartialConfig` — the override surface

A `config_override:` deserializes into `PartialConfig` (`#[serde(deny_unknown_fields)]`, `#[non_exhaustive]`). Only these fields may be overridden:

| Field | Scope | Escalation rule |
|---|---|---|
| `use_tools` | role | Must be a **subset** of the parent role's `use_tools`. Parent `"all"` permits any child set; a child `"all"` is rejected unless the parent is also `"all"`; a parent with no tools rejects any grant. |
| `mcp_servers` | config | **Disable-only** this release: `[]` disables all MCP for the stage; any non-empty list is rejected. |
| `working_directory` | config | Must be a lexical **descendant of** (or equal to) the parent's working directory. `../` escapes are rejected. |
| `temperature` | role | Tuning knob — no escalation check. |
| `top_p` | role | Tuning knob — no escalation check. |
| `max_output_tokens` | role | Tuning knob — no escalation check. |

A typo'd key fails to deserialize (`deny_unknown_fields`). Note: the role *frontmatter* parser skips an unparseable pipeline node with a warning rather than hard-failing the role — a pre-existing, pipeline-wide leniency left unchanged by this phase.

Fields deliberately **not** overridable: `clients` (model providers are stage-level via `model:`), `rag` (conversation-scoped), `sessions` (pipeline-scoped).

## Validating offline — `--check`

The escalation guard runs at execution preflight **and** under `--check`, so violations are caught without a model call or network:

```bash
aichat -r my-pipeline --check        # exit 0 + "check passed" when overrides only narrow
aichat -r my-pipeline --check -o json # {"valid": true, ...}
```

Rejections exit non-zero (config error → exit code 3) with a teaching message:

- `use_tools` escalation — names the offending tool and tells you to add it to the parent or remove it from the stage ("…may only narrow…").
- `working_directory` escape — reports that the path "escapes" the parent tree.
- non-empty `mcp_servers` — reports MCP re-selection is "not supported" this release (use `[]` to disable).

## Telemetry — `config_overrides_applied`

Each stage trace in `-o json` carries `config_overrides_applied`, listing (in a fixed order) the `PartialConfig` fields that fired for that stage:

```json
{
  "stages": [
    { "role": "research",  "config_overrides_applied": ["use_tools", "working_directory", "temperature"] },
    { "role": "implement", "config_overrides_applied": [] }
  ]
}
```

The field is omitted from a stage's trace when empty (`skip_serializing_if`), and the human-readable `-o text` output omits it entirely — JSON gets the audit detail, text gets the summary. This keeps pipeline isolation auditable by default rather than silent.

## Backward compatibility

Total. A pipeline with no `config_override:` declarations runs identically to before Phase 36 — same execution path, same shared `Config`, same trace minus the new (empty, hence omitted) field. The override mechanism adds cost only to the stages that opt in.
