# Initial Phased Roadmap: Token-Efficient Tool Orchestration

**Date:** 2026-03-10
**Source analysis:** [tool-analysis.md](../analysis/2026-03-10-tool-analysis.md)
**Reviewed by:** ML Integration Engineer, ML Architect, UX Engineer (parallel critique)

---

## Design Review Summary

Three specialist reviewers critiqued the original tier proposal. Their input produced the following material changes:

| Original Proposal | Reviewer | Change |
|---|---|---|
| Tier 1B: Pipeline-as-Tool | Integration + UX | Demoted to Phase 2. Prerequisite work needed: pipeline stages can't do tool calling (`pipe.rs` calls `call_chat_completions`, not `call_react`). Config mutation is a shared-state bug. |
| Tier 2D: TOON Output Format | Architect | **Killed.** No model has been trained on TOON. Will produce malformed output across providers. Use existing formats + a `compact` prompt modifier instead. |
| Tier 3E: Tool Use Examples | Architect | **Promoted to Phase 1.** Biggest single accuracy improvement (72% -> 90%). Already partially implemented via `### INPUT: / ### OUTPUT:` in `parse_structure_prompt`. Trivial to formalize. |
| Tier 3F: Deferred Tool Loading | Integration | **Promoted to Phase 1.** Directly fixes the known `use_tools: all` bug (21K tokens). Highest token-savings-per-line-of-code change. |
| Tier 2C: MCP Consumption | All three | **Demoted to Phase 3.** Much harder than "medium effort" — needs process lifecycle management, caching, auth passthrough, Rust MCP client library. Separate design doc required. UX reviewer questions whether it belongs in aichat at all vs. a separate binary ("one tool per job"). |
| `--mcp` flag overloading | UX | Rejected. `--mcp` (serve) and `--mcp-server <CMD>` (consume) must be distinct. |
| `-r <role> --describe` | UX | Rejected. Use existing `--info` flag instead. Extend `--info` output when a role is selected. |
| New: `-o json` for metadata | UX | Added. Extend `-o` to `--list-*` and `--info` commands. Highest-leverage agent UX change. |
| New: Tool count warning | Integration + Architect | Added as Phase 0 prerequisite. 10-line change that prevents the motivating problem. |
| New: Per-stage context guards | Architect | Added as Phase 0. Pipeline stages don't check `guard_max_input_tokens()` before invocation. |
| Two-step discovery on small models | Architect | MCP lazy discovery gated behind model capability. Don't enable for sub-14B models — accuracy drops 15-25% with discovery indirection. |

---

## Phase 0: Prerequisites

*Targeted fixes to existing code. No new features. Each is a small, isolated change.*

### 0A. Tool Count Warning

**Problem:** `use_tools: all` silently injects 86K+ characters into the system prompt, causing hangs with local models. No feedback to the user.

**Changes:**
- In `select_functions()` (`src/config/mod.rs`): warn when tool count exceeds a threshold (e.g., 20 tools)
- Log tool count at debug level for every invocation
- Add a configurable timeout with actionable error message: `"Tool definitions exceed N tokens. Scope use_tools to specific tools: use_tools: tool1,tool2"`

**Scope:** ~10-20 lines in `src/config/mod.rs`.

### 0B. Pipeline Tool-Calling Support

**Problem:** `src/pipe.rs` calls `call_chat_completions` directly, bypassing the `call_react` tool-calling loop. No pipeline stage can use tools. This blocks Phase 2A (Pipeline-as-Role) and undermines the "make for AI" positioning.

**Changes:**
- When a pipeline stage's role has `use_tools`, call `call_react` instead of `call_chat_completions`
- Strip `<think>` tags between stages when routing from a reasoning model to a non-reasoning model
- Add per-stage input token guard: call `guard_max_input_tokens()` after constructing the stage input, before calling the model

**Risk:** `call_react` requires the full agent loop from `src/main.rs`. Extracting it into a callable function may require refactoring. If extraction is too costly, a simpler approach: inject the role's tools into the `call_chat_completions` request and handle tool calls in a local loop within `run_stage`.

