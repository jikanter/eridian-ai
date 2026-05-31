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
  - In **plain-text prompt steps**, `%%` is substituted inline with the previous step's AI output before the step runs. Example: `"Summarize this: %%"`.
  - In **dot commands** (steps starting with `.`), `%%` is left untouched so the REPL's own `%%` handling applies — e.g. `.file %%` attaches the previous reply as a document.
  - `%%` is only resolved after a step that produced AI output; earlier dot-only steps (e.g. `.role`, `.agent`) leave it unresolved and the token passes through as-is.
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

### Chaining Output Between Steps (Phase 28C)
Pass one step's AI output as the prompt for the next step using inline `%%`:
```yaml
# <aichat-config-dir>/macros/extract-and-summarize.yaml
variables:
  - name: url
steps:
  - .role text-extractor
  - .file {{url}} -- Extract the main content
  - .role summarizer
  - "Summarize the following in one paragraph:\n\n%%"
```
The final step's `%%` is replaced with the extractor's output before being sent to the summarizer role.

See also: [Agents](./agents.md), [Knowledge](./knowledge.md)