# Phase 9: Schema Fidelity : Overview - Epic 2 
| Item | Description | Status |
|---|---|---|
| 9A | Provider-native structured output — OpenAI `response_format: json_schema` | -- |
| 9B | Provider-native structured output — Claude tool-use-as-schema | -- |
| 9C | Schema validation retry loop (inject error, re-prompt, configurable `schema_retries:`) | -- |
| 9D | Capability-aware pre-flight validation (model supports tools? vision? sufficient context?) | -- |

**Key design:** When `response_format` is active, suppress the system prompt schema suffix (~50-200 tokens saved per call). Schema retry short-circuits when native structured output guarantees conformance.

## [Epic Details](./phase-9-schema-fidelity.md)
