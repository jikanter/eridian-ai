# Tool Efficiency Analysis: CLI vs MCP vs Advanced Tool Use

**Date:** 2026-03-10

## Sources

- [mcp2cli](https://github.com/knowsuchagency/mcp2cli) — Runtime CLI generation from MCP servers and OpenAPI specs
- [CLI vs MCP](https://kanyilmaz.me/2026/02/23/cli-vs-mcp.html) — Token efficiency comparison by Kanyilmaz
- [Advanced Tool Use](https://www.anthropic.com/engineering/advanced-tool-use) — Anthropic engineering blog on Tool Search, Programmatic Tool Calling, and Tool Use Examples

## The Problem All Three Sources Identify

Every agent framework today pays a **tool tax**: full JSON schemas for all available tools are injected into the system prompt on every conversation turn, regardless of whether the tools are actually used.

| Source | Measurement |
|--------|-------------|
| mcp2cli | 6 MCP servers x 14 tools = ~15,540 tokens at session start |
| CLI vs MCP blog | MCP session start ~15,540 tokens vs CLI ~300 tokens (98% savings) |
| Anthropic | 55K+ tokens for tool definitions, reduced to ~500 with Tool Search (85% savings) |

This is not a theoretical concern. aichat already encounters it: `use_tools: all` with the full llm-functions set produces 86K+ characters (~21K tokens) in the system prompt, causing apparent hangs with local models. See [use-tools-all-performance.md](./2026-03-10-use-tools-all-performance.md).

## Three Escape Hatches

### 1. mcp2cli: Convert MCP to CLI at Runtime

**Approach:** Dynamically generate argparse CLIs from MCP server schemas. Agents discover tools via `--list` (~16 tokens/tool) and get usage via `--help` (~80-200 tokens/tool) only when needed.

**Token savings by scenario:**

| Scenario | Turns | Native MCP Cost | mcp2cli Cost | Saved |
|----------|-------|-----------------|-------------|-------|
| 30-tool task manager | 15 | 54,525 | 2,309 | 96% |
| 80-tool multi-server | 20 | 193,360 | 3,897 | 98% |
| 120-tool platform | 25 | 362,350 | 5,181 | 99% |

**Key design decisions:**
- No code generation — schemas read at runtime, new endpoints appear immediately
- Provider-agnostic — works with any LLM since it's invoked as a shell command
- TOON format — token-efficient encoding reduces output 40-60% for structured data

### 2. Anthropic Tool Search: Deferred Schema Loading

**Approach:** Mark tools with `defer_loading: true`. Claude discovers tools via a built-in search tool, loading full schemas only when needed.

**Results:**
- Opus 4 accuracy: 49% -> 74%
- Opus 4.5 accuracy: 79.5% -> 88.1%
- Initial token cost: 55K -> ~500 (85% reduction)

**Limitation:** When a tool IS fetched, the full JSON schema (~121 tokens/tool) is still injected. CLI discovery (`--help`) is cheaper per-tool than Tool Search schema injection.

### 3. Anthropic Programmatic Tool Calling: Code as Orchestrator

**Approach:** Instead of individual tool calls via API, Claude writes Python code to orchestrate multiple tools. Only final results enter context — intermediate data stays in the sandbox.

**Results:**
- Token consumption: 43,588 -> 27,297 (37% reduction)
- Eliminates 19+ inference passes in multi-step workflows
- Knowledge retrieval: 25.6% -> 28.5%

**Key insight:** This is conceptually identical to aichat's declarative pipeline system — chain stages, only return the final output. aichat does it with role composition and `pipe_to`; Anthropic does it with sandboxed Python.

### 4. Tool Use Examples (Supplementary)

Providing concrete usage examples alongside JSON schemas improved accuracy from 72% to 90% on complex parameter handling. This is orthogonal to token efficiency but directly relevant to aichat's role system.

## What This Means for aichat

### aichat is already the efficient interface

The CLI vs MCP blog's entire thesis is that CLIs beat MCP for token efficiency. When an agent calls:

```bash
aichat -r summarize < input.txt
```

It pays ~30 tokens for the command, not 121+ for a JSON schema per turn. aichat's roles, with their `--help`-style discoverability and Unix composability, are already the efficient interface these articles argue for.

### aichat's pipelines ARE programmatic tool calling

Anthropic's pattern of "write code to orchestrate tools, only return final results" is what aichat's pipeline system does: `extract:deepseek -> review:claude -> format:gpt4o`, with only the final output entering the caller's context. The difference is aichat does it declaratively in YAML rather than imperatively in Python.

### The MCP facade pattern is validated

All three sources agree: MCP has value as an external interface but is expensive as an internal protocol. This directly validates the architecture in [llm-functions-interaction.mdx](./2026-03-10-llm-functions-interaction.mdx): tools stay as Unix executables, the runtime projects protocol facades outward.

## Proposed Features

### Tier 1: Leverage Existing Strengths (Low Effort, High Impact)

#### A. Token-Efficient Role Discovery

Inspired by mcp2cli's `--list` / `--help` pattern:

```bash
aichat --list-roles          # compact one-liner per role (~16 tokens each)
aichat -r <role> --describe  # short usage, not full schema (~80-200 tokens)
```

When aichat serves as MCP server, advertise a `discover_roles` tool that returns compact summaries rather than injecting all role schemas upfront. This makes aichat the cheapest tool an agent can reach for.

**Cost comparison for an agent with access to 30 aichat roles:**

| Method | Per-turn cost | 20-turn session |
|--------|-------------|-----------------|
| Native MCP (all schemas) | ~3,630 tokens | 72,600 |
| aichat --list-roles + on-demand --describe | ~67 + ~180 per used role | ~1,940 (for 5 unique roles) |

#### B. Pipeline-as-Tool

Expose multi-stage pipelines as single callable units. An agent calls one aichat command; internally it chains models. Only the final result enters the agent's context.

```yaml
# roles/code-review.yaml
name: code-review
pipeline:
  - role: extract-diff
    model: deepseek-chat
  - role: review
    model: claude-sonnet-4-6
  - role: format-feedback
    model: gpt-4o-mini
```

The agent sees one tool. Internally three models run. This is Anthropic's Programmatic Tool Calling pattern without requiring a Python sandbox — aichat's Rust runtime handles it.

### Tier 2: Absorb the mcp2cli Pattern (Medium Effort)

#### C. MCP Consumption as CLI Subcommands

aichat already serves MCP. The mcp2cli insight is that *consuming* MCP servers and re-exposing them as CLI is valuable:

```bash
aichat --mcp "npx server-github" --list
aichat --mcp "npx server-github" create-issue --title "Bug" --body "..."
```

This makes aichat the universal adapter: agents talk to aichat (cheap CLI), aichat talks to MCP servers (rich protocol). Token savings compound.

**Architecture:**

```
Agent (Claude Code, Cursor)
    |
    calls: aichat --mcp <server> <tool> --args  (~30 tokens)
    |
aichat (Rust runtime)
    |
    connects: MCP stdio/HTTP to server
    translates: CLI args -> JSON-RPC tools/call
    returns: stdout text (not JSON-RPC envelope)
```

#### D. Token-Optimized Output Format

mcp2cli's TOON format reduces output tokens 40-60% for structured data. aichat's `-o` flag already supports JSON/CSV/TSV. Adding a compact format for agent consumption reinforces cost-conscious positioning:

```bash
aichat -r query-db -o compact "list active users"
# Returns token-efficient encoding instead of pretty JSON
```

### Tier 3: Adopt Anthropic's Advanced Patterns (Medium-High Effort)

#### E. Tool Use Examples in Role Definitions

Anthropic's data: accuracy 72% -> 90% with concrete examples. Add an `examples:` field to roles:

```yaml
name: code-review
examples:
  - input: "Review this Python function for bugs"
    args: { language: python, severity: high }
  - input: "Quick style check on main.rs"
    args: { language: rust, severity: low }
```

When aichat serves as MCP, include examples in tool definitions.

#### F. Deferred Tool Loading for aichat's Own Tool Use

When aichat itself calls tools via llm-functions, apply Tool Search internally: inject a compact tool list, only load full schemas for tools the model selects. This directly fixes the `use_tools: all` performance issue.

```
Current:  System prompt includes 84 tool schemas = 21K tokens
Proposed: System prompt includes tool index = ~1.3K tokens
          Model requests specific tools = ~300 tokens each, on demand
```

## Strategic Summary

```
              Token Tax Problem
                    |
    +---------------+---------------+
    |               |               |
 mcp2cli        Anthropic       CLI vs MCP
 (MCP->CLI)    (Tool Search +   (CLI wins on
              Programmatic)     efficiency)
    |               |               |
    +---------------+---------------+
                    |
              aichat's Play:
         "Cheapest tool an agent
          can reach for, that also
          orchestrates everything"
```

The sharpest move is **Tier 1A + 1B first**: role discovery + pipeline-as-tool. They require minimal new code, play to existing strengths, and directly validate the "make for AI" thesis with quantifiable token savings.

Tier 2C (MCP consumption as CLI) is the ambitious play that would make aichat a genuine category of one: the only tool that bridges both directions — consuming MCP efficiently and serving results cheaply.

## Relationship to Existing Analysis

This analysis provides **quantitative token-efficiency backing** for the architectural decisions in [llm-functions-interaction.mdx](./2026-03-10-llm-functions-interaction.mdx). That document argued against MCP as internal protocol on architectural grounds (process overhead, composition breakage, dependency inversion). These three sources add the economic argument: MCP tool injection costs 96-99% more tokens than CLI-based discovery. The two analyses are complementary and mutually reinforcing.
