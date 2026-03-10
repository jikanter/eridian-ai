# Debugging: `use_tools: all` Causes Apparent Hang with Local Models

**Date:** 2026-03-10

## Symptom

```bash
aichat-dev --no-stream --role 'stats' '1,2,3,4,5'
```

Appears to hang indefinitely. No output, no error. Ctrl-C required to exit.

## Root Cause

The `stats` role was defined with `use_tools: all`:

```yaml
---
use_tools: all
---
Execute a python function to determine the mean, median, and mode of the following numbers
```

`use_tools: all` causes `select_functions()` (`src/config/mod.rs:1676`) to include **every** function declaration from `functions.json` in the API request. In this environment, that was **148 tools** totaling **~86K characters (~21K tokens)** of JSON tool definitions injected into the prompt.

The model (`Qwen3-Coder-30B-A3B-Instruct-3bit` via vLLM on Apple Silicon) was not hanging — it was processing an enormous prompt very slowly. The same model responded instantly to the same prompt with a single tool definition.

## Diagnosis Path

| Test | Result |
|------|--------|
| `curl localhost:8000/v1/models` | vLLM healthy, model loaded |
| `curl` chat completion with 1 tool | Responded in ~2s with correct tool call |
| `aichat-dev 'say hello'` (no role) | Responded normally (no tools attached) |
| `aichat-dev --role stats '1,2,3,4,5'` | Hung — 148 tools in prompt |
| Changed `use_tools: all` → `use_tools: execute_py_code` | Responded in ~10s with correct tool call |

## The Math

```
148 functions × ~580 chars avg = ~86K chars ≈ 21K tokens (tool definitions alone)
+ system prompt + user message ≈ 22K+ input tokens
```

On a 3-bit quantized 30B model running on CPU/GPU hybrid (Apple Silicon), processing 22K input tokens can take minutes. The model wasn't stuck — the prefill phase was just slow.

## Fix

Scope `use_tools` to only the tools the role actually needs:

```yaml
---
use_tools: execute_py_code
---
Execute a python function to determine the mean, median, and mode of the following numbers
```

Multiple tools can be comma-separated: `use_tools: execute_py_code,get_current_time`

## Broader Implications

### For role authors

- **Never use `use_tools: all` with local models** unless the model is fast enough to handle the full tool set. On cloud APIs (GPT-4, Claude) with fast prefill, `all` is usually fine. On local quantized models, it creates a severe performance cliff.
- **Scope tools to what the role needs.** A stats role only needs `execute_py_code`. A research role might need `web_search,fetch_url_via_curl`. Listing specific tools is both faster and produces better model behavior (less tool confusion).

### For aichat development

Potential improvements to surface this problem earlier:

1. **Warn when tool count is high.** If `select_functions()` returns more than N tools (e.g., 20), log a warning. Most roles need 1-5 tools.
2. **Log tool count in debug output.** `AICHAT_LOG_LEVEL=debug` should show how many tools were selected and their estimated token cost.
3. **Timeout with actionable error.** Instead of hanging forever, a configurable timeout on the API call could produce: `"Request timed out. 148 tools were included (est. 21K tokens). Consider scoping use_tools in your role."`

## Related Code

- Tool selection: `src/config/mod.rs:1676` (`select_functions`)
- Tool declarations loaded from: `functions.json` (symlinked from llm-functions)
- Role definition: `~/.config/aichat/roles/stats.md` (or `~/Library/Application Support/aichat/roles/`)
- Function execution: `src/function.rs:21` (`eval_tool_calls`)
