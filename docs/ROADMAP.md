# AIChat Roadmap

**Last updated:** 2026-04-07
**Last updated:** 2026-04-07
**317 tests passing (144 unit + 173 compatibility), 0 failures**

---

## Vision

AIChat is becoming **"make for AI workflows"**: a token-efficient, Unix-native CLI that lets agents and humans compose multi-model pipelines, consume external tools via MCP, and expose roles as callable infrastructure. The REPL remains a debug/interactive surface, not the primary interface.

Roles are the fundamental unit of composition. This roadmap evolves roles from static prompt templates into **typed, addressable, evaluable building blocks** that compose across machines, execution models, and cost budgets.

### Governing Constraints

- **Cost-conscious above all.** Every feature must justify its token budget.
- **One tool per job.** Unix composition over monolithic features.
- **No new languages, no desktop UI, no breaking argc/llm-functions** without explicit approval.

---

## Epic Overview

| Epic | Scope | Phases | Status | Origin |
|---|---|---|---|---|
| 1 | Core Platform | 0-8 | **Done** | -- |
| 2 | Runtime Intelligence | 9-11 | Planned | [epic-2.md](./analysis/epic-2.md) |
| 3 | Composition UX | 12-13 | **New** | Theme 6: UX Designer |
| 4 | Typed Ports & Capabilities | 14-15 | **New** | Theme 1: All four experts |
| 5 | Server Pipeline Engine | 16-18 | Planned | [epic-3.md](./analysis/epic-3.md) |
| 6 | Universal Addressing | 19-20 | **New** | Theme 5: AI Architect + ML Engineer |
| 7 | DAG Execution | 21-22 | **New** | Theme 4: AI Architect |
| 8 | Feedback Loop | 23-24 | **New** | Theme 2: ML Engineer + ML App Engineer |
| 9 | RAG Evolution | 25-27 | Planned | [epic-4.md](./analysis/epic-4.md) |
| 10 | Entity Evolution | 28-29 | Planned | [epic-5.md](./analysis/epic-5.md) |

Architecture reference: [architecture.md](architecture/architecture.md)

---

## Pre-Roadmap Features

| Feature | Commit | Reference |
|---|---|---|
| Model-aware variables and conditionals | `589b9b1` | [demo](./demos/demo-model-aware.md) |
| Composable roles (`extends`, `include`) | `cdb5d9e` | [demo](./demos/demo-composable-roles.md) |
| Schema-aware stdin/stdout (`input_schema`, `output_schema`) | `b57668d` | [demo](./demos/demo.md) |
| Role parameters (`-v key=value`) and env bridging (`{{$VAR}}`) | `1dbab28` | [analysis](./analysis/2026-03-02-role-parameters.md) |
| Output format flag (`-o json/jsonl/tsv/csv/text`) | `e72d776` | [analysis](./analysis/2026-03-06-output-format.md) |
| `__INPUT__` de-hoisting in extended roles | `9ce9755` | [demo](./demos/demo-dehoist-input.md) |
| Macro system | `30dae5c` | [docs](./macros.md) |
| Semantic exit codes (11 codes, error chain walking) | `c7d4e7e` | `src/utils/exit_code.rs` |

---

## Epic 1: Core Platform

### Phase 0: Prerequisites -- [detail](./roadmap/phase-0-prerequisites.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 0A | Tool count warning (>20 tools) | Done | `dde1078` |
| 0B | Pipeline tool-calling (`call_react` in `pipe.rs`) | Done | `dde1078` |
| 0C | Pipeline config isolation | Done | `dde1078` |

### Phase 1: Token Efficiency Foundations -- [detail](./roadmap/phase-1-token-efficiency.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 1A | `-o json` for `--list-*` and `--info` | Done | `dde1078` |
| 1B | Role `description` field | Done | `dde1078` |
| 1C | Deferred tool loading (`tool_search`) | Done | `dde1078` |
| 1D | Tool use examples in role frontmatter | Done | `dde1078` |

### Phase 2: Pipeline & Output Maturity -- [detail](./roadmap/phase-2-pipeline-output.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 2A | Pipeline-as-Role | Done | `dde1078` |
| 2B | Compact output modifier (`-o compact`) | Done | `dde1078` |

### Phase 3: MCP Consumption -- [detail](./roadmap/phase-3-mcp-consumption.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 3A | Design document | Done | -- |
| 3B | Discovery (`--mcp-server <CMD> --list-tools`) | Done | `7b31472` |
| 3C | Execution (`--call <TOOL> --json '{...}'`) | Done | `7b31472` |
| 3D | Config-based servers (`mcp_servers:` in config.yaml) | Done | `7b31472` |

### Phase 4: Error Handling & Schema Fidelity -- [detail](./roadmap/phase-4-error-handling.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 4A | Stop silent data loss | Done | `1e5b7d2` |
| 4B | Structured error types (`AichatError`) | Done | `1e5b7d2` |
| 4C | Structured error output (`-o json`) | Done | `1e5b7d2` |
| 4D | Fix `JsonSchema` lossiness | Done | `fec32e4` |
| 4E | Pipeline stage tracebacks | Done | `fe60f03` |

### Phase 5: Remote MCP & Token-Efficient Discovery -- [detail](./roadmap/phase-5-remote-mcp.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 5A | Remote MCP servers (HTTP/SSE) | Done | `7f500b8` |
| 5B | Lazy role discovery via MCP | Done | `7f500b8` |

### Phase 6: Metadata Framework Enhancements -- [detail](./roadmap/phase-6-metadata-framework.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 6A | Shell-injective variables (`{ shell: "git diff --cached" }`) | Done | `30669d7` |
| 6B | Lifecycle hooks (`pipe_to`, `save_to`) | Done | `30669d7` |
| 6C | Unified resource binding (`mcp_servers:` per-role) | Done | `30669d7` |

