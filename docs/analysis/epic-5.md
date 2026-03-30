# Epic 5: Entity Evolution & Agent Dynamism

**Created:** 2026-03-16
**Status:** Planning
**Depends on:** Phase 7.5 (macro/agent config override), Phase 8 (observability)

---

## Motivation

AIChat has four entity types: Prompt, Role, Agent, and Macro. Roles are the most powerful declarative unit (20+ metadata fields, pipelines, schemas, inheritance). Agents add runtime capabilities (own tools, RAG, dynamic instructions, sessions) but lack Role features (schemas, pipelines, lifecycle hooks, MCP binding). Macros orchestrate imperatively but can't flow data between steps.

The critical gap is **agent composability**: agents cannot call other agents, cannot be used as pipeline stages, and cannot be invoked as tools. Complex tasks must be solved by a single monolithic agent with all tools loaded, violating the "one tool per job" principle.

The second gap is **agent dynamism**: the `call_react` loop is a flat ReAct cycle with no planning, no conditional branching, no memory across invocations, and no runtime tool discovery beyond the existing `tool_search` pattern.

This epic addresses both gaps while preserving AIChat's core strengths: declarative composition, token efficiency, and Unix-native simplicity. The strategy is to make agents composable within existing mechanisms (pipelines, tool dispatch, macros), not to build a framework.

### Competitive Context

| Capability | Claude Code | LangGraph | CrewAI | AIChat |
|---|---|---|---|---|
| Single-agent ReAct | Yes | Yes | Yes | Yes |
| Multi-agent delegation | No | Yes (StateGraph) | Yes (Crew) | **No → Epic 5** |
| Agent planning | Implicit | Yes (Plan-Execute) | Yes (goals) | **No → compose via pipeline** |
| Agent memory | No | Checkpointing | Short-term | **No → Epic 5** |
| Declarative pipelines | No | No (code-only) | No (code-only) | **Yes (unique)** |
| Schema-validated I/O | No | No | No | **Yes (unique)** |
| Token-efficient tools | No | No | No | **Yes (unique)** |
| Cost accounting | No | No | No | **Phase 8A (planned)** |

AIChat's strategy: don't compete on agent autonomy (LangGraph/CrewAI territory). Compete on **agent composability** — making agents first-class participants in the pipeline/tool/macro composition model that is already AIChat's strength.

---

## Feature 1: Agent-as-Tool

### Problem

Agents cannot be called by other agents, by pipeline stages, or by roles. An agent with `use_tools: web_search,execute_command` cannot add `use_tools: code-reviewer-agent`. Complex tasks that should be delegated to specialist agents must be solved monolithically.

### Solution

Make agents callable as tools through the existing `ToolCall::eval()` dispatch. When a tool name matches a known agent, init the agent, run `call_react`, return the text output as a `ToolResult`.

### Implementation

In `src/function.rs`, `ToolCall::eval()` (lines 306-349) already has three dispatch paths: `tool_search`, pipeline-role, and llm-function. Add a fourth:

```rust
// After check_pipeline_role (line 313-394):
if let Some(agent_name) = self.check_agent(config) {
    return self.eval_agent(config, &agent_name, abort_signal).await;
}
```

`eval_agent`:
1. Init Agent B via `Agent::init(config, &agent_name)`
2. Create Input from tool call arguments: `Input::from_str(config, &args["input"], Some(agent_role))`
3. Set up agent context (variables, functions, RAG)
4. Call `call_react(&mut input, client, abort_signal)` with Agent B's role and tools
5. Return output text as `ToolResult`

**Recursion prevention**: Thread a `depth: usize` parameter through `call_react`. Max depth = 3 (configurable via `react_max_depth:`). When exceeded, return an error ToolResult: "Agent delegation depth exceeded."

**Token isolation**: Each sub-agent gets its own context window. Agent A's messages are NOT passed to Agent B — only the tool call arguments. Agent B's system prompt + tools are its own context. This is the key cost advantage over monolithic prompts.

**Step budget**: Sub-agent steps count toward the parent's MAX_REACT_STEPS (shared budget) OR each agent gets its own budget (isolated budget). Isolated is safer for cost control. Configurable via `react_step_sharing: isolated | shared`.