**Scope:** ~50-100 lines in `src/pipe.rs`, possible refactor of `call_react` in `src/client/common.rs`.

### 0C. Pipeline Config Isolation

**Problem:** `pipe.rs` mutates global config state to switch models (`config.write().set_model()`). This affects concurrent operations and leaves config in a dirty state if a stage fails.

**Changes:**
- Each pipeline stage creates a config snapshot (clone) rather than mutating the shared `Arc<RwLock<Config>>`
- Stage-local config is discarded after the stage completes
- Original config is restored regardless of success or failure

**Scope:** ~20-30 lines in `src/pipe.rs`.

---

## Phase 1: Token Efficiency Foundations

*Low effort, high impact. Every feature here either saves tokens or improves accuracy.*

### 1A. Structured Metadata Output (`-o` for `--list-*` and `--info`)

**Rationale (UX):** This is the single highest-leverage agent UX change. Currently `--list-roles` returns bare names, one per line. Agents need structured data to make decisions. The `-o` mechanism already exists for LLM output; extending it to metadata commands serves both humans (default text) and agents (`-o json`) with zero new flags.

**Changes:**
- `--list-roles -o json` produces:
  ```json
  [
    {"name": "reviewer", "model": "openai:gpt-4o", "tools": ["fs_cat", "fs_ls"], "description": "Code review with inline feedback"},
    {"name": "summarize", "model": "claude-sonnet-4-6", "tools": [], "description": "Summarize text input"}
  ]
  ```
- `--list-roles` (no `-o`) remains unchanged: one name per line
- `aichat -r <role> --info -o json` produces a detailed JSON object (prompt_length, model, temperature, tools, variables, schemas) — replaces the proposed `--describe` flag
- Same pattern for `--list-models`, `--list-sessions`, etc.

**Implementation:** In `main.rs`, the `--list-roles` handler (lines 91-95) is 4 lines. Add an `if let Some(fmt) = cli.output { ... }` branch. For roles, add a `description` field to role YAML frontmatter (optional, first line of prompt used as fallback).

**Token budget (for agent consuming 30 roles):**

| Method | Tokens |
|---|---|
| Current: agent calls `--info` per role (30 calls) | ~6,000 |
| Proposed: `--list-roles -o json` (1 call) | ~480 |

**Scope:** ~40-60 lines across `main.rs` and `src/config/role.rs`.

### 1B. Role Description Metadata

**Rationale:** Feeds into 1A. Roles need a compact, human/agent-readable description.

**Changes:**
- Add optional `description:` field to role YAML frontmatter
- If not provided, derive from first sentence of the role prompt
- Used in `--list-roles -o json`, MCP tool descriptions, and `--info` output

**Scope:** ~15-20 lines in `src/config/role.rs`.

### 1C. Deferred Tool Loading

**Rationale (Integration + Architect):** Directly fixes the `use_tools: all` bug. Highest token-savings-per-line-of-code change. Saves 21K -> ~1.3K tokens.

**Design:**

When `select_functions()` returns more than N tools (threshold: 15-20, configurable), inject a `tool_search` meta-function instead of all schemas:

```
The tool_search function is available. Call it with a keyword to discover relevant tools.
Do NOT guess tool names — always search first.
```

The compact index returned by `tool_search` uses numbered natural language (works across all providers, per Architect review):

```
Available tools matching "file":
1. fs_cat - Read file contents (path)
2. fs_ls - List directory (path, recursive?)
3. fs_write - Write content to file (path, content)
Call the tool by name with its parameters.
```

On the next `call_react` iteration, inject full schemas for only the selected tools.

**Threshold logic:**
- Fewer than 15 tools: eager-load all schemas (no indirection overhead)
- 15+ tools: deferred loading via `tool_search`
- Leverage existing `mapping_tools` config for group-based loading: when the model searches "filesystem", load the entire `fs` group at once (~5 schemas, ~600 tokens) rather than individual tools

