# Agents

Agents are specialized personas with their own configuration, instructions, and tools. They are more powerful than simple roles because they can maintain their own variables, reference documents, and use specific models or settings.

For the foundational model of Agents, Roles, Prompts, and Macros, see [the Entity model](../architecture/entity-model.md) — these are presets over one Entity substrate, distinguished by their backing and the facet families they own.

## File Location

Each agent has its own directory in:
`<aichat-config-dir>/agents/<agent-name>/`

Inside this directory, two main files define the agent:
1. **`index.yaml`** (or `agent.yaml`): The core definition of the agent.
2. **`config.yaml`**: The user-specific configuration for the agent.

## Agent Definition (`index.yaml`)

This file defines the agent's identity and capabilities.

```yaml
name: coder
description: A specialized software engineer agent
version: 1.0.0
instructions: |
  You are an expert software engineer.
  Use the provided context to solve the user's problem.
dynamic_instructions: false
variables:
  - name: language
    default: rust
conversation_starters:
  - "Help me refactor this function"
  - "Explain this codebase"
documents:
  - "docs/**/*.md"
```

### Fields
- **`name`**: The display name of the agent.
- **`description`**: A brief description shown in the agent list.
- **`instructions`**: The system prompt for the agent.
- **`dynamic_instructions`**: If `true`, instructions can be updated during the session.
- **`variables`**: Custom variables the agent can use.
- **`conversation_starters`**: Suggested prompts shown when starting the agent.
- **`documents`**: Glob patterns for documents the agent should index or reference.

## Agent Configuration (`config.yaml`)

This file allows overriding settings for the agent, similar to the global `config.yaml`.

```yaml
model: openai:gpt-4o
temperature: 0.2
use_tools: fs,web_search
variables:
  language: python
```

### Fields
- **`model`**: The LLM to use for this agent.
- **`temperature`**, **`top_p`**: Sampling parameters.
- **`use_tools`**: Tools available to the agent (e.g., `fs`, `web_search`, `mcp`).
- **`agent_prelude`**: A session to use when starting the agent.
- **`variables`**: Override default values for agent variables.

## Agent Variables

Agents can define variables that can be interpolated in their instructions or used during execution.
- They can have default values.
- They can be set via the agent's `config.yaml` or interactively when starting the agent.

## Usage

Start an agent in the REPL:
```bash
.agent coder
```

Or via CLI:
```bash
aichat --agent coder "How do I use traits?"
```

## Agents as tools (composability)

An agent can be called as a **tool** by a role or by another agent. List the
agent's name in `use_tools`, and the model can delegate a self-contained task to
it:

```yaml
---
react_max_steps: 6
use_tools: code-reviewer
---
You triage pull requests. Delegate code review to the `code-reviewer` agent.
```

When the model calls `code-reviewer`, the sub-agent runs in its **own context
window** — the parent's conversation is never passed, only the `input` argument
you give it. This token isolation is the cost advantage over stuffing everything
into one monolithic prompt. The sub-agent returns its final answer as the tool
result.

**Recursion is bounded.** Agents calling agents are capped at `react_max_depth`
(default `3`), set in `config.yaml`:

```yaml
react_max_depth: 3
```

Past the cap, a further delegation returns a tool error instead of recursing, so
a delegation cycle cannot run away.

A real function always wins a name collision: if a tool and an agent share a
name, the function is called — agent-as-tool never hijacks an existing tool.

## Bounding the agent loop

Two role/agent frontmatter knobs shape the tool-calling (ReAct) loop:

- **`react_max_steps:`** caps how many tool-calling iterations a single turn may
  run (default `10`). Lower it for cheap, bounded agents:

  ```yaml
  ---
  react_max_steps: 4
  ---
  ```

- **`finish` tool.** When `react_max_steps` is set, a synthetic `finish` tool is
  exposed so the model can end the turn explicitly with its final answer (passed
  as `summary`) instead of relying on an empty tool-call turn. It is *not* added
  to ordinary tool turns, so default behavior and token usage are unchanged.

See also: [Macros](./macros.md), [Knowledge](./knowledge.md)