**Discovery**: Extend `tool_search` to include agent names and descriptions in its searchable index. When an agent is discovered via `tool_search`, its declaration is injected into the active tool set.

### Files to Modify

| File | Change |
|---|---|
| `src/function.rs` | Add `check_agent()` + `eval_agent()` dispatch in `ToolCall::eval()`; extend `eval_tool_search()` to include agents |
| `src/client/common.rs` | Add `depth` parameter to `call_react()`; enforce max depth |
| `src/config/mod.rs` | Add `react_max_depth` config field |

### Effort

Medium. ~150 lines. The pattern follows existing `check_pipeline_role`/`eval_pipeline_role` exactly.

### Parallelization

Independent of Features 2-7. Foundation for Feature 6 (agent composition).

---

## Feature 2: ReactPolicy Trait (Configurable Agent Loop Behavior)

### Problem

`call_react` is a flat loop with one control mechanism: the retry budget (Phase 7C). There is no way to:
- Stop execution when cost exceeds a budget
- Switch to a different model after N failures
- Inject guidance when the agent is stuck
- Define custom termination conditions

Phase 9C (schema retry), Phase 10D (model fallback), and cost guards are all specialized versions of this general need.

### Solution

A `ReactPolicy` trait that injects deterministic checkpoints into the `call_react` loop. Zero-cost for happy-path execution. Policies compose.

### Implementation

**New trait** in `src/client/common.rs`:

```rust
pub trait ReactPolicy: Send + Sync {
    fn check(&self, context: &ReactContext) -> ReactAction;
}

pub enum ReactAction {
    Continue,
    InjectGuidance(String),    // Add message before next turn
    SwitchModel(String),       // Change model for remaining steps
    Stop(String),              // Halt with partial result + reason
}

pub struct ReactContext<'a> {
    pub step: usize,
    pub max_steps: usize,
    pub total_cost_usd: f64,
    pub total_input_tokens: usize,
    pub total_output_tokens: usize,
    pub consecutive_failures: usize,
    pub last_tool_results: &'a [ToolResult],
    pub elapsed_ms: u64,
}
```

**Built-in policies**:

```rust
// Cost guard: stop when budget exceeded
pub struct CostGuard { pub max_cost_usd: f64 }
impl ReactPolicy for CostGuard {
    fn check(&self, ctx: &ReactContext) -> ReactAction {
        if ctx.total_cost_usd > self.max_cost_usd {
            ReactAction::Stop(format!("Cost budget exceeded: ${:.4}", ctx.total_cost_usd))
        } else { ReactAction::Continue }
    }
}

// Stagnation detector: inject guidance after N consecutive failures
pub struct StagnationGuard { pub max_consecutive_failures: usize }

// Model escalation: switch to expensive model after failures
pub struct ModelEscalation { pub escalation_model: String, pub trigger_failures: usize }
```

**Integration**: In `call_react`, after `annotate_repeated_failures` (line 503-517), call `policy.check(&context)` and handle the action. Default policy is `Continue` always (zero overhead).

**Configuration**: Role/agent frontmatter `react_policy:` field:
```yaml
react_policy:
  max_cost: 0.50              # CostGuard
  stagnation_threshold: 3     # StagnationGuard
  escalation_model: claude:claude-sonnet-4-6  # ModelEscalation after 5 failures
```

### Files to Modify

| File | Change |
|---|---|
| `src/client/common.rs` | ReactPolicy trait; policy check in `call_react` loop; built-in policies |
| `src/config/role.rs` | Parse `react_policy:` from frontmatter |

### Effort

Medium. ~200 lines for trait + 3 policies + integration.

### Parallelization

Independent of all other features. The `call_react` insertion point is orthogonal to Feature 1's dispatch changes.

### Token Impact

Zero for happy-path. Policies are deterministic runtime checks, not LLM calls. CostGuard prevents runaway spending. StagnationGuard saves wasted turns.

---

## Feature 3: Agent Memory (JSONL Fact Store)

### Problem