**Model capability gate (Architect):** For models below 14B parameters or models without native function calling, skip deferred loading — the accuracy loss from two-step discovery exceeds the token savings. Use `supports_function_calling` from `ModelData` as the gate.

**Changes:**
- Modify `select_functions()` in `src/config/mod.rs` to return `tool_search` when tool count exceeds threshold
- Add `tool_search` handler in `src/function.rs` that returns the compact index
- Modify `call_react` in `src/client/common.rs` to support dynamic tool sets between loop iterations (currently the tool list is fixed at loop start)

**Scope:** ~100-150 lines across `src/config/mod.rs`, `src/function.rs`, `src/client/common.rs`.

### 1D. Tool Use Examples in Role Definitions

**Rationale (Architect):** Biggest single accuracy improvement in the entire plan. Anthropic data: 72% -> 90% on complex parameter handling. The role system already supports `### INPUT: / ### OUTPUT:` example pairs via `parse_structure_prompt`. Formalizing this is trivial.

**Changes:**
- Add optional `examples:` field to role YAML frontmatter:
  ```yaml
  examples:
    - input: "Review this Python function for bugs"
      args: { language: python, severity: high }
    - input: "Quick style check on main.rs"
      args: { language: rust, severity: low }
  ```
- When aichat serves as MCP, append examples to the tool `description` field (the only cross-client compatible approach — MCP `Tool` type has no `examples` field)
- When aichat calls tools itself, include examples in the system prompt alongside tool schemas
- Document the existing `### INPUT: / ### OUTPUT:` pattern as the lightweight alternative for roles that don't use structured arguments

**Scope:** ~30-50 lines in `src/config/role.rs` and `src/mcp.rs`.

---

## Phase 2: Pipeline & Output Maturity

*Medium effort. Builds on Phase 0 prerequisites.*

### 2A. Pipeline-as-Role

**Rationale:** Expose multi-stage pipelines as single callable units. An agent calls one aichat command; internally it chains models; only the final result enters the caller's context.

**Design:** Define pipelines in role frontmatter, not as separate config:

```yaml
name: code-review
description: Multi-model code review pipeline
pipeline:
  - role: extract-diff
    model: deepseek-chat
  - role: review
    model: claude-sonnet-4-6
  - role: format-feedback
    model: gpt-4o-mini
```

When aichat loads this role, it generates a `FunctionDeclaration` from the role's `name`, `description`, and `input_schema` (if any). The role is callable from CLI, MCP, and function calling — same interface as any other role.

**Prerequisites (from Phase 0):**
- 0B: Pipeline stages must support tool calling
- 0C: Pipeline config isolation (no shared-state mutation)

**Additional requirements:**
- Add `name`, `description`, `parameters` fields to `PipelineDef` struct
- Generate `FunctionDeclaration` entries from pipeline definitions, wire into `select_functions()`
- Error propagation: if stage 2/4 fails, return a structured error result to the caller (not `bail!()`)
- Error messages must include stage index: `"pipeline stage 2/4 (role 'analyzer'): Model output is not valid JSON"`

**Scope:** ~150-200 lines across `src/pipe.rs`, `src/config/role.rs`, `src/function.rs`.

### 2B. Compact Output Modifier

**Rationale:** The original TOON format proposal was killed (no model trained on it, will produce malformed output across providers). Instead, add a `compact` output modifier that instructs the LLM to be maximally terse.

**Design:**
- Add `Compact` variant to `OutputFormat` enum in `src/cli.rs`
- `system_prompt_suffix` for `compact`: `"Respond with minimal tokens. Use short keys, abbreviations, and omit optional fields. No formatting, no explanations."`
- `-o compact` is a prompt modifier for LLM output only (no effect on metadata commands like `--list-*`)
- This is distinct from `-o json` (which is both a prompt modifier AND a structural format)

**What NOT to build:** A novel serialization format (TOON). If users need token-efficient structured output, `-o json` with short keys instructed via the role prompt achieves 20-40% savings without a new format. Every model handles this reliably.

**Scope:** ~20 lines in `src/cli.rs`.

---

