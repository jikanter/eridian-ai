# Phase 2: Pipeline & Output Maturity

**Status:** Done
**Commit(s):** `dde1078`

---

| Item | Status | Commit | Notes |
|---|---|---|---|
| 2A. Pipeline-as-Role | Done | `dde1078` | Roles with `pipeline:` stages callable as tools |
| 2B. Compact output modifier (`-o compact`) | Done | `dde1078` | Prompt modifier for terse LLM output |

Pipeline-as-Role is aichat's answer to Anthropic's Programmatic Tool Calling pattern. Where Anthropic uses sandboxed Python to orchestrate multiple tools and return only final results, aichat does it declaratively in YAML — an agent sees one tool, internally three models run. See [tool analysis §3](../analysis/2026-03-10-tool-analysis.md#3-anthropic-programmatic-tool-calling-code-as-orchestrator).
