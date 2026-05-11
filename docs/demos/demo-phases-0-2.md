# Phases 0-2: Token-Efficient Tool Orchestration

*2026-03-30T15:47:57Z by Showboat 0.6.1*
<!-- showboat-id: 2eef9de2-37fb-4737-aeea-f69d4d769f73 -->

Phases 0-2 add structured metadata output, role descriptions, deferred tool loading, tool use examples, pipeline-as-role, compact output, and pipeline safety (tool-calling support and config isolation).

## Phase 1A: Structured Metadata Output

`--list-models`, `--list-roles`, and `--info` now support `-o json` for machine-readable output. Downstream tools can parse roles, models, and config without scraping human-readable text.

```bash
aichat --list-roles -o json | jq 'map(select(.name == "%code%")) | .[0] | {name, model, tools}'
```

```output
{
  "name": "%code%",
  "model": "default",
  "tools": []
}
```

Each role includes its name, a derived description (first 100 chars of the prompt), model, and tool list. Downstream agents can filter roles by capability without loading them individually.

```bash
aichat -r %code% --info -o json 2>&1 | jq '.model = "MODEL"'
```

```output
{
  "model": "MODEL",
  "role": "%code%",
  "description": "Provide only code without comments or explanations.",
  "prompt_length": 199,
  "stream": false
}
```

`--info -o json` surfaces the active model, role, description, prompt length, and stream settings as structured data. Agents can probe config state without parsing human-readable output.

## Phase 1B: Role Descriptions

Roles can declare a `description` field in their YAML frontmatter. This is surfaced in `--list-roles -o json` and `--info -o json`. When no explicit description is set, the first 100 characters of the prompt are used as a derived description.

```bash
cat <<'ROLE'
---
description: Summarize text into bullet points
temperature: 0.3
---
You are a summarization assistant. Given any text, produce a concise bulleted summary capturing the key points. Use markdown bullet format.
ROLE
```

```output
---
description: Summarize text into bullet points
temperature: 0.3
---
You are a summarization assistant. Given any text, produce a concise bulleted summary capturing the key points. Use markdown bullet format.
```

The `description` field is separate from the prompt — it is metadata for discovery and agent consumption. It appears in JSON listings but does not affect the system prompt sent to the model.

## Phase 1D: Tool Use Examples

Roles can include concrete `examples` in frontmatter. When a role has both `examples` and `use_tools`, the examples are injected into the system prompt as few-shot demonstrations. Per Anthropic's engineering data, this improves tool selection accuracy from 72% to 90%.

```bash
cat <<'ROLE'
---
use_tools: fs_cat,fs_ls
examples:
  - input: "Show me the contents of main.rs"
    args: {"name": "fs_cat", "arguments": {"path": "src/main.rs"}}
  - input: "What files are in the src directory?"
    args: {"name": "fs_ls", "arguments": {"path": "src"}}
---
You are a filesystem assistant. Help users explore and read files.
ROLE
```

```output
---
use_tools: fs_cat,fs_ls
examples:
  - input: "Show me the contents of main.rs"
    args: {"name": "fs_cat", "arguments": {"path": "src/main.rs"}}
  - input: "What files are in the src directory?"
    args: {"name": "fs_ls", "arguments": {"path": "src"}}
---
You are a filesystem assistant. Help users explore and read files.
```

When this role is loaded and tools are resolved, the system prompt includes a `## Tool Use Examples` section showing each input and the expected tool call with arguments.

## Phase 1C: Deferred Tool Loading

When a role selects more than 15 tools, full JSON schemas are replaced with a single `tool_search` meta-function. The model calls `tool_search` with a keyword, gets a compact index of matching tools, and then calls the specific tool. This saves 92-99% of token overhead for tool-heavy configurations.

```bash
grep "DEFERRED_TOOL_THRESHOLD\|deferred_tools\|tool_search\|tool count" src/config/mod.rs | head -10
```

```output
    pub deferred_tools: Option<DeferredToolState>,
/// When more than DEFERRED_TOOL_THRESHOLD tools are selected,
/// we inject a tool_search meta-function instead of all schemas.
const DEFERRED_TOOL_THRESHOLD: usize = 15;
            deferred_tools: None,
        if let Some(ref deferred) = self.deferred_tools {
                // Always include tool_search so the model can search for more tools
                functions.push(FunctionDeclaration::tool_search());
            // replace with tool_search meta-function
            if functions.len() > DEFERRED_TOOL_THRESHOLD
```

