# Epic 6: Universal Addressing

**Created:** 2026-04-07
**Status:** Planning
**Depends on:** Epic 5 Phase 17B (role invocation endpoint enables remote resolution)
**Phases:** 19-20
**Source:** Theme 5 — AI Architect + ML Engineer. Absorbs Epic 10 F4/F6/F7.

---

## Motivation

A pipeline stage that says `role: "review"` should resolve identically whether `review` is a local YAML file, an agent directory, a role exposed by a remote aichat server, or an MCP tool. The user's accidental discovery — two aichat instances composing roles across machines — is the seed of this epic.

---

## Phases

### Phase 19: RoleResolver & Unified Entity Resolution

| Item | Description |
|---|---|
| 19A | `RoleResolver` trait (unified resolution across entity types) |
| 19B | Unified entity resolution under `-r` (roles → agents → macros, explicit overrides preserved) |
| 19C | Agent-in-pipeline (pipeline stages resolve agents via `to_role()` bridge) |
| 19D | Agent MCP binding (`mcp_servers:` on AgentConfig, reuses Phase 6C machinery) |

### Phase 20: Remote & Federated Composition

| Item | Description |
|---|---|
| 20A | Remote role resolution (`remote:host:port/role-name` addressing) |
| 20B | Remote role discovery (query remote aichat's `/v1/roles` for capabilities) |
| 20C | `remotes:` config section (named remote aichat instances) |
| 20D | Federated pipeline execution (stages can reference remote roles) |

---

## Key Designs

**19A — RoleResolver Trait:**

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
```

**19B — Unified `-r` Resolution:**

```rust
pub fn resolve_entity(&self, name: &str) -> Result<EntityRef> {
    if let Some(ref_) = self.resolve_prefixed(name)? { return Ok(ref_); }
    if let Ok(role) = self.retrieve_role(name) { return Ok(EntityRef::Role(role)); }
    if self.agent_names().contains(&name.to_string()) { return Ok(EntityRef::Agent(name.into())); }
    if self.macro_names().contains(&name.to_string()) { return Ok(EntityRef::Macro(name.into())); }
    bail!("Entity '{}' not found", name)
}
```

Backward compatible: `-a` always agent, `--macro` always macro.

**20A — Remote Resolution:**

```yaml
remotes:
  staging:
    endpoint: http://staging.internal:8080
    api_key: ${STAGING_API_KEY}
```

```yaml
pipeline:
  - role: extract                              # local
  - role: remote:security/vulnerability-scan   # remote aichat instance
  - role: summarize                            # local
```

`RemoteRoleResolver` calls `GET /v1/roles/{name}` for resolution, `POST /v1/roles/{name}/invoke` for execution.

Files: `src/config/resolver.rs` (new), `src/config/mod.rs`, `src/main.rs`, `src/pipe.rs`, `src/config/agent.rs`.

Full designs: [ROADMAP.md, Epic 6 section](../ROADMAP.md#epic-6-universal-addressing-new)

---

## Absorbed Features (from former Epic 5 / now Epic 10)

| Feature | Original | Now |
|---|---|---|
| Unified entity resolution | Epic 10 F4 | Phase 19B |
| Agent-in-pipeline | Epic 10 F6 | Phase 19C |
| Agent MCP binding | Epic 10 F7 | Phase 19D |

---

## What NOT to Build

| Proposal | Reason |
|---|---|
| Service mesh / sidecar proxy | Wrong layer. `remote:` prefix in YAML is sufficient. |
| Agent discovery protocol | Roles are the addressable unit. Agents expose via `to_role()`. |
| Distributed state / shared context | Agents communicate via tool call arguments. No shared state. |
