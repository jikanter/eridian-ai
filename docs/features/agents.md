# Agents

Agents are specialized personas with their own configuration, instructions, and tools. They are more powerful than simple roles because they can maintain their own variables, reference documents, and use specific models or settings.

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

See also: [Macros](./macros.md), [Knowledge](./knowledge.md)