Agents have no memory across invocations. Each session starts from the same instructions + tools. An agent that has processed 100 requests has learned nothing about which tools work for which queries, what the user prefers, or what common patterns exist.

### Solution

A per-agent JSONL memory file, automatically populated from trace data, readable by `_instructions` at session start. Zero LLM calls for writes. Zero API calls for reads.

### Implementation

**Memory file**: `<agent_data_dir>/memory.jsonl`

**Write path** — at the end of each `call_react` invocation, append a summary record:
```jsonl
{"ts":"2026-03-16T...","type":"invocation","turns":3,"tools_used":["web_search","fs_cat"],"tools_failed":["execute_command"],"cost_usd":0.012,"success":true}
```

This piggybacks on the existing `TraceEmitter::emit_summary` (trace.rs). The delta: also write to `memory.jsonl`. ~15 lines.

**Read path** — `_instructions` shell function reads memory at session start:
```bash
# In _instructions for an agent:
MEMORY="$LLM_AGENT_DATA_DIR/memory.jsonl"
if [ -f "$MEMORY" ]; then
    echo "## Learned Knowledge (last 50 invocations)"
    tail -50 "$MEMORY" | jq -r 'select(.type=="invocation") |
      "- Tools: \(.tools_used | join(", ")) | Failed: \(.tools_failed // [] | join(", ")) | Cost: $\(.cost_usd)"'
fi
```

This is a shell script, not Rust. The agent author decides what to extract from memory. AIChat only handles the write side.

**Memory record types** (extensible):
- `invocation` — per-invocation summary (automatic)
- `tool_outcome` — per-tool success/failure/latency (automatic)
- `correction` — user correction captured from session (future, manual)

**Convenience**: New `memory:` field in AgentConfig. When `memory: true`, automatically:
1. Set `LLM_AGENT_DATA_DIR` env var for `_instructions`
2. Enable trace-to-memory bridging in `call_react`
3. Add memory records to `_instructions` output if `dynamic_instructions: true`

### Files to Modify

| File | Change |
|---|---|
| `src/client/common.rs` | Append memory record at end of `call_react` |
| `src/config/agent.rs` | Add `memory:` field to AgentConfig; set `LLM_AGENT_DATA_DIR` env var |
| `src/utils/ledger.rs` | Reuse `append_run_log` pattern for memory file |

### Effort

Small. ~60 lines of Rust for the write path. The read path is shell scripts in agent `_instructions` — zero Rust.

### Token Impact

Write: zero (filesystem append). Read: ~50-200 tokens injected into system prompt from memory aggregation. This is a fixed cost that makes every subsequent invocation more informed.

---

## Feature 4: Unified Entity Resolution

### Problem

Users must choose between `-r` (role), `-a` (agent), `--macro` (macro) before they can invoke an entity. The roadmap notes: "A user who wants to create a reusable prompt that calls tools has to choose between a Role with use_tools, an Agent with functions.json, or a Macro that sets up a Role."

### Solution

Unify entity resolution under `-r`. The flag resolves against a combined namespace: roles first, then agents, then macros. Explicit `-a` and `--macro` remain as overrides for name collisions.

### Implementation

New function `Config::resolve_entity(name)`:
```rust
pub fn resolve_entity(&self, name: &str) -> Result<EntityRef> {
    // 1. Check roles directory
    if let Ok(role) = self.retrieve_role(name) {
        return Ok(EntityRef::Role(role));
    }
    // 2. Check agents
    if self.agent_names().contains(&name.to_string()) {
        return Ok(EntityRef::Agent(name.to_string()));
    }
    // 3. Check macros
    if self.macro_names().contains(&name.to_string()) {
        return Ok(EntityRef::Macro(name.to_string()));
    }
    bail!("Entity '{}' not found (checked roles, agents, macros)", name)
}
```

In `src/main.rs`, when `-r name` is specified, call `resolve_entity(name)` and dispatch accordingly.

**Backward compatibility**: `-a name` always resolves as agent. `--macro name` always resolves as macro. `-r name` uses the unified resolution. No breaking change.

### Files to Modify

| File | Change |
|---|---|
| `src/config/mod.rs` | Add `resolve_entity()` method |
| `src/main.rs` | Use `resolve_entity()` when `-r` flag is specified |