## Phase 3: MCP Consumption

*High effort. Requires separate design doc. May belong in a separate binary.*

### 3A. Design Spike: MCP Client Architecture

Before implementation, answer these questions in a design doc:

1. **Process lifecycle:** MCP stdio servers are long-lived. Spawning `npx server-github` for a single `--list` costs 500ms-2s startup. Options:
   - Persistent daemon that keeps MCP connections alive
   - Caching layer that stores `tools/list` results with TTL
   - Accept the latency for one-shot CLI use

2. **Rust MCP client:** Does `rmcp` support client mode? If not, what library? What's the dependency cost? (Constraint: "significant increase in number of dependencies" requires approval.)

3. **Schema-to-CLI mapping:** MCP tool schemas are JSON Schema. Nested objects, arrays, `oneOf`/`anyOf` don't map cleanly to CLI flags. Options:
   - True CLI flags (limited to flat schemas, ergonomic)
   - JSON argument passthrough (universal, not ergonomic)
   - Hybrid: flat args as flags, complex args as `--json '{...}'`

4. **Authentication:** MCP servers need tokens (`GITHUB_TOKEN`, etc.). Options:
   - Environment variable passthrough (implicit)
   - Config file (`mcp-sources.yaml`) with auth fields
   - `--auth-header` flag (mcp2cli approach)

5. **One tool per job:** Does this belong in aichat, or in a standalone `mcp2cli`-style binary that pipes output into aichat? The UX reviewer argues the latter is more Unix-native. The counter-argument is that aichat's role system adds value (you can wrap an MCP tool in a role with a better prompt).

### 3B. Read-Only Spike

If the design doc resolves in favor of building in aichat:

**Phase 3B.1:** `--mcp-server <CMD> --list-tools` — discovery only, no execution. Validates the architecture without process lifecycle complexity.

**Phase 3B.2:** `--mcp-server <CMD> call <tool> [args]` — full execution with caching and auth.

**CLI design (UX):**
```bash
aichat --mcp-server "npx server-github" --list-tools
aichat --mcp-server "npx server-github" --list-tools -o json
aichat --mcp-server "npx server-github" call create-issue --title "Bug"
```

`--mcp` (existing: serve as MCP server) and `--mcp-server` (new: connect to MCP server as client) are distinct flags with opposite semantics. No overloading.

**Integration with tool dispatch:** MCP-consumed tools must merge into the `FunctionDeclaration` system so that `select_functions` and `use_tools` work uniformly. Two classes of tools with different dispatch paths is a maintenance hazard.

**Config-based sources (eventual):**
```yaml
tool_sources:
  - type: mcp
    command: ["npx", "server-github"]
    env: { GITHUB_TOKEN: "${GITHUB_TOKEN}" }
  - type: mcp
    endpoint: "http://localhost:8808"
  - type: local
    path: "~/.config/aichat/functions"  # backward compat
```

**Scope:** Phase 3A (design doc): 1-2 days. Phase 3B.1 (discovery): ~200 lines. Phase 3B.2 (execution + caching): ~500+ lines, new dependency.

---

## Phase 4: MCP Server Optimization

*Depends on Phase 1C (Deferred Tool Loading) for the internal pattern.*

### 4A. Lazy Role Discovery via MCP

**Problem:** When aichat serves as MCP (`--mcp`), it exposes all tools via `list_tools`. For large tool sets, this is expensive for the consuming agent.

**Design:** Advertise a `discover_roles` meta-tool that returns compact role summaries. Only inject full tool schemas when the agent requests a specific role.

**Protocol constraint:** MCP's `tools/list` is specified to return all tools. Lazy loading requires:
1. Return only `discover_roles` (and a few always-loaded tools) from `list_tools`
2. When the agent calls `discover_roles` and selects a role, dynamically add tools via `notifications/tools/list_changed`
3. Fallback for clients that don't support `list_changed`: detect during `initialize` handshake and fall back to eager loading

**Model capability gate:** Only enable for agents powered by models that handle two-step discovery well (Claude 3.5+, GPT-4+, Gemini 1.5+). For sub-14B models, send all schemas.