### Phase 7: Error Messages & Tool Execution -- [detail](./roadmap/phase-7-error-messages.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 7A | Stderr capture + tool error diagnostics | Done | `d125ee0` |
| 7B | Pre-flight checks + typed error variants | Done | `d125ee0` |
| 7C | Retry budget + loop detection | Done | `d125ee0` |
| 7C1 | Per-tool timeout | Done | `d125ee0` |
| 7D1 | Async tool execution | Done | `d125ee0` |
| 7D2 | Concurrent tool execution | Done | `d125ee0` |

### Phase 7.5: Macro & Agent Config Override -- [detail](./roadmap/phase-7.5-set-expansion.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 7.5A | Extend `.set` with role-level fields | Done | `fe60f03` |
| 7.5B | Macro frontmatter assembly | Done | `fe60f03` |
| 7.5C | Agent `.set` parity | Done | `fe60f03` |
| 7.5D | Guard rails (schema meta-validation) | Done | `fe60f03` |

### Phase 8: Data Processing & Observability -- [detail](./roadmap/phase-8-data-observability.md)

| Item | Description | Status | Commit |
|---|---|---|---|
| 8A1 | Run log & cost accounting | Done | `fe60f03` |
| 8A2 | Pipeline trace metadata | Done | `fe60f03` |
| 8B | Batch record processing (`--each`) | Done | `fe60f03` |
| 8C | Record field templating (`{{.field}}`) | Done | `fe60f03` |
| 8D | Headless RAG | Done | `fe60f03` |
| 8F | Interaction trace (`--trace`) | Done | `fe60f03` |
| 8G | Trace JSONL (`AICHAT_TRACE=1`) | Done | `fe60f03` |

---

## Epic 2: Runtime Intelligence -- [design](./analysis/epic-2.md)

> Every token sent to an LLM should be a token that only an LLM can process. If deterministic logic can resolve a question, it should never reach the model.

### Phase 9: Schema Fidelity

| Item | Description | Status |
|---|---|---|
| 9A | Provider-native structured output — OpenAI `response_format: json_schema` | -- |
| 9B | Provider-native structured output — Claude tool-use-as-schema | -- |
| 9C | Schema validation retry loop (inject error, re-prompt, configurable `schema_retries:`) | -- |
| 9D | Capability-aware pre-flight validation (model supports tools? vision? sufficient context?) | -- |

**Key design:** When `response_format` is active, suppress the system prompt schema suffix (~50-200 tokens saved per call). Schema retry short-circuits when native structured output guarantees conformance.

### Phase 10: Resilience & Cost-Aware Routing

*Merges existing Phase 10 resilience with Theme 3 (cost-aware routing).*

| Item | Description | Status |
|---|---|---|
| 10A | API-level retry with exponential backoff (`src/client/retry.rs`) | -- |
| 10B | Pipeline stage output cache (content-addressable, `sha256(role+model+input)`, configurable TTL) | -- |
| 10C | Pipeline stage retry (configurable `stage_retries:`, retryable error classification) | -- |
| 10D | Cost-aware model routing (`model_policy:` on roles) | -- |
| 10E | Pipeline model fallback (`fallback_models:` chain on stage failure) | -- |

**10D Design — Cost-Aware Model Routing:**

Static `model:` fields leave massive savings on the table. A `model_policy:` field enables deterministic routing without an LLM call for the routing decision:

```yaml
# Deterministic routing by input characteristics
model_policy:
  default: deepseek:deepseek-chat
  rules:
    - when: { token_count_gt: 2000 }
      model: claude:claude-sonnet-4-6
    - when: { schema_failures_gt: 1 }
      model: openai:gpt-4o
  fallback: openai:gpt-4o
```

**Implementation:** In `Input::create_client()`, before model resolution, evaluate `model_policy.rules` against the input. Rules are deterministic predicates — `token_count_gt`, `has_images`, `has_tools`, `schema_failures_gt` — evaluated via `estimate_token_length()` and input metadata. No LLM call needed.

For `--each` batch processing, this alone can cut costs 40-60% on mixed-complexity workloads by routing simple inputs to cheap models.

**Files:** `src/config/role.rs` (add `model_policy`), `src/config/input.rs` (evaluate rules in `create_client()`), `src/config/mod.rs` (parse policy config).

### Phase 11: Context Budget & Budget Propagation

*Merges existing Phase 11 with pipeline-level budget propagation.*

| Item | Description | Status |
|---|---|---|
| 11A | Context budget allocator core (`src/context_budget.rs`) | -- |
| 11B | BM25-ranked file inclusion (score files against query, fill budget by relevance) | -- |
| 11C | Budget-aware RAG (dynamic `top_k = remaining_budget / avg_chunk_tokens`) | -- |
| 11D | Pipeline budget propagation (`budget:` field, per-stage allocation) | -- |

**11D Design — Pipeline Budget Propagation:**

No framework currently propagates token budgets through a composition graph. A 4-stage pipeline doesn't know its total budget.

```yaml
pipeline:
  budget_usd: 0.05         # total pipeline budget
  stages:
    - role: extract          # gets proportional share
    - role: review
      budget_weight: 2.0     # gets 2x share
    - role: format
```

**Implementation:** In `pipe.rs:run()`, compute per-stage budgets from total. Pass budget to each `run_stage_inner()`. When a stage approaches budget, signal truncation rather than failure. Reuses Phase 11A's `ContextBudget` allocator per stage.

This turns "cost-conscious" from a cultural norm into an architectural guarantee.

**Files:** `src/pipe.rs` (budget allocation + enforcement), `src/config/role.rs` (pipeline `budget_usd:`, stage `budget_weight:`).

---

## Epic 3: Composition UX (NEW)

