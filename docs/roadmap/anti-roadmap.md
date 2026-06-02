# Anti-Roadmap: What NOT to Build

Proposals considered and rejected, with reasoning. Linked from [`ROADMAP.md`](../ROADMAP.md).

| Proposal                                               | Reason                                                                                                | Source          | Rule Strength |
|--------------------------------------------------------|-------------------------------------------------------------------------------------------------------|-----------------|---------------|
| LiteLLM as dependency                                  | Python runtime conflicts with single-binary constraint. Already works via `openai-compatible` client. | Epic 2          | Weak          |
| Semantic caching with vector DB                        | Forked out to the **astrophage** peer repo (record/replay/cache substrate); see [integrated-architecture/SPEC-astrophage.md](../architecture/integrated-architecture/SPEC-astrophage.md) | ML App Engineer | Strong        |
| Multi-agent orchestration framework                    | Over-engineering. Agent-as-tool + pipelines + macros compose to cover every topology.                 | Epic 5          | Strong        |
| Token-exact counting (tiktoken)                        | Only covers OpenAI tokenizers. Budget allocation needs order-of-magnitude, not exact precision.       | Epic 2          | Moderate      |
| Knowledge graph with entity extraction                 | Requires LLM calls per chunk during indexing. Violates cost-conscious constraint.                     | Epic 4          | Strong        |
| Visual pipeline designer GUI                           | Violates "no desktop UI" constraint. Roles are YAML files; text editor is the authoring tool.         | Epic 3          | Strong        |
| Event bus / message passing between agents             | Wrong abstraction for single-shot CLI. Agent-as-tool IS the communication channel.                    | Epic 5          | Strong        |
| Full-blown package registry for roles                  | Premature. `--fork-role` + git + `extends` covers sharing. Registry adds platform burden.             | UX Designer     | Strong        |
| Real-time file watching daemon                         | CLI tools are invocation-based. Use git hooks, cron, or shell loops.                                  | AI Architect    | Strong        |
| Confidence scoring on LLM output                       | Research problem, not engineering. No reliable way without another LLM call.                          | Epic 2          | Strong        |
| `model_policy` cost-aware routing (Phase 10 follow-on) | Routing belongs in pipelines via `switch:`/`when:` (Phase 21), not as a separate runtime knob.        | Epic 2          | Strong        |