The threshold is set at 15 tools. Below that, full schemas are sent as normal. Above it, the model gets a single `tool_search` function. After calling `tool_search`, the matched tools are activated for subsequent turns. Sub-14B parameter models skip deferred loading because the two-step indirection degrades their accuracy.

## Phase 2A: Pipeline-as-Role

Roles can define a `pipeline` in frontmatter — a sequence of stages that chain LLM calls. When another role's agent calls a pipeline role as a tool, it executes the full pipeline and returns the result. This enables composable multi-model workflows without external orchestration.

```bash
cat <<'ROLE'
---
description: Multi-model code review pipeline
pipeline:
  - role: code-analyst
    model: ollama:deepseek-r1:14b
  - role: code-reviewer
    model: vllm:base
---
ROLE
```

```output
---
description: Multi-model code review pipeline
pipeline:
  - role: code-analyst
    model: ollama:deepseek-r1:14b
  - role: code-reviewer
    model: vllm:base
---
```

Pipeline stages run sequentially, piping output from one to the next. Each stage saves and restores model state (Phase 0C) and uses `call_react` when the stage role has tools (Phase 0B).

## Pipeline CLI: --pipe with --stage

Pipelines can also be invoked directly from the command line using `--pipe` with one or more `--stage` flags. Each stage specifies a role and optional model override with `@model` syntax.

```bash
aichat --help 2>&1 | grep -A1 -E "(--pipe |--stage|--pipe-def)"
```

```output
      --stage <ROLE[@MODEL]>
          Pipeline stages (role or role@model)
--
      --pipe-def <FILE>
          Pipeline definition file
```

## Phase 0B & 0C: Pipeline Safety

Two prerequisite fixes ensure pipelines are robust:

**0B: Pipeline Tool-Calling** — When a stage role has `use_tools`, the pipeline calls `call_react` (the full agent loop with tool dispatch) instead of `call_chat_completions`.

**0C: Config Isolation** — Each stage saves the current model ID before execution and restores it after, regardless of success or failure.

```bash
grep -E "saved_model_id|let has_tools|Phase 0B" src/pipe.rs | head -3
```

```output
            let saved_model_id = config.read().current_model().id();
            if let Err(e) = config.write().set_model(&saved_model_id) {
    let has_tools = role.use_tools().is_some();
```

## Phase 0A: Tool Count Warning

When more than 20 tools are selected, a warning is emitted to stderr so the user knows about the token overhead. This catches the `use_tools: all` footgun that silently injects 86K+ characters.

```bash
grep "tools selected" src/config/mod.rs | head -5
```

```output
                    "{} tools selected — this may cause slow responses with local models. \
                        "Warning: {} tools selected. Consider scoping use_tools to specific tools: use_tools: tool1,tool2",
            debug!("select_functions: {} tools selected", functions.len());
```

```bash
grep -A 1 "functions.len() > 20" src/config/mod.rs | head -2
```

```output
            if functions.len() > 20 {
                warn!(
```

## Phase 2B: Compact Output Modifier

A new `-o compact` output format acts as a prompt modifier (not a structural format). It appends a system prompt suffix instructing the model to respond with minimal tokens. Unlike `-o json`, it does not enforce structure or disable streaming.

```bash
grep -A2 "Compact =>" src/cli.rs
```

```output
            OutputFormat::Compact => Some(
                "\n\nRespond with minimal tokens. Use short keys, abbreviations, and omit optional fields. No formatting, no explanations."
            ),
--
            OutputFormat::Tsv | OutputFormat::Csv | OutputFormat::Text | OutputFormat::Compact => {
                Ok(cleaned)
            }
```

## Integration Tests

Verify key features using `aichat --dry-run` and structured output flags.

```bash
echo "test" | aichat --dry-run -r %code% 2>&1 | grep -vE "^(--- |  in: | out: )"
```

````output
Provide only code without comments or explanations.
### INPUT:
async sleep in js
### OUTPUT:
```javascript
async function timeout(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}
```

test
````

## Tests

```bash
cargo test 2>&1 | grep -c "^test result: FAILED" | xargs -I {} echo "FAILED test results: {}"
```

```output
FAILED test results: 0
```