> Apply token-consciousness to human attention, not just LLM calls. Every role invocation should make the user slightly more aware of what the system can do.

*Source: Theme 6 — UX Designer analysis. Focuses on reducing the cost of understanding before the cost of execution.*

### Phase 12: Discoverability & Previews

| Item | Description | Status |
|---|---|---|
| 12A | Resolved prompt preview (`--dry-run` with `extends`/`include` expanded, variables interpolated) | -- |
| 12B | Pipeline visualization in `--dry-run` (text diagram: `extract -> validate -> summarize (3 stages)`) | -- |
| 12C | Port signatures in `--list-roles` (`--verbose` shows `in: raw-text, out: json{summary, entities}`) | -- |
| 12D | Composition summary after `.role <name>` in REPL (`extends: base, includes: [safety], tools: 3`) | -- |

**12A/12B Design — Resolved Preview:**

`--dry-run` already exists but shows the raw prompt. Enhance it to render the *fully resolved* state:

```bash
$ aichat -r code-reviewer --dry-run "review this"

--- Resolved Role: code-reviewer ---
  extends: base-analyst
  includes: [json-output, safety-checks]
  model: claude:claude-sonnet-4-6
  tools: 3 (web_search, fs_cat, execute_command)
  input_schema: { type: "string" }
  output_schema: { properties: { issues: [...], severity: [...] } }

--- Pipeline ---
  1. extract (deepseek:deepseek-chat)
  2. review (claude:claude-sonnet-4-6)
  3. format (deepseek:deepseek-chat)

--- Assembled Prompt (847 tokens) ---
  [system] You are a code review assistant...
  [user] review this

--- Estimated Cost ---
  $0.003 (3 stages, ~2400 tokens total)
```

Zero tokens spent. This is the "terraform plan" moment — the most beloved command in that ecosystem because it eliminates the fear of "what will this actually do?"

**Files:** `src/main.rs` (enhance `--dry-run` path), `src/config/role.rs` (add `resolve_full()` that expands extends/include/variables).

**12C Design — Port Signatures:**

```bash
$ aichat --list-roles --verbose
  code-reviewer    in: text      out: json{issues, severity}    3 tools   extends: base-analyst
  summarizer       in: text      out: text                      0 tools
  classifier       in: json{...} out: json{label, confidence}   0 tools   pipeline: 2 stages
```

Derived from existing `input_schema`/`output_schema`. A one-line human-readable summary of JSON Schema top-level properties. When no schema is defined, shows `in: any, out: text`.

**Files:** `src/config/role.rs` (add `port_signature()` method), `src/main.rs` or `src/config/mod.rs` (render in list output).

### Phase 13: Authoring & Teaching

| Item | Description | Status |
|---|---|---|
| 13A | `--fork-role <source> <new-name>` (creates pre-populated `extends:` file) | -- |
| 13B | Schema mismatch errors as teaching moments (side-by-side diff with suggestion) | -- |
| 13C | Built-in guardrail role examples (PII detection, prompt injection, topic restriction) | -- |
| 13D | `--explain-role <name>` (human-readable description of what a role does and how it composes) | -- |

**13A Design — Fork Role:**

```bash
$ aichat --fork-role base-analyst my-analyst

Created roles/my-analyst.md:
  ---
  extends: base-analyst
  # model: claude:claude-sonnet-4-6     # override parent's model
  # temperature: 0.7                     # override parent's temperature
  # output_schema:                       # override parent's schema
  ---
  # Add your prompt additions here. Parent prompt is inherited.
```

This is the pattern that made Terraform modules composable in practice. The fork command turns a 5-minute file-editing task into a 5-second command, and teaches the user that `extends` exists.

**Files:** `src/cli.rs` (add `--fork-role` flag), `src/main.rs` (generate the file with commented-out parent fields).

**13B Design — Error Teaching:**

Current schema mismatch on pipeline failure:
```
error: pipeline stage 2 output schema validation failed
  /required/language: missing required property
```

After Phase 13B:
```
error: pipeline stage 2 output schema validation failed

  Stage 1 produced:     { "text": "...", "summary": "..." }
  Stage 2 expects:      { "content": "...", "language": "..." }

  Missing fields: content, language
  Extra fields: text, summary

  hint: Did you mean to add a transform role between stages 1 and 2?
        Try: aichat --fork-role json-transform my-adapter
```

**Files:** `src/config/role.rs` (enhance `validate_schema` error formatting), `src/pipe.rs` (pass both schemas to error formatter).

**13C Design — Guardrail Role Examples:**

Guardrails are not a new feature — they are a role authoring pattern. Ship 3 example roles in `assets/roles/` that demonstrate the pattern:

```yaml
# assets/roles/guardrail-pii.md
---
name: guardrail-pii
description: Detect and redact PII from text
model: deepseek:deepseek-chat    # cheap model sufficient
output_schema:
  type: object
  properties:
    safe: { type: boolean }
    redacted: { type: string }
    findings: { type: array, items: { type: string } }
  required: [safe, redacted]
---
Scan the following text for personally identifiable information (PII).
If PII is found, set safe=false and return the redacted version.
__INPUT__
```

Users compose guardrails into pipelines via existing mechanisms:
```yaml
pipeline:
  - role: guardrail-pii
  - role: my-actual-task
  - role: guardrail-topic
```

**Files:** `assets/roles/guardrail-pii.md`, `assets/roles/guardrail-injection.md`, `assets/roles/guardrail-topic.md`.

---

## Epic 4: Typed Ports & Capabilities (NEW)

> Roles should declare what they *can do*, not just what they *are*. Type-based wiring instead of name-based wiring is what makes systems evolvable.

*Source: Theme 1 — convergence across all four expert analyses. This is the single highest-leverage abstraction change.*

### Phase 14: Capability Manifests

