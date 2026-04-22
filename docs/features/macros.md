# Macros

Macros are predefined sequences of REPL commands that automate repetitive workflows. They support customizable variables and run in isolated execution contexts.

## File Location

Macros are defined as YAML files in:
`<aichat-config-dir>/macros/*.yaml`

The filename (excluding the `.yaml` extension) is used as the macro name. For example, `commit.yaml` becomes the `.macro commit` command.

## File Format

A macro definition consists of `variables` and `steps`.

```yaml
variables:
  - name: agent
    default: coder
  - name: query
    rest: true

steps:
  - .agent {{agent}}
  - "{{query}}"
  - .file %%
```

### `variables` (Optional)
An array of variable definitions:
- **`name`** (Required): The variable name, referenced as `{{name}}` in steps.
- **`default`** (Optional): A default value if none is provided at execution.
- **`rest`** (Optional, Boolean): If `true`, collects all remaining arguments into this variable. Only valid for the last variable.

### `steps` (Required)
An array of strings, each representing a valid REPL command.
- Macros support the `%%` token to refer to the output of the previous step.
- Commands start with a dot (e.g., `.agent`, `.file`, `.role`).
- Plain text is treated as a prompt to the current AI model.

## Execution Context

When a macro is executed via `.macro <name> [args]`:
- It runs in an **isolated context**.
- It does **not** inherit or modify the current session's role, agent, or RAG state.
- Once the macro finishes, your REPL state remains exactly as it was before execution.

## Special Tokens

- **`{{var}}`**: Substituted with the value of the variable `var`.
- **`%%`**: Refers to the output of the most recent AI response within the macro execution.

## Examples

### Expert Prompt Generator
```yaml
# <aichat-config-dir>/macros/expert.yaml
variables:
  - name: topic
    rest: true
steps:
  - "I want you to act as an expert prompt engineer. Help me craft a prompt for: {{topic}}"
  - .info %%
```

See also: [Agents](./agents.md), [Knowledge](./knowledge.md)