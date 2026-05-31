# Anti-Roadmap: What NOT to Build

Proposals considered and rejected, with reasoning. Linked from [`ROADMAP.md`](../ROADMAP.md).

| Proposal | Reason | Source |
|---|---|---|
| LiteLLM as dependency | Python runtime conflicts with single-binary constraint. Already works via `openai-compatible` client. | Epic 2 |
| Semantic caching with vector DB | Exact-match cache (Phase 10B) covers the high-value case. Semantic dedup can be a pipeline role. | ML App Engineer |
| Multi-agent orchestration framework | Over-engineering. Agent-as-tool + pipelines + macros compose to cover every topology. | Epic 5 |
| Token-exact counting (tiktoken) | Only covers OpenAI tokenizers. Budget allocation needs order-of-magnitude, not exact precision. | Epic 2 |
| Knowledge graph with entity extraction | Requires LLM calls per chunk during indexing. Violates cost-conscious constraint. | Epic 4 |
| Visual pipeline designer GUI | Violates "no desktop UI" constraint. Roles are YAML files; text editor is the authoring tool. | Epic 3 |
| Event bus / message passing between agents | Wrong abstraction for single-shot CLI. Agent-as-tool IS the communication channel. | Epic 5 |
| Full-blown package registry for roles | Premature. `--fork-role` + git + `extends` covers sharing. Registry adds platform burden. | UX Designer |
| Real-time file watching daemon | CLI tools are invocation-based. Use git hooks, cron, or shell loops. | AI Architect |
| Confidence scoring on LLM output | Research problem, not engineering. No reliable way without another LLM call. | Epic 2 |
| `model_policy` cost-aware routing (Phase 10 follow-on) | Routing belongs in pipelines via `switch:`/`when:` (Phase 21), not as a separate runtime knob. | Epic 2 |