**Scope:** ~80-120 lines in `src/mcp.rs`. Requires `rmcp` to support `notifications/tools/list_changed`.

---

## Cross-Cutting Concerns

### Error Handling Strategy

Every phase must define error behavior. The codebase uses `bail!()` extensively, which terminates the process. For an agent calling aichat as a tool, a crash means a missing tool result.

**Rule:** Features callable by agents (MCP tools, pipeline-as-role, deferred tool loading) must return structured error results, not crash. Structured errors include:
- Error category (tool_not_found, schema_validation, model_error, timeout)
- Human-readable message
- Stage context for pipelines (`"stage 2/4 (role 'analyzer')"`)

### Caching Strategy

| Data | Cache Duration | Invalidation |
|---|---|---|
| `--list-roles` output | Session lifetime | File modification time on role YAML |
| `tool_search` results | Within `call_react` loop | Not cached across sessions |
| MCP `tools/list` (Phase 3) | Configurable TTL (default 1hr) | `--refresh` flag or TTL expiry |
| MCP server process (Phase 3) | Connection lifetime | Explicit shutdown or idle timeout |

### Multi-Provider Testing

Features that involve model behavior (1C deferred loading, 1D examples, 2A pipelines) must be tested against at minimum:
- A frontier model (Claude Sonnet/Opus or GPT-4o)
- A mid-tier model (Gemini Flash or Deepseek)
- A local model (Ollama with 7-14B parameter model)

The deferred loading threshold (15 tools) and model capability gate (`supports_function_calling`) should be validated empirically, not just theoretically.

---

## Implementation Order (Linear)

```
Phase 0 (Prerequisites)
  0A. Tool count warning                    ~10-20 lines
  0B. Pipeline tool-calling support         ~50-100 lines
  0C. Pipeline config isolation             ~20-30 lines

Phase 1 (Token Efficiency)
  1A. -o json for --list-* and --info       ~40-60 lines
  1B. Role description metadata             ~15-20 lines
  1C. Deferred tool loading                 ~100-150 lines
  1D. Tool use examples in roles            ~30-50 lines

Phase 2 (Pipeline & Output)
  2A. Pipeline-as-Role                      ~150-200 lines
  2B. Compact output modifier               ~20 lines

Phase 3 (MCP Consumption)
  3A. Design doc                            document
  3B. Read-only spike, then full impl       ~200-500+ lines

Phase 4 (MCP Server Optimization)
  4A. Lazy role discovery via MCP           ~80-120 lines
```

**Total estimated new code (Phases 0-2):** ~450-650 lines
**Total estimated new code (Phase 3):** ~500+ lines + new dependency
**Total estimated new code (Phase 4):** ~80-120 lines

---

## What to Preserve

- **exec + stdout + exit code as the tool contract** — Unix IPC. Do not replace with a protocol.
- **argc comment-driven schemas** — authoring format stays. All output formats derive from it.
- **`bin/` wrappers for CLI use** — `aichat -r summarize < input.txt` keeps working.
- **Language-agnostic tools** — bash/js/py support is a strength.
- **MCP as outward-facing facade only** — tools never speak MCP. The runtime translates.
- **`-o` as the single output control axis** — no `--machine-readable`, no `--agent-mode`. One flag.

## What Was Killed

| Proposal | Reason |
|---|---|
| TOON output format | No model trained on it. Will produce malformed output across providers. Existing `-o json/tsv` + short-key JSON via role prompts achieves 80% of the savings with 100% reliability. |
| `-r <role> --describe` flag | Duplicates existing `--info`. Introduces a flag that changes behavior of another flag. |
| `--mcp` overloading | Opposite semantics (serve vs consume) behind same flag. Use `--mcp-server <CMD>` for consumption. |
| MCP `discover_roles` as Tier 1 | Protocol dependency (`list_changed` support), model capability requirements, and no evidence agents will use two-step discovery without prompting. Deferred to Phase 4 after internal deferred loading proves the pattern. |