### Effort

Small. ~50 lines. Zero behavioral change for existing `-a` and `--macro` users.

---

## Feature 5: Configurable React Loop

### Problem

`MAX_REACT_STEPS = 10` is hardcoded (common.rs:435). Some tasks genuinely need more turns. Some should stop earlier. There is no agent-level control.

### Solution

Expose step limit and add an explicit "finish" tool for clean termination.

### Implementation

**Configurable step limit** — new frontmatter field:
```yaml
react_max_steps: 20  # default: 10
```

Read in `call_react` from the active role/agent config. Falls back to 10.

**Finish tool** — a synthetic tool that cleanly exits the loop:
```rust
FunctionDeclaration {
    name: "finish",
    description: "Signal that the task is complete. Call this when you have the final answer.",
    parameters: json!({"type": "object", "properties": {
        "result": {"type": "string", "description": "The final result"}
    }, "required": ["result"]}),
}
```

When `call_react` sees a `finish` tool call, it extracts `result` and returns it as the output, bypassing further iterations. This gives the LLM an explicit way to say "I'm done" instead of the implicit "stop requesting tools."

### Files to Modify

| File | Change |
|---|---|
| `src/client/common.rs` | Read `react_max_steps` from config; handle `finish` tool call |
| `src/config/role.rs` | Add `react_max_steps` to frontmatter |
| `src/function.rs` | Add `finish` to synthetic tool set (alongside `tool_search`) |

### Effort

Small. ~40 lines.

---

## Feature 6: Agent-in-Pipeline

### Problem

Pipeline stages can reference roles but not agents. A pipeline cannot include an agent stage that has its own tools and RAG.

### Solution

Allow pipeline stages to reference agent names. When `run_stage_inner` resolves a stage role, fall back to agent resolution.

### Implementation

In `pipe.rs:run_stage_inner()` (lines 112-186), the role resolution at line 119-122:
```rust
let role = config.read().retrieve_role(&stage.role_name)?;
```

Change to:
```rust
let role = config.read().retrieve_role(&stage.role_name)
    .or_else(|_| {
        // Try resolving as agent
        let agent = Agent::init(config, &stage.role_name)?;
        Ok(agent.to_role())
    })?;
```

The `to_role()` method already produces a complete Role with the agent's instructions, model, tools, and config. Pipeline stages that reference agents get the agent's full capabilities.

```yaml
pipeline:
  - role: extract              # regular role
  - role: triage-agent         # resolves to agent, gets agent's tools + RAG
  - role: format               # regular role
```

### Files to Modify

| File | Change |
|---|---|
| `src/pipe.rs` | Agent fallback in role resolution |
| `src/config/agent.rs` | Ensure `Agent::init()` works without REPL context |

### Effort

Small. ~30 lines. The `to_role()` bridge already exists.

---

## Feature 7: Agent MCP Binding

### Problem

Roles can declare `mcp_servers:` (Phase 6C) to auto-bind MCP tools. Agents cannot. An agent that needs MCP tools must wrap itself in a role, which is cumbersome and loses agent-specific features (own tools, RAG, dynamic instructions).

### Solution

Add `mcp_servers:` to AgentConfig. When an agent declares MCP servers, those tools are merged into the agent's function set.

### Implementation

In `src/config/agent.rs`, add to `AgentConfig`:
```rust
pub mcp_servers: Option<Vec<String>>,
```

In `Agent::to_role()` (line 347-363), after syncing model/temperature/tools, also sync `mcp_servers`:
```rust
if let Some(servers) = &self.config.mcp_servers {
    role.role_mcp_servers = Some(servers.clone());
}
```

The existing Phase 6C machinery in `Config::retrieve_role()` (lines 1106-1118) already handles `mcp_servers` → `use_tools` expansion. Since `to_role()` produces a Role that goes through `retrieve_role`, this Just Works.

### Files to Modify

| File | Change |
|---|---|
| `src/config/agent.rs` | Add `mcp_servers` to `AgentConfig`; sync in `to_role()` |

### Effort

Tiny. ~15 lines. Leverages existing Phase 6C infrastructure completely.

