# Phase 1: Token Efficiency Foundations

**Status:** Done
**Commit(s):** `dde1078`

---

| Item | Status | Commit | Notes |
|---|---|---|---|
| 1A. `-o json` for `--list-*` and `--info` | Done | `dde1078` | Structured metadata for agent consumption |
| 1B. Role `description` field | Done | `dde1078` | Frontmatter field, falls back to first sentence of prompt |
| 1C. Deferred tool loading (`tool_search`) | Done | `dde1078` | Threshold at 15 tools. Compact index, dynamic schema injection |
| 1D. Tool use examples in role frontmatter | Done | `dde1078` | `examples:` field with `input` + `args` pairs |

Phase 1C directly implements the pattern documented in the [tool efficiency analysis](../analysis/2026-03-10-tool-analysis.md): Anthropic's Tool Search reduced initial token cost from 55K to ~500 (85% reduction). aichat's implementation applies the same principle to llm-functions, dropping the `use_tools: all` penalty from ~21K tokens to ~1.3K. See [use_tools: all performance analysis](../analysis/2026-03-10-use-tools-all-performance.md).
