# Phase 19: RoleResolver & Unified Entity Resolution : Overview - Epic 6

**Status (2026-05-04):** Shipped. 27 new resolver unit tests + 5 agent-config tests; full suite 452 unit + 197 compatibility + 4 pipeline integration pass.

| Item | Description | Status |
|---|---|---|
| 19A | `RoleResolver` trait + `RoleAddress` parser + `EntityRef` (parser supports `agent:` / `macro:` / `remote:` / `mcp:` prefixes; only `agent:` / `macro:` resolve in Phase 19) | **Done** |
| 19B | Unified entity resolution under `-r` (roles -> agents -> macros, with explicit `-a`/`--macro` overrides; explicit-prefix errors propagate, bare-name failures fall through) | **Done** |
| 19C | Agent-in-pipeline (pipeline stages resolve agents via `Agent::init().to_role()`; macros rejected at preflight) | **Done** |
| 19D | Agent MCP binding (`mcp_servers:` on `AgentConfig`, shares the Phase 6C expansion helper with roles) | **Done** |

**Files touched:**
- `src/config/resolver.rs` *(new, ~370 lines incl. tests)* — trait, address parser, classifier, MCP-binding helper, pipeline-admissibility gate
- `src/config/mod.rs` — `Config::classify_entity` / `resolve_entity` / `RoleResolver` impl; refactored Phase 6C site to share the helper
- `src/config/agent.rs` — `mcp_servers:` field on `AgentConfig`; `Agent::init` calls the shared expansion helper; +5 unit tests
- `src/config/preflight.rs` — `validate_pipeline_stages` classifies, defers agent-stage capability check to execution, rejects macros
- `src/pipe.rs` — new `resolve_stage_entity()` async helper used by `run_stage_inner`
- `src/main.rs` — `-r` reroute block before agent/macro dispatch
- `tests/integration/pipeline.sh` — error-message string update (`unknown role` → `unknown entity`)

**Limitations carried forward (intentional):**
- Agent-in-pipeline does not interactively prompt for agent variables; missing required vars leave `{{var}}` placeholders unrendered.
- Agent-in-pipeline loads pre-built RAG from disk if present, but never prompts to initialize one (non-interactive context).
- `stage_retries` / `fallback_models` fields are role-only; agent stages run with default retry budgets.
- `Remote { host, role }` and `Mcp { server, tool }` parse as forward stubs; resolution is Phase 20 / a later epic.

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