---

## Feature 8: Macro Output Chaining

### Problem

Macros can sequence REPL commands but cannot pass output between steps programmatically. Each step's output goes to stdout. A macro cannot say "take the output of step 1 and feed it to step 2."

### Solution

A `%%` variable in macros that resolves to the previous step's output (parallel to the existing `%%` for "last reply" in REPL).

### Implementation

In `macro_execute()` (`src/config/mod.rs`, lines 2869-2903), after each step that produces output, capture the output in a `prev_output` variable:

```rust
// After running each step:
if let Some(last_reply) = config.read().last_message.as_ref() {
    prev_output = last_reply.1.clone();  // (input, output) tuple
}

// Before interpolating the next step:
step_text = step_text.replace("%%", &prev_output);
```

The REPL already captures `last_message` (the last assistant response). The macro runner just needs to read it between steps.

```yaml
# macros/extract-and-summarize.yaml
variables:
  - name: url
steps:
  - ".role text-extractor"
  - ".file {{url}} -- Extract the main content"
  - ".role summarizer"
  - "Summarize this: %%"
```

### Files to Modify

| File | Change |
|---|---|
| `src/config/mod.rs` | Capture and interpolate `%%` between macro steps |

### Effort

Small. ~20 lines.

---

## Cross-Feature Dependency Graph

```
F1 (agent-as-tool) ─────────────── Foundation for F6
F2 (ReactPolicy) ───────────────── Independent
F3 (agent memory) ──────────────── Independent
F4 (unified resolution) ────────── Independent
F5 (configurable loop) ─────────── Independent
F6 (agent-in-pipeline) ── soft dep on F1 (for full benefit)
F7 (agent MCP binding) ─────────── Independent (tiny)
F8 (macro output chaining) ──────── Independent
```

**Maximum parallelism: 7 independent work streams** (F1, F2, F3, F4, F5, F7, F8). F6 can start in parallel but benefits from F1.

---

## What NOT to Build

| Proposal | Reason |
|---|---|
| Multi-agent orchestration framework (CrewAI-style) | Over-engineering. Agent-as-tool (F1) + pipelines + macros compose to cover every topology. |
| LLM-driven planning step | Costs tokens upfront. Compose via pipeline: plan-role → execute-role. |
| Merge Role and Agent into one struct | Premature. `to_role()` bridge works. llm-functions agent format is a separate authoring contract. |
| Give agents `extends`/`include`/`pipeline` | Agent identity is directory-based. Role inheritance doesn't map. Pipelines create two orchestration models. |
| Shared mutable state between agents | Concurrency hazard. Agents communicate through tool call/result (text in, text out). |
| Custom ReAct loop per agent | One `call_react` with pluggable ReactPolicy is strictly better than N custom loops. |
| Tool synthesis (LLM generates tools) | Unbounded cost. LLM calls per synthesized tool. |
| Agent event bus / message passing | Wrong abstraction for single-shot CLI. Agent-as-tool IS the communication channel. |
| Persistent agent processes | `call_react` is single-invocation. Long-running agents need different architecture. |

---

## Relationship to Existing Roadmap

| Epic 5 Feature | Existing Phase | Relationship |
|---|---|---|
| F1 (agent-as-tool) | Phase 2A (pipeline-as-role as tool) | **Extension** — same pattern, extended to agents |
| F2 (ReactPolicy) | Phase 7C (retry budget), Phase 9C (schema retry), Phase 10D (model fallback) | **Generalization** — ReactPolicy subsumes all three as specialized policies |
| F3 (agent memory) | None | **New** — leverages Phase 8 trace infrastructure |
| F4 (unified resolution) | None | **New** — UX improvement only, no architectural change |
| F5 (configurable loop) | None | **New** — expose existing hardcoded constant |
| F6 (agent-in-pipeline) | Phase 2A (pipeline-as-role) | **Extension** — `to_role()` bridge enables agent stages |
| F7 (agent MCP binding) | Phase 6C (role MCP binding) | **Extension** — same `mcp_servers:` pattern on AgentConfig |
| F8 (macro output chaining) | None | **New** — reuses existing `last_message` infrastructure |