| Item | Description | Status |
|---|---|---|
| 14A | `capabilities:` field on roles (semantic intent tags) | -- |
| 14B | Human-readable port type annotations (derived from schema) | -- |
| 14C | Local capability resolver (`config.find_roles_by_capability("summarization")`) | -- |
| 14D | `--find-role` CLI flag (search by capability, input/output type) | -- |

**14A Design — Capabilities Field:**

```yaml
---
name: code-reviewer
description: Reviews code for bugs and security issues
capabilities: [code-review, security-audit, rust, python]
input_schema:
  type: object
  properties:
    code: { type: string }
    language: { type: string }
output_schema:
  type: object
  properties:
    issues: { type: array }
    severity: { type: string, enum: [low, medium, high, critical] }
---
```

Capabilities are free-form string tags. They enable discovery ("find me a role that can do code-review") without requiring formal ontology. This mirrors MCP Server Cards' approach to tool discovery.

**14C Design — Capability Resolver:**

```rust
// New method on Config
pub fn find_roles_by_capability(&self, capability: &str) -> Vec<&Role> {
    self.roles.iter()
        .filter(|r| r.capabilities().iter().any(|c| c.contains(capability)))
        .collect()
}

pub fn find_roles_by_port(&self, input_type: Option<&str>, output_type: Option<&str>) -> Vec<&Role> {
    self.roles.iter()
        .filter(|r| {
            let input_ok = input_type.map_or(true, |t| r.port_accepts(t));
            let output_ok = output_type.map_or(true, |t| r.port_produces(t));
            input_ok && output_ok
        })
        .collect()
}
```

**14D Design — Find Role CLI:**

```bash
$ aichat --find-role --capability code-review
  code-reviewer    in: {code, language}  out: {issues, severity}  capabilities: [code-review, security-audit]
  lint-checker     in: text              out: {errors}            capabilities: [code-review, linting]

$ aichat --find-role --accepts json --produces json
  classifier       in: json{text}        out: json{label, confidence}
  transformer      in: json{...}         out: json{...}
```

**Files:** `src/config/role.rs` (add `capabilities: Vec<String>`, `port_accepts()`, `port_produces()`), `src/config/mod.rs` (add resolver methods), `src/cli.rs` (add `--find-role` flag), `src/main.rs` (render results).

### Phase 15: Contract Testing

| Item | Description | Status |
|---|---|---|
| 15A | Pipeline schema compatibility check at authoring time (`showboat validate-pipeline`) | -- |
| 15B | Cross-stage schema containment validation (output N satisfies input N+1) | -- |
| 15C | `--check` flag for validating role/pipeline definitions without execution | -- |

**15A Design — Authoring-Time Validation:**

```bash
$ showboat validate-pipeline extract-review-format

Pipeline: extract-review-format (3 stages)
  Stage 1: extract
    output_schema: { text: string, metadata: object }
  Stage 2: review
    input_schema:  { content: string, language: string }     # MISMATCH
    output_schema: { issues: array, severity: string }
  Stage 3: format
    input_schema:  { issues: array }                         # OK (subset)

FAIL: Stage 1 output -> Stage 2 input
  Missing: content, language
  Extra: text, metadata
  Suggestion: Add a transform role or update schemas for compatibility.
```

JSON Schema containment check: verify that a document conforming to output_schema would pass input_schema validation. This is deterministic — no LLM needed. Zero runtime cost, prevents an entire class of pipeline failures.

**Files:** `src/config/preflight.rs` (new: pipeline schema validation), integration with `showboat` command.

---

## Epic 5: Server Pipeline Engine -- [design](./analysis/epic-3.md)

*Renumbered from original Epic 3. Exposes AIChat's unique runtime capabilities over HTTP, turning the server from a proxy into a pipeline execution engine.*

> **[DEFERRED 2026-04-17]** Phases 16, 17, and 18 are parked while Epic 9
> (Knowledge Evolution) is in flight. The existing `--serve` behavior is
> unchanged; expanding the server surface is a future-session decision.

### Phase 16: Server Hardening

| Item | Description | Status |
|---|---|---|
| 16A | Configurable CORS origins (`serve_cors_origins:` in config.yaml) | -- |
| 16B | Optional bearer token auth (`serve_api_key:`) | -- |
| 16C | Health endpoint (`GET /health`) | -- |
| 16D | Streaming usage in final SSE chunk | -- |
| 16E | Hot-reload endpoint (`POST /v1/reload`) | -- |
| 16F | Role metadata security (`RolePublicView` — hide prompt text, shell commands, filesystem paths) | -- |
| 16G | Single-role retrieval (`GET /v1/roles/{name}`) | -- |
| 16H | Cost in API responses (`usage.cost_usd` + `X-AIChat-Cost-USD` header) | -- |

### Phase 17: Role & Pipeline Execution

| Item | Description | Status |
|---|---|---|
| 17A | Roles as virtual models (`model: "role:classify"` in `/v1/chat/completions`) | -- |
| 17B | Role invocation endpoint (`POST /v1/roles/{name}/invoke` — non-streaming) | -- |
| 17C | Role invocation endpoint (streaming with stage-boundary SSE events) | -- |
| 17D | Pipeline execution endpoint (`POST /v1/pipelines/run` — named or inline stages) | -- |
| 17E | Batch processing endpoint | -- |

**17A Design:** Roles appear as virtual models in `/v1/models`. OpenWebUI sees them in its model dropdown. Selecting `role:code-reviewer` transparently executes the full role pipeline. Zero changes to OpenWebUI.

**17B Design:** Dedicated endpoint with structured input, variables, model override, and trace:

```json
POST /v1/roles/classify/invoke
{
  "input": "Review this code for security issues...",
  "variables": {"language": "rust"},
  "model": "deepseek:deepseek-chat",
  "trace": true
}
```

