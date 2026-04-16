# Phase 9: Runtime Intelligence — Schema Fidelity

**Status:** Planned
**Epic:** 2 — Runtime Intelligence
**Design:** [epic-2.md](../analysis/epic-2.md)

---

> **[ADDED 2026-03-16]** Builds the deterministic intelligence layer between user input and LLM calls.
> Governing principle: every token sent to an LLM should be a token that only an LLM can process.
> Full design: [`docs/analysis/epic-2.md`](../analysis/epic-2.md)

| Item | Status | Notes |
|---|--------|---|
| 9A. Provider-native structured output (OpenAI `response_format`) | —      | When role has `output_schema` and model supports `response_format: json_schema`, inject it into the API request body. Suppresses system prompt schema suffix (saves ~50-200 tokens). New `supports_response_format_json_schema` boolean on `ModelData`. |
| 9B. Provider-native structured output (Claude tool-use-as-schema) | —      | For Claude models: define synthetic tool whose `input_schema` IS the `output_schema`, force via `tool_choice`, extract args as output. Different API shape than 9A but same outcome. |
| 9C. Schema validation retry loop | —      | On `validate_schema("output", ...)` failure, inject validation error as new user message and retry (default: 1 retry). New `schema_retries:` role frontmatter field. Short-circuits when native structured output (9A/9B) is active. |
| 9D. Capability-aware pre-flight validation | Done   | Before API call: check `use_tools` vs `supports_function_calling`, images vs `supports_vision`, pipeline stage model availability and schema compatibility. Fails at config time, not at API time. Zero tokens. |

**Parallelization:** 9A, 9B, 9C, 9D are all independently implementable. 9A and 9B can run concurrently (different provider paths). 9C has a soft dependency on 9A/9B (should short-circuit when native mode active) but can be built first. 9D is fully independent.

**Key files:** `src/client/openai.rs` (9A), `src/client/claude.rs` (9B), `src/main.rs` + `src/pipe.rs` (9C), new `src/config/preflight.rs` (9D), `src/client/model.rs` + `models.yaml` (9A/9B).
