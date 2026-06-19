# Phase 28 — Agent Composability : Overview — Epic 10 (Entity Evolution)

**Status:** Done — 28A/28B/28C shipped (2026-06-15) · **Owner:** aichat ↔ llm-functions · **Horizon:** Next

> **Goal.** Make agents first-class **composable** units: callable as tools, with a configurable
> react loop and macro output-chaining. Agents are defined in
> [llm-functions](https://github.com/jikanter/personal-llm-functions), so the composability
> contract is inherently cross-repo.

## Sub-phases

| Item | Description | Status |
|---|---|---|
| 28A | **Agent-as-tool** — agents callable via `ToolCall::eval()` dispatch, with a recursion depth limit | **Done** |
| 28B | **Configurable react loop** — `react_max_steps:` in frontmatter, `finish` synthetic tool | **Done** |
| 28C | **Macro output chaining** — `%%` variable resolves to the previous step's output | **Done** |

### Implementation notes (shipped 2026-06-15)

- **28A — agent-as-tool.** `ToolCall::eval()` gains a dispatch branch (`function.rs`):
  `check_agent` resolves a tool name to a known agent (via `list_agents()`), and a real
  function of the same name always wins the collision (`is_agent_tool`). `eval_agent` runs the
  agent as a delegated sub-agent in a **cloned config** with its own context window — the
  parent's messages are never passed, only the call's `input` — at an incremented
  `agent_depth`, bounded by `react_max_depth` (config field, default `DEFAULT_REACT_MAX_DEPTH`
  = 3). Exceeding the cap returns a `[TOOL_ERROR]` without a model call. `select_functions`
  (`config/mod.rs`) emits a `FunctionDeclaration::agent_as_tool` (flagged `agent: true`, single
  `input` param) for any agent named in `use_tools`, so the model can see and call it.
- **28B — configurable react loop.** `react_max_steps:` role/agent frontmatter
  (`Option<usize>`, `config/role.rs`); `call_react` uses it for the loop cap, falling back to
  `MAX_REACT_STEPS` (10). The synthetic `finish` tool (`FunctionDeclaration::finish`) is
  injected by `maybe_inject_finish` **only when `react_max_steps` is set** (so default tool
  turns are unchanged — token-conscious); `ToolCall::eval` echoes its `summary`, and
  `call_react` detects the call via `finish_summary` to terminate cleanly and append the final
  answer.
- **28C — macro output chaining.** `%%` substitution extracted to the pure
  `substitute_prev_output` helper (`config/mod.rs`) and locked with unit tests (dot-command
  skip, empty-prev no-op, multi-occurrence). Behavior was already live; this phase adds the
  test coverage it lacked.
- **Verified.** New unit tests across `function.rs` (finish + agent-as-tool declarations,
  `maybe_inject_finish`, `agent_depth_exceeded`, `is_agent_tool`), `config/role.rs`
  (`react_max_steps` parse/export), `config/mod.rs` (`substitute_prev_output`,
  `select_functions` agent injection via a fixture), and `client/common.rs` (`finish_summary`).
  Full suite: **842 + 6 + 197 pass**. Bats `tests/integration/phase-28-agent-composability.sh`
  drives the `--dry-run` surface for 28A/28B config resolution.
- **Deferred (Phase 43 harness).** Live delegation e2e (the model actually calling an
  agent-as-tool and the sub-agent's `call_react` running) needs a model — same deferral as the
  42E provider-event e2e. The "extend `tool_search` to index agents for discovery" enhancement
  is also deferred; agents are discoverable today via explicit `use_tools`.

## Cross-repo seams

Agent definitions (`functions.json`, `_instructions`) live in **llm-functions**; aichat owns the
dispatch and react loop. Agent-as-tool composes with pipelines and macros — the topology aichat
deliberately does **not** build a multi-agent orchestration framework for (see
[`anti-roadmap.md`](anti-roadmap.md)).

## Dependencies & detail

- **Feeds:** Phase 29 (agent dynamism), Phase 49 (agent memory federation).
- **Full design:** [`phase-28-agent-composability.md`](phase-28-agent-composability.md).
