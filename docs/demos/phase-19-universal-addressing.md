# Phase 19: Universal Addressing - RoleResolver & Unified Entity Resolution

*2026-05-04T15:14:56Z by Showboat 0.6.1*
<!-- showboat-id: 1fc4ab31-2449-4de9-8cb7-5f1038bdb79e -->

Phase 19 lands the first half of Epic 6: Universal Addressing. It introduces a single addressing layer that all entity types eventually share, and unifies what `-r` accepts so a name dispatches to the right kind (role, agent, or macro) automatically. Phase 20 (remote/federated composition) is hard-blocked on Epic 5 Phase 17B and remains parked.

Four items shipped:
- 19A — `RoleResolver` trait, `RoleAddress` parser, `EntityRef` enum. Parser supports `agent:`, `macro:`, `remote:`, `mcp:` prefixes; only the first two resolve in this phase. The remaining two are forward stubs so addresses written today survive Phase 20.
- 19B — Unified `-r` resolution. Roles still take precedence; bare names fall back to agents, then macros. `-a` and `--macro` remain authoritative. Explicit-prefix errors propagate; bare-name failures fall through to the legacy code path.
- 19C — Agent-in-pipeline. Pipeline stages can resolve to agents via `Agent::init().to_role()`. Macros are rejected at preflight.
- 19D — `mcp_servers:` on `AgentConfig`. Reuses the Phase 6C role expansion via a shared helper, so role and agent MCP binding stay in lockstep.

## 19A — Public surface in `src/config/resolver.rs`

```bash
grep -E "^pub (fn|struct|enum|trait) " src/config/resolver.rs
```

```output
pub fn expand_mcp_servers_into_use_tools(
pub enum RoleAddress {
pub enum EntityRef {
pub trait RoleResolver {
pub fn pipeline_stage_admissible(entity: &EntityRef) -> Result<()> {
pub fn classify_address(
```

## 19A unit tests — address parsing + classification + admissibility

```bash
cargo test --bin aichat --quiet config::resolver:: 2>&1 | tail -3 | sed "s/finished in [0-9.]*s/finished in Xs/" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/"
```

```output
.............................
test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs

```

## 19B — Unified `-r` reroute in action

The same `-r NAME` invocation now resolves to whichever entity exists. `aichat` is configured as an agent; passing it via `-r` reroutes through `Config::classify_entity` and dispatches as if `-a aichat` were given.

```bash
./target/debug/aichat -r aichat --info 2>&1 | head -3
```

```output
name: aichat
config:
  model: null
```

## 19B — Explicit prefixes propagate precise errors

When the user writes `agent:NAME`, the prefix is honored even if a role of the same name exists. If the agent doesn't exist, the error is agent-flavored — not the legacy generic "Unknown role" message.

```bash
./target/debug/aichat -r agent:doesnotexist --info 2>&1 | head -1
```

```output
Error: Agent 'doesnotexist' not found
```

```bash
./target/debug/aichat -r macro:doesnotexist --info 2>&1 | head -1
```

```output
Error: Macro 'doesnotexist' not found
```

## 19B — Bare-name resolution still falls through cleanly

A genuinely-unknown bare name doesn't get the agent/macro error — it falls through to the legacy `use_role` path so the role-flavored error fires unchanged.

```bash
./target/debug/aichat -r nopenotfound --info 2>&1 | head -1
```

```output
Error: Unknown role `nopenotfound`
```

## 19D — `mcp_servers:` lands on `AgentConfig`

Agents now declare MCP server bindings the same way roles do. The `Agent::init` path expands the list into `use_tools` via the helper extracted in `resolver.rs` — the role path was refactored to use the same helper, keeping the two binding semantics identical.

```bash
grep -E "mcp_servers" src/config/agent.rs | head -3
```

```output
        if !agent_config.mcp_servers.is_empty() {
                cfg.mcp_servers.keys().map(|s| s.as_str()).collect();
            let new_use_tools = super::resolver::expand_mcp_servers_into_use_tools(
```

```bash
cargo test --bin aichat --quiet config::agent::tests 2>&1 | tail -3 | sed "s/finished in [0-9.]*s/finished in Xs/" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/"
```

```output
.....
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs

```

## 19C — Agent-in-pipeline + macro rejection

Pipeline stages now resolve through `classify_entity` and accept either roles or agents. Macros are gated out at preflight with a clear, actionable error so the user isn't surprised at execution time. Pre-existing pipeline integration tests confirm the rerouted error message still fires for genuinely-unknown stage names.

```bash
bats tests/integration/pipeline.sh 2>&1 | tail -6
```

```output
1..4
ok 1 pipeline: --stage with invalid role fails preflight
ok 2 pipeline: --pipe-def with non-existent file fails
ok 3 pipeline: role with pipeline frontmatter
ok 4 pipeline: --stage overrides model
```

## Full suite — no regressions

```bash
cargo test --bin aichat --quiet > /tmp/p19_unit.log 2>&1; grep "^test result:" /tmp/p19_unit.log | sed "s/finished in [0-9.]*s/finished in Xs/" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/; s/[0-9]+ passed/N passed/"
```

```output
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```

```bash
cargo test --quiet --test compatibility > /tmp/p19_compat.log 2>&1; grep "^test result:" /tmp/p19_compat.log | sed "s/finished in [0-9.]*s/finished in Xs/" | sed -E "s/finished in [0-9.]+s/finished in Xs/; s/[0-9]+ filtered out/N filtered out/; s/[0-9]+ passed/N passed/"
```

```output
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; N filtered out; finished in Xs
```