Response includes `output`, `usage` (with `cost_usd`), `schema_valid`, and optional `trace` with per-stage breakdown.

### Phase 18: Discovery & Estimation

| Item | Description | Status |
|---|---|---|
| 18A | Cost estimation endpoint (`POST /v1/estimate` — token/cost preview without LLM call) | -- |
| 18B | OpenAPI specification (`GET /v1/openapi.json`) | -- |
| 18C | Root page (`GET /` — endpoint listing with links to spec) | -- |

**18A Design:** Returns estimated cost plus cheaper alternatives:

```json
{
  "estimated_cost_usd": 0.015,
  "alternatives": [
    {"model": "deepseek:deepseek-chat", "estimated_cost_usd": 0.0004},
    {"model": "openai:gpt-4o-mini", "estimated_cost_usd": 0.002}
  ]
}
```

---

## Epic 6: Universal Addressing (NEW)

> A pipeline stage that says `role: "review"` should resolve identically whether `review` is a local YAML file, an agent directory, a role exposed by a remote aichat server, or an MCP tool.

*Source: Theme 5 — AI Architect. The "remote aichat" discovery is the seed of this epic. Absorbs Epic 5's unified entity resolution (F4) and agent-in-pipeline (F6).*

### Phase 19: RoleResolver & Unified Entity Resolution

| Item | Description | Status |
|---|---|---|
| 19A | `RoleResolver` trait (unified resolution across entity types) | -- |
| 19B | Unified entity resolution under `-r` (roles -> agents -> macros, with explicit `-a`/`--macro` overrides) | -- |
| 19C | Agent-in-pipeline (pipeline stages resolve agents via `to_role()` bridge) | -- |
| 19D | Agent MCP binding (`mcp_servers:` on AgentConfig, reuses Phase 6C machinery) | -- |

**19A Design — RoleResolver:**

```rust
pub trait RoleResolver {
    fn resolve(&self, address: &str) -> Result<ResolvedRole>;
    fn discover(&self, query: &CapabilityQuery) -> Result<Vec<RoleSummary>>;
}

pub enum RoleAddress {
    Local(String),                          // "review" -> roles/review.md
    Agent(String),                          // "agent:triage" -> agents/triage/
    Remote { host: String, role: String },  // "remote:staging:8080/review"
    Mcp { server: String, tool: String },   // "mcp:github/create_pr"
}

pub struct ResolvedRole {
    pub role: Role,
    pub source: RoleAddress,
    pub capabilities: Vec<String>,
}
```

**19B Design:** The `-r` flag uses unified resolution:

```rust
pub fn resolve_entity(&self, name: &str) -> Result<EntityRef> {
    // 1. Explicit prefix: "agent:foo", "remote:host/bar", "mcp:server/tool"
    if let Some(ref_) = self.resolve_prefixed(name)? { return Ok(ref_); }
    // 2. Local roles
    if let Ok(role) = self.retrieve_role(name) { return Ok(EntityRef::Role(role)); }
    // 3. Agents
    if self.agent_names().contains(&name.to_string()) { return Ok(EntityRef::Agent(name.to_string())); }
    // 4. Macros
    if self.macro_names().contains(&name.to_string()) { return Ok(EntityRef::Macro(name.to_string())); }
    bail!("Entity '{}' not found (checked roles, agents, macros)", name)
}
```

Backward compatible: `-a name` always resolves as agent. `--macro name` always resolves as macro.

**Files:** `src/config/resolver.rs` (new: RoleResolver trait + local impl), `src/config/mod.rs` (resolve_entity), `src/main.rs` (use resolve_entity for `-r`), `src/pipe.rs` (agent fallback in stage resolution), `src/config/agent.rs` (add `mcp_servers`).

### Phase 20: Remote & Federated Composition

