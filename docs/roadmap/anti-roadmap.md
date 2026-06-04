# Anti-Roadmap: What NOT to Build

Proposals considered and rejected, with reasoning. Linked from [`../ROADMAP.md`](../ROADMAP.md).

## Runtime & features

| Proposal                                               | Reason                                                                                                | Source          | Rule Strength |
|--------------------------------------------------------|-------------------------------------------------------------------------------------------------------|-----------------|---------------|
| LiteLLM as dependency                                  | Python runtime conflicts with single-binary constraint. Already works via `openai-compatible` client. | Epic 2          | Weak          |
| Semantic caching with vector DB                        | Forked out to the **astrophage** peer repo (record/replay/cache substrate); see [integrated-architecture/SPEC-astrophage.md](../architecture/integrated-architecture/SPEC-astrophage.md) | ML App Engineer | Strong        |
| Multi-agent orchestration framework                    | Over-engineering. Agent-as-tool + pipelines + macros compose to cover every topology.                 | Epic 5 / 10     | Strong        |
| Token-exact counting (tiktoken)                        | Only covers OpenAI tokenizers. Budget allocation needs order-of-magnitude, not exact precision.       | Epic 2          | Moderate      |
| Knowledge graph with entity extraction                 | Requires LLM calls per chunk during indexing. Violates cost-conscious constraint.                     | Epic 4          | Strong        |
| Visual pipeline designer GUI                           | Violates "no desktop UI" constraint. Roles are YAML files; text editor is the authoring tool.         | Epic 3          | Strong        |
| Event bus / message passing between agents             | Wrong abstraction for single-shot CLI. Agent-as-tool IS the communication channel.                    | Epic 5          | Strong        |
| Full-blown package registry for roles                  | Premature. `--fork-role` + git + `extends` covers sharing. Registry adds platform burden.             | UX Designer     | Strong        |
| Real-time file watching daemon                         | CLI tools are invocation-based. Use git hooks, cron, or shell loops.                                   | AI Architect    | Strong        |
| Confidence scoring on LLM output                       | Research problem, not engineering. No reliable way without another LLM call.                          | Epic 2          | Strong        |
| `model_policy` cost-aware routing (Phase 10 follow-on) | Routing belongs in pipelines via `switch:`/`when:` (Phase 21), not as a separate runtime knob.        | Epic 2          | Strong        |
| Merging Role and Agent into one struct                 | The `to_role()` bridge works; llm-functions is a separate authoring contract. Agent identity is directory-based. | Epic 10  | Strong        |

## Cross-repo boundaries (the four-repo split)

These keep the integrated system's seams clean. Crossing them re-couples repos whose value is
their independence.

| Proposal                                               | Reason                                                                                                | Source          | Rule Strength |
|--------------------------------------------------------|-------------------------------------------------------------------------------------------------------|-----------------|---------------|
| astrophage reverse-depending on a consumer (aichat/brief/harness) | Breaks the runtime-agnostic value. The only inbound coupling is `base_url` + the `X-Eridian-Session-Id` header. | [SPEC-astrophage §8](../architecture/integrated-architecture/SPEC-astrophage.md) | Strong |
| Pushing structure-aware or knowledge keys across the astrophage seam | Re-imports runtime-awareness into the one component whose value is being runtime-agnostic. The `StageCache (role,model,input)` key, `cache_control` (L3), and `FactId` stay in aichat. | [SPEC-003 §0](../analysis/caching/SPEC-003-cache-substrate.md) | Strong |
| brief gaining runtime / network code (`tokio`/`reqwest`) | brief is format-first: it declares and emits, it never executes. The cassette-binding seam is format-only, forever. | [SPEC-astrophage §8](../architecture/integrated-architecture/SPEC-astrophage.md) | Strong |
| Caching tool execution through astrophage              | astrophage caches the **wire** (LLM responses) only. Deterministic tool stdout is replayed aichat-side from the keystone trace, keyed `(tool_name, args_hash)`. | [SPEC-004 §llm-functions](../analysis/caching/SPEC-004-ecosystem-surfaces.md) | Strong |
| A parallel telemetry model alongside the trace         | Every projection (accounting, OTel, cassette events) is *derived from* the SPEC-001 keystone trace — never a second source of truth. | [ADR-0001](../analysis/caching/ADR-0001-trace-as-keystone.md) | Strong |
| Editing brief / llm-functions from the aichat repo     | Companion changes are **documented here, applied there**. Cross-repo docs link by GitHub URL, never local path. | [integrated-architecture/README](../architecture/integrated-architecture/README.md) | Strong |