| Item | Description | Status |
|---|---|---|
| 20A | Remote role resolution (`remote:host:port/role-name` addressing) | -- |
| 20B | Remote role discovery (query remote aichat's `/v1/roles` for capabilities) | -- |
| 20C | `remotes:` config section (named remote aichat instances) | -- |
| 20D | Federated pipeline execution (stages can reference remote roles) | -- |

**20A Design — Remote Resolution:**

```yaml
# config.yaml
remotes:
  staging:
    endpoint: http://staging.internal:8080
    api_key: ${STAGING_API_KEY}
  security:
    endpoint: http://security-scanner.internal:8080
```

```yaml
# roles/secure-review.md
pipeline:
  - role: extract                              # local
  - role: remote:security/vulnerability-scan   # remote aichat instance
  - role: summarize                            # local
```

**Implementation:** `RemoteRoleResolver` implements `RoleResolver`. Resolution calls `GET /v1/roles/{name}` on the remote. Execution calls `POST /v1/roles/{name}/invoke`. Requires Epic 5 Phase 17B to exist.

This is the pattern the user discovered accidentally — two aichat instances composing roles across machines. A triage role on machine A routes to code-analysis on machine B (which has the codebase) and security-scan on machine C (which has vulnerability databases).

**Files:** `src/config/resolver.rs` (add `RemoteRoleResolver`), `src/config/mod.rs` (parse `remotes:` config), `src/pipe.rs` (dispatch to remote resolver for `remote:` prefix stages).

---

## Epic 7: DAG Execution (NEW)

> Sequential pipelines are the bicycle. DAGs with conditional routing are the car. The runtime foundation (concurrent tool execution via `join_all`) already exists.

*Source: Theme 4 — AI Architect. Three new primitives within the existing pipeline model: fan-out, conditional, merge.*

### Phase 21: Pipeline DAG Primitives

| Item | Description | Status |
|---|---|---|
| 21A | Fan-out (run multiple stages in parallel on same input) | -- |
| 21B | Conditional routing (`when:` predicate routes to stage A or B) | -- |
| 21C | Merge (combine parallel outputs into single input for next stage) | -- |
| 21D | Pipeline DAG validation (cycle detection, unreachable nodes, type compatibility) | -- |

**21A/21B/21C Design — DAG Pipeline Syntax:**

Stays declarative YAML. Additive to existing sequential model — a `pipeline:` without `parallel:` or `when:` works exactly as today.

```yaml
# Sequential (unchanged)
pipeline:
  - role: extract
  - role: summarize

# Fan-out + merge
pipeline:
  - role: extract
  - parallel:
      - role: security-review
      - role: style-review
      - role: performance-review
    merge: concatenate          # concatenate | json_array | custom_role
  - role: synthesize

# Conditional routing
pipeline:
  - role: classify
  - switch:
      - when: { output_field: "category", equals: "bug" }
        role: bug-triage
      - when: { output_field: "category", equals: "feature" }
        role: feature-review
      - otherwise:
        role: general-review
  - role: format
```

**Implementation:**

`PipelineStage` becomes an enum:

```rust
enum PipelineNode {
    Stage(PipelineStage),                   // existing sequential stage
    Parallel {
        branches: Vec<PipelineNode>,
        merge: MergeStrategy,
    },
    Switch {
        conditions: Vec<ConditionalBranch>,
        otherwise: Option<Box<PipelineNode>>,
    },
}

enum MergeStrategy {
    Concatenate,                            // join outputs with newlines
    JsonArray,                              // wrap in JSON array
    CustomRole(String),                     // merge via a role
}

struct ConditionalBranch {
    when: Predicate,                        // JSONPath predicate on prior output
    node: PipelineNode,
}
```

**Fan-out runtime:** Uses existing `futures_util::future::join_all` from Phase 7D2. Each parallel branch gets a clone of the input. Branches execute concurrently.

**Conditional runtime:** Evaluate `when:` predicates against the previous stage's JSON output. Predicates are deterministic — `output_field`, `equals`, `contains`, `gt`, `lt` — no LLM call.

**Merge strategies:**
- `concatenate`: join outputs with `\n---\n` separator
- `json_array`: `[output1, output2, output3]`
- `custom_role`: pipe concatenated outputs through a merge role

**Files:** `src/pipe.rs` (refactor `PipelineStage` to `PipelineNode` enum, add parallel/conditional execution), `src/config/role.rs` (parse DAG YAML syntax).

### Phase 22: DAG Observability & Budget

| Item | Description | Status |
|---|---|---|
| 22A | DAG trace visualization (tree structure in `--trace` output) | -- |
| 22B | Per-branch cost tracking in parallel execution | -- |
| 22C | Budget-aware fan-out (split pipeline budget across parallel branches) | -- |
| 22D | DAG stage caching (cache branches independently, skip unchanged) | -- |

**22A Design — DAG Trace:**

```
[pipeline] secure-review (5 stages, 2 parallel)
  [1] extract              deepseek:deepseek-chat   500→200tok  $0.0001  0.8s
  [2] parallel (3 branches)
    [2a] security-review   claude:claude-sonnet-4-6  200→300tok  $0.004   1.2s
    [2b] style-review      deepseek:deepseek-chat    200→150tok  $0.0001  0.6s
    [2c] perf-review       deepseek:deepseek-chat    200→180tok  $0.0001  0.7s
    merge: concatenate     --                        --          --       0ms
  [3] synthesize           claude:claude-sonnet-4-6  630→200tok  $0.006   1.5s
  total: $0.0103  4.3s (wall) vs 6.1s (sequential)
```

---

## Epic 8: Feedback Loop (NEW)

> Roles have no metrics, no regression testing, no A/B comparison. Every role invocation should be a scored data point.

*Source: Theme 2 — ML Engineer + ML App Engineer analyses. Closes the gap between "prompt template" and "optimizable, testable, versionable component."*

### Phase 23: Role Evaluation

| Item | Description | Status |
|---|---|---|
| 23A | `metrics:` field on roles (shell commands that score output) | -- |
| 23B | `--compare` flag (run input through two roles, show results side-by-side with cost) | -- |
| 23C | Cost attribution by role in run log (tag each pipeline stage in JSONL) | -- |
| 23D | Role invocation history (append scored records to per-role ledger) | -- |

**23A Design — Metrics Field:**

```yaml
---
name: summarizer
metrics:
  - name: valid_json
    shell: "jq . >/dev/null 2>&1"
  - name: under_500_words
    shell: "test $(wc -w < /dev/stdin) -lt 500"
  - name: has_required_fields
    shell: "jq -e '.summary and .key_points' >/dev/null 2>&1"
---
```

Each metric receives the role's output on stdin and exits 0 (pass) or 1 (fail). Metrics run after output validation, before lifecycle hooks. Results recorded in the JSONL run log alongside cost and tokens.

**Implementation:** In `src/main.rs`, after `validate_schema("output", ...)`, iterate `role.metrics()`. For each, pipe output to the shell command. Record `{metric_name, pass: bool}` in the trace event.

**Files:** `src/config/role.rs` (add `metrics: Vec<RoleMetric>`), `src/main.rs` (evaluate metrics post-output), `src/utils/trace.rs` (emit metric events).

**23B Design — Compare Flag:**

```bash
$ echo "Review this code" | aichat --compare summarizer-v1 summarizer-v2

--- summarizer-v1 (deepseek:deepseek-chat) ---
  Output: { "summary": "...", "key_points": [...] }
  Metrics: valid_json=PASS  under_500_words=PASS  has_required_fields=PASS
  Cost: $0.0004  (892 input, 341 output tokens)

--- summarizer-v2 (claude:claude-haiku-4-5) ---
  Output: { "summary": "...", "key_points": [...] }
  Metrics: valid_json=PASS  under_500_words=PASS  has_required_fields=PASS
  Cost: $0.002  (892 input, 287 output tokens)

--- Comparison ---
  Cost ratio: summarizer-v2 is 5.0x more expensive
  Token ratio: summarizer-v2 uses 16% fewer output tokens
  Metrics: both pass all metrics
```

Manual A/B testing with zero infrastructure. Combined with the metrics field, this becomes systematic.

**Files:** `src/cli.rs` (add `--compare` flag taking two role names), `src/main.rs` (parallel execution and diff rendering).

**23C Design — Cost Attribution by Role:**

Currently the JSONL run log records the top-level role but not per-stage breakdown. Add `stage_role` and `pipeline_role` fields to each run log entry:

```jsonl
{"role":"extract","pipeline":"secure-review","stage":1,"model":"deepseek:deepseek-chat","cost_usd":0.0001,...}
{"role":"review","pipeline":"secure-review","stage":2,"model":"claude:claude-sonnet-4-6","cost_usd":0.012,...}
```

This enables downstream aggregation: `duckdb "SELECT role, SUM(cost_usd) FROM read_json('run.jsonl') GROUP BY role"`.

**Files:** `src/pipe.rs` (add `stage_role` + `pipeline_role` to trace/run log entries), `src/utils/ledger.rs` (extend run log schema).

### Phase 24: Regression Testing & Prompt Distillation

| Item | Description | Status |
|---|---|---|
| 24A | Role regression testing (replay saved input/output pairs, check metrics) | -- |
| 24B | Role-as-judge pattern (document + example roles) | -- |
| 24C | Prompt distillation pipeline (expensive model -> validate -> append examples to cheap role) | -- |
| 24D | `showboat validate-role` integration (replay test cases from trace log) | -- |

**24A Design — Role Regression Testing:**

When you edit a role's prompt, you have no way to verify it still produces acceptable output. Regression testing replays saved input/output pairs from the trace log:

```bash
$ aichat --test-role summarizer

Replaying 5 recorded invocations for 'summarizer':
  [1/5] input: "The auth flow..."     metrics: 3/3 PASS   cost: $0.0004
  [2/5] input: "OAuth2 requires..."   metrics: 3/3 PASS   cost: $0.0004
  [3/5] input: "Session tokens..."    metrics: 2/3 FAIL   cost: $0.0004
    FAIL: under_500_words (output was 623 words)
  [4/5] input: "API gateway..."       metrics: 3/3 PASS   cost: $0.0004
  [5/5] input: "Rate limiting..."     metrics: 3/3 PASS   cost: $0.0004

Result: 4/5 passed (80%)  Total cost: $0.002
```

Test cases are extracted from the role's invocation history (Phase 23D). The `--save-test` flag captures the current invocation as a test case.

**24B Design — Role-as-Judge:**

```bash
$ aichat -r writer "Explain OAuth2" | aichat -r judge
```

A `judge` role with structured output:

```yaml
---
name: judge
description: Evaluate LLM output quality
output_schema:
  type: object
  properties:
    score: { type: integer, minimum: 1, maximum: 5 }
    reasoning: { type: string }
    pass: { type: boolean }
  required: [score, reasoning, pass]
---
Evaluate the following text for clarity, accuracy, and completeness.
Score 1-5. Pass if score >= 3.
__INPUT__
```

Two YAML files replace heavyweight eval frameworks (DeepEval, Confident AI).

**24C Design — Prompt Distillation Pipeline:**

Use an expensive model to generate high-quality outputs, then use those as few-shot examples for a cheap model:

```yaml
# roles/distill.md
---
name: distill
pipeline:
  - role: generate-with-expensive   # stage 1: expensive model generates
  - role: validate-output           # stage 2: check metrics
  - role: append-example            # stage 3: add passing examples to target role
---
```

This is what DSPy's BootstrapFinetune does, but approximated without fine-tuning — just example curation through existing pipeline mechanics.

---

## Epic 9: RAG Evolution -- [design](./analysis/epic-4.md)

*Renumbered from original Epic 4.*

### Phase 25: Structured Retrieval

| Item | Description | Status |
|---|---|---|
| 25A | Sibling chunk expansion (prev/next sibling links, search-time context windows) | -- |
| 25B | Metadata-enriched chunks (heading hierarchy, function names, line ranges) | -- |
| 25C | Incremental HNSW insertion (full rebuild only on deletion) | -- |
| 25D | Binary vector storage (`rag.bin` sidecar, ~40x faster load) | -- |

### Phase 26: RAG Composability

| Item | Description | Status |
|---|---|---|
| 26A | Role `rag:` field (declarative RAG binding, like `mcp_servers:`) | -- |
| 26B | Pipeline RAG integration (`use_embeddings()` in pipeline stages) | -- |
| 26C | CLI RAG mode (`--rag name` in non-REPL invocations) | -- |
| 26D | Search-only mode (`--rag-search` — retrieval without LLM, zero cost) | -- |
| 26E | Multi-RAG search (federated search across multiple RAGs, RRF merge) | -- |
| 26F | RAG as LLM tool (`rag_mode: tool` — agent-directed search, not auto-inject) | -- |

### Phase 27: Graph Expansion & Observability

| Item | Description | Status |
|---|---|---|
| 27A | Chunk-adjacency graph (markdown links, imports, cross-references as edges) | -- |
| 27B | RAG trace integration (search events in `--trace`, chunk source attribution) | -- |

---

## Epic 10: Entity Evolution -- [design](./analysis/epic-5.md)

*Renumbered from original Epic 5. Some items absorbed by Epic 6 (Universal Addressing).*

### Phase 28: Agent Composability

| Item | Description | Status |
|---|---|---|
| 28A | Agent-as-tool (agents callable via `ToolCall::eval()` dispatch, recursion depth limit) | -- |
| 28B | Configurable react loop (`react_max_steps:` in frontmatter, `finish` synthetic tool) | -- |
| 28C | Macro output chaining (`%%` variable resolves to previous step's output) | -- |

### Phase 29: Agent Dynamism

| Item | Description | Status |
|---|---|---|
| 29A | ReactPolicy trait (composable policies: CostGuard, StagnationGuard, ModelEscalation) | -- |
| 29B | Agent memory (JSONL fact store, trace-to-memory bridging, `memory: true` in AgentConfig) | -- |

---

## Cross-Epic Dependency Graph

```
Epic 1 (Core Platform)         ──── DONE ──────────────────────────────────────────
  │
  ├── Epic 2 (Runtime Intelligence) ─── Phases 9-11 ─── Infrastructure for everything
  │     │
  │     ├── Epic 3 (Composition UX) ─── Phases 12-13 ─── Low cost, can start early
  │     │     │
  │     │     └── Epic 4 (Typed Ports) ─── Phases 14-15 ─── Foundational abstraction
  │     │           │
  │     │           ├── Epic 5 (Server Engine) ─── Phases 16-18 ─── Enables remote
  │     │           │     │
  │     │           │     └── Epic 6 (Universal Addressing) ─── Phases 19-20
  │     │           │           │
  │     │           │           └── Epic 7 (DAG Execution) ─── Phases 21-22
  │     │           │
  │     │           └── Epic 8 (Feedback Loop) ─── Phases 23-24 ─── Independent track
  │     │
  │     └── Epic 9 (RAG Evolution) ─── Phases 25-27 ─── Parallel track
  │
  └── Epic 10 (Entity Evolution) ─── Phases 28-29 ─── After addressing is unified
```

**Parallel tracks:** Epics 8 (Feedback Loop) and 9 (RAG Evolution) can proceed in parallel with Epics 5-7, as they share no code dependencies.

**Critical path:** Epic 2 → Epic 4 → Epic 5 → Epic 6 → Epic 7

---

## What NOT to Build

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

---

## Success Metrics

| Metric | Current State | Target | Epic |
|---|---|---|---|
| Schema failure rate with `output_schema` | Unknown | <5% (Phase 9A/B), <1% (Phase 9C) | 2 |
| Pipeline re-run cost after stage failure | 100% (full re-run) | Stage cost only (Phase 10B cache) | 2 |
| Time to understand a role before using it | Read YAML file | `--dry-run` shows everything in 0 tokens | 3 |
| Time to create a role variant | 5 min (copy + edit) | 5 sec (`--fork-role`) | 3 |
| Can compose roles across machines | Accidental (HTTP hack) | First-class (`remote:host/role`) | 6 |
| Pipeline topology | Sequential only | Fan-out, conditional, merge | 7 |
| Role quality tracking | None | Per-role metrics + regression tests | 8 |
| AIChat features accessible via HTTP | 3 (chat, embed, rerank) | 8+ (roles, pipelines, batch, cost) | 5 |
| Context utilization for `-f dir/` | 100% of files (wasteful) | BM25-ranked, budget-optimized | 2 |
| Pre-flight error prevention | 0 errors caught | All capability mismatches caught | 2 |
| Batch cost savings with mixed complexity | 0% (static model) | 40-60% (deterministic routing) | 2 |

---

## Phase Summary Table

| Phase | Epic | Scope | Key Deliverable |
|---|---|---|---|
| 0-8 | 1: Core Platform | Done | Foundation |
| 9 | 2: Runtime Intelligence | Schema Fidelity | Native structured output, schema retry |
| 10 | 2: Runtime Intelligence | Resilience & Routing | API retry, stage cache, cost-aware `model_policy:` |
| 11 | 2: Runtime Intelligence | Context Budget | Budget allocator, BM25 ranking, pipeline budget propagation |
| 12 | 3: Composition UX | Discoverability | `--dry-run` resolved, port signatures, composition summaries |
| 13 | 3: Composition UX | Authoring | `--fork-role`, error teaching, guardrail examples |
| 14 | 4: Typed Ports | Capabilities | `capabilities:` field, capability resolver, `--find-role` |
| 15 | 4: Typed Ports | Contract Testing | Pipeline schema validation, `--check` flag |
| 16 | 5: Server Engine | Hardening | CORS, auth, health, cost headers |
| 17 | 5: Server Engine | Execution | Virtual models, role invoke, pipeline endpoint |
| 18 | 5: Server Engine | Discovery | Cost estimation, OpenAPI spec |
| 19 | 6: Universal Addressing | Resolution | `RoleResolver` trait, unified `-r`, agent-in-pipeline |
| 20 | 6: Universal Addressing | Federation | Remote roles, `remotes:` config, federated pipelines |
| 21 | 7: DAG Execution | Primitives | `parallel:`, `switch:`/`when:`, merge strategies |
| 22 | 7: DAG Execution | Observability | DAG trace, per-branch cost, budget-aware fan-out |
| 23 | 8: Feedback Loop | Evaluation | `metrics:` field, `--compare`, cost attribution |
| 24 | 8: Feedback Loop | Testing | Role regression, role-as-judge, prompt distillation |
| 25 | 9: RAG Evolution | Retrieval | Sibling expansion, metadata, incremental indexing |
| 26 | 9: RAG Evolution | Composability | Role `rag:`, CLI RAG, multi-RAG, RAG-as-tool |
| 27 | 9: RAG Evolution | Graph | Chunk adjacency, RAG trace |
| 28 | 10: Entity Evolution | Composability | Agent-as-tool, configurable loop, macro chaining |
| 29 | 10: Entity Evolution | Dynamism | ReactPolicy trait, agent memory |
