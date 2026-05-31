//! Phase 19A: RoleResolver — unified resolution across entity types.
//!
//! Today, `-r` resolves only roles, `-a` only agents, `--macro` only macros, and
//! pipeline stages refer strictly to roles by name. This module introduces a
//! single addressing layer that all of those eventually share.
//!
//! In Phase 19, only `Local`, `Agent`, and `Macro` resolve to a concrete
//! `EntityRef`. `Remote` and `Mcp` parse but defer their resolution to Phase 20
//! (federated composition) and a later MCP-as-role epic respectively. Keeping
//! the variants here means the parser is forward-compatible: addresses written
//! today as `remote:host:8080/foo` will start working when the resolver gains a
//! `RemoteRoleResolver`.

use anyhow::{bail, Result};
use std::collections::HashSet;

/// Phase 6C / 19D: expand a list of `mcp_servers:` names into a comma-joined
/// `server:*` suffix on a `use_tools` string. Used by both `retrieve_role`
/// (Phase 6C) and `Agent::init` (Phase 19D) so role and agent expansion stay
/// in lockstep. Unknown servers are filtered out with a warning — same
/// behavior as the original Phase 6C call site.
pub fn expand_mcp_servers_into_use_tools(
    entity_kind: &str,
    entity_name: &str,
    mcp_servers_requested: &[String],
    current_use_tools: Option<&str>,
    available_servers: &HashSet<&str>,
) -> Option<String> {
    if mcp_servers_requested.is_empty() {
        return current_use_tools.map(str::to_string);
    }
    let mcp_prefixes: Vec<String> = mcp_servers_requested
        .iter()
        .filter(|s| available_servers.contains(s.as_str()))
        .map(|s| format!("{s}:*"))
        .collect();
    for s in mcp_servers_requested {
        if !available_servers.contains(s.as_str()) {
            warn!(
                "{} '{}' references unknown mcp_server '{}' — not in global config",
                entity_kind, entity_name, s
            );
        }
    }
    if mcp_prefixes.is_empty() {
        return current_use_tools.map(str::to_string);
    }
    let existing = current_use_tools.unwrap_or_default();
    if existing.is_empty() {
        Some(mcp_prefixes.join(","))
    } else {
        Some(format!("{},{}", existing, mcp_prefixes.join(",")))
    }
}

/// Parsed form of an entity address. Covers local lookups today and reserves
/// the federated and MCP shapes Phase 20 will fill in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleAddress {
    /// Bare name. Resolved by the role → agent → macro fallback in
    /// [`super::Config::resolve_entity`].
    Local(String),
    /// Forced agent lookup (`agent:foo`).
    Agent(String),
    /// Forced macro lookup (`macro:foo`).
    Macro(String),
    /// Remote aichat instance — `remote:host[:port]/role-name`. Phase 20.
    Remote { host: String, role: String },
    /// MCP tool — `mcp:server/tool`. Reserved for a future MCP-as-role epic.
    Mcp { server: String, tool: String },
}

impl RoleAddress {
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            bail!("Empty entity address");
        }
        if let Some(rest) = trimmed.strip_prefix("agent:") {
            let name = rest.trim();
            if name.is_empty() {
                bail!("Empty name after 'agent:' prefix");
            }
            return Ok(RoleAddress::Agent(name.to_string()));
        }
        if let Some(rest) = trimmed.strip_prefix("macro:") {
            let name = rest.trim();
            if name.is_empty() {
                bail!("Empty name after 'macro:' prefix");
            }
            return Ok(RoleAddress::Macro(name.to_string()));
        }
        if let Some(rest) = trimmed.strip_prefix("remote:") {
            // remote:host[:port]/role-name — split on the LAST '/'
            // so an authority with a port (host:8080) survives intact.
            let (host, role) = rest
                .rsplit_once('/')
                .ok_or_else(|| anyhow::anyhow!(
                    "Remote address '{trimmed}' missing '/role-name' suffix"
                ))?;
            if host.is_empty() || role.is_empty() {
                bail!("Remote address '{trimmed}' has empty host or role");
            }
            return Ok(RoleAddress::Remote {
                host: host.to_string(),
                role: role.to_string(),
            });
        }
        if let Some(rest) = trimmed.strip_prefix("mcp:") {
            let (server, tool) = rest
                .split_once('/')
                .ok_or_else(|| anyhow::anyhow!(
                    "MCP address '{trimmed}' missing '/tool' suffix"
                ))?;
            if server.is_empty() || tool.is_empty() {
                bail!("MCP address '{trimmed}' has empty server or tool");
            }
            return Ok(RoleAddress::Mcp {
                server: server.to_string(),
                tool: tool.to_string(),
            });
        }
        Ok(RoleAddress::Local(trimmed.to_string()))
    }
}

/// What kind of entity a name resolves to, plus the resolved name.
/// Carries the *classification*, not the loaded entity — callers run their
/// existing loader (`retrieve_role`, `Agent::init`, `Config::load_macro`,
/// or `RemoteRoleResolver`) once they know which branch to take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityRef {
    Role(String),
    Agent(String),
    Macro(String),
    /// Phase 20A: a role hosted on a remote aichat server. `target` is
    /// either the name of a `remotes:` config entry or a literal
    /// `host[:port]` authority; the resolver decides which at execution
    /// time by checking the named-remote table first.
    Remote { target: String, role: String },
}

impl EntityRef {
    pub fn kind(&self) -> &'static str {
        match self {
            EntityRef::Role(_) => "role",
            EntityRef::Agent(_) => "agent",
            EntityRef::Macro(_) => "macro",
            EntityRef::Remote { .. } => "remote",
        }
    }

    pub fn name(&self) -> &str {
        match self {
            EntityRef::Role(s) | EntityRef::Agent(s) | EntityRef::Macro(s) => s.as_str(),
            EntityRef::Remote { role, .. } => role.as_str(),
        }
    }
}

/// Phase 19A: a unified resolver. Implementations cover local entities,
/// remote aichat instances (Phase 20), and MCP tools (later).
///
/// `discover` is intentionally omitted at this stage; it requires the
/// `CapabilityQuery` shape from Phase 14 (Typed Ports). Adding it now would
/// commit to a contract before the prerequisite epic lands.
pub trait RoleResolver {
    fn resolve(&self, address: &str) -> Result<EntityRef>;
}

/// Phase 19C / 20D: gatekeeper for pipeline-stage admissibility.
/// Roles and agents both yield a `Role` (agents via `to_role()`), so they're
/// valid stages. Phase 20D adds `Remote` — remote-hosted roles execute via
/// HTTP and stream their output back to the next local stage. Macros run
/// REPL commands in an isolated config clone — they have no role shape and
/// cannot participate in stage chaining.
pub fn pipeline_stage_admissible(entity: &EntityRef) -> Result<()> {
    match entity {
        EntityRef::Role(_) | EntityRef::Agent(_) | EntityRef::Remote { .. } => Ok(()),
        EntityRef::Macro(name) => bail!(
            "Macro '{name}' cannot be used as a pipeline stage \
             (macros execute REPL commands in an isolated config clone, not LLM stages)"
        ),
    }
}

/// Pure classification step — given a parsed address and three "does this
/// name exist?" probes, decide what kind of entity to dispatch as. This is
/// where the role → agent → macro fallback lives, isolated from any I/O so
/// it can be unit-tested without filesystem fixtures.
pub fn classify_address(
    address: &RoleAddress,
    role_exists: impl Fn(&str) -> bool,
    agent_exists: impl Fn(&str) -> bool,
    macro_exists: impl Fn(&str) -> bool,
) -> Result<EntityRef> {
    match address {
        RoleAddress::Local(name) => {
            if role_exists(name) {
                return Ok(EntityRef::Role(name.clone()));
            }
            if agent_exists(name) {
                return Ok(EntityRef::Agent(name.clone()));
            }
            if macro_exists(name) {
                return Ok(EntityRef::Macro(name.clone()));
            }
            bail!("Entity '{name}' not found (checked roles, agents, macros)")
        }
        RoleAddress::Agent(name) => {
            if !agent_exists(name) {
                bail!("Agent '{name}' not found");
            }
            Ok(EntityRef::Agent(name.clone()))
        }
        RoleAddress::Macro(name) => {
            if !macro_exists(name) {
                bail!("Macro '{name}' not found");
            }
            Ok(EntityRef::Macro(name.clone()))
        }
        RoleAddress::Remote { host, role } => {
            // Phase 20A: classification is pure — the resolver does NOT
            // touch the network or the `remotes:` table here. The remote
            // call happens at execution time inside `RemoteRoleResolver`.
            // We accept any well-formed `remote:target/role` here; if the
            // target name is unknown and isn't a routable host, the error
            // surfaces when the HTTP call is attempted.
            Ok(EntityRef::Remote {
                target: host.clone(),
                role: role.clone(),
            })
        }
        RoleAddress::Mcp { .. } => bail!(
            "MCP-as-role resolution is not yet implemented in Phase 19"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_name_as_local() {
        assert_eq!(
            RoleAddress::parse("review").unwrap(),
            RoleAddress::Local("review".into())
        );
    }

    #[test]
    fn trims_whitespace_around_bare_name() {
        assert_eq!(
            RoleAddress::parse("  review  ").unwrap(),
            RoleAddress::Local("review".into())
        );
    }

    #[test]
    fn parses_agent_prefix() {
        assert_eq!(
            RoleAddress::parse("agent:triage").unwrap(),
            RoleAddress::Agent("triage".into())
        );
    }

    #[test]
    fn parses_macro_prefix() {
        assert_eq!(
            RoleAddress::parse("macro:nightly").unwrap(),
            RoleAddress::Macro("nightly".into())
        );
    }

    #[test]
    fn parses_remote_with_port() {
        assert_eq!(
            RoleAddress::parse("remote:staging.internal:8080/review").unwrap(),
            RoleAddress::Remote {
                host: "staging.internal:8080".into(),
                role: "review".into(),
            }
        );
    }

    #[test]
    fn parses_remote_without_port() {
        assert_eq!(
            RoleAddress::parse("remote:host/role").unwrap(),
            RoleAddress::Remote {
                host: "host".into(),
                role: "role".into(),
            }
        );
    }

    #[test]
    fn parses_mcp_address() {
        assert_eq!(
            RoleAddress::parse("mcp:github/create_pr").unwrap(),
            RoleAddress::Mcp {
                server: "github".into(),
                tool: "create_pr".into(),
            }
        );
    }

    #[test]
    fn rejects_empty_input() {
        assert!(RoleAddress::parse("").is_err());
        assert!(RoleAddress::parse("   ").is_err());
    }

    #[test]
    fn rejects_empty_after_prefix() {
        assert!(RoleAddress::parse("agent:").is_err());
        assert!(RoleAddress::parse("macro:").is_err());
        assert!(RoleAddress::parse("remote:host/").is_err());
        assert!(RoleAddress::parse("remote:/role").is_err());
        assert!(RoleAddress::parse("mcp:srv/").is_err());
        assert!(RoleAddress::parse("mcp:/tool").is_err());
    }

    #[test]
    fn rejects_remote_missing_role() {
        assert!(RoleAddress::parse("remote:host:8080").is_err());
    }

    fn servers<'a>(names: &[&'a str]) -> HashSet<&'a str> {
        names.iter().copied().collect()
    }

    #[test]
    fn mcp_expansion_empty_list_preserves_use_tools() {
        let avail = servers(&["foo"]);
        assert_eq!(
            expand_mcp_servers_into_use_tools("role", "x", &[], Some("a,b"), &avail),
            Some("a,b".into())
        );
        assert_eq!(
            expand_mcp_servers_into_use_tools("role", "x", &[], None, &avail),
            None
        );
    }

    #[test]
    fn mcp_expansion_appends_known_server() {
        let avail = servers(&["foo"]);
        let out = expand_mcp_servers_into_use_tools(
            "role",
            "x",
            &["foo".into()],
            None,
            &avail,
        );
        assert_eq!(out, Some("foo:*".into()));
    }

    #[test]
    fn mcp_expansion_appends_to_existing_use_tools() {
        let avail = servers(&["foo"]);
        let out = expand_mcp_servers_into_use_tools(
            "agent",
            "y",
            &["foo".into()],
            Some("local_tool"),
            &avail,
        );
        assert_eq!(out, Some("local_tool,foo:*".into()));
    }

    #[test]
    fn mcp_expansion_filters_unknown_servers() {
        let avail = servers(&["foo"]);
        // Only "foo" is known; "bar" is dropped.
        let out = expand_mcp_servers_into_use_tools(
            "role",
            "x",
            &["foo".into(), "bar".into()],
            None,
            &avail,
        );
        assert_eq!(out, Some("foo:*".into()));
    }

    #[test]
    fn mcp_expansion_all_unknown_preserves_use_tools() {
        let avail = servers(&["foo"]);
        // No known servers; existing use_tools must survive untouched.
        let out = expand_mcp_servers_into_use_tools(
            "role",
            "x",
            &["bar".into()],
            Some("local"),
            &avail,
        );
        assert_eq!(out, Some("local".into()));
    }

    fn never(_: &str) -> bool { false }
    fn always(_: &str) -> bool { true }

    #[test]
    fn classify_local_prefers_role() {
        let addr = RoleAddress::parse("review").unwrap();
        let out = classify_address(&addr, always, always, always).unwrap();
        assert_eq!(out, EntityRef::Role("review".into()));
    }

    #[test]
    fn classify_local_falls_back_to_agent() {
        let addr = RoleAddress::parse("triage").unwrap();
        let out = classify_address(&addr, never, always, always).unwrap();
        assert_eq!(out, EntityRef::Agent("triage".into()));
    }

    #[test]
    fn classify_local_falls_back_to_macro() {
        let addr = RoleAddress::parse("nightly").unwrap();
        let out = classify_address(&addr, never, never, always).unwrap();
        assert_eq!(out, EntityRef::Macro("nightly".into()));
    }

    #[test]
    fn classify_local_unknown_errors() {
        let addr = RoleAddress::parse("ghost").unwrap();
        let err = classify_address(&addr, never, never, never).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ghost"), "msg = {msg}");
        assert!(msg.contains("roles"), "msg = {msg}");
        assert!(msg.contains("agents"), "msg = {msg}");
        assert!(msg.contains("macros"), "msg = {msg}");
    }

    #[test]
    fn classify_explicit_agent_prefix_skips_role_and_macro() {
        let addr = RoleAddress::parse("agent:triage").unwrap();
        // Even when a role and macro by the same name exist, the prefix is honored.
        let out = classify_address(&addr, always, always, always).unwrap();
        assert_eq!(out, EntityRef::Agent("triage".into()));
    }

    #[test]
    fn classify_explicit_agent_prefix_unknown_errors() {
        let addr = RoleAddress::parse("agent:ghost").unwrap();
        assert!(classify_address(&addr, always, never, always).is_err());
    }

    #[test]
    fn classify_explicit_macro_prefix_skips_role_and_agent() {
        let addr = RoleAddress::parse("macro:nightly").unwrap();
        let out = classify_address(&addr, always, always, always).unwrap();
        assert_eq!(out, EntityRef::Macro("nightly".into()));
    }

    #[test]
    fn classify_explicit_macro_prefix_unknown_errors() {
        let addr = RoleAddress::parse("macro:ghost").unwrap();
        assert!(classify_address(&addr, always, always, never).is_err());
    }

    #[test]
    fn classify_remote_address_with_named_target() {
        // Phase 20A: `remote:NAME/role` classifies as Remote regardless of
        // whether NAME exists in `remotes:` — the lookup happens at
        // execution time, not classification.
        let addr = RoleAddress::parse("remote:staging/review").unwrap();
        let out = classify_address(&addr, never, never, never).unwrap();
        assert_eq!(
            out,
            EntityRef::Remote {
                target: "staging".into(),
                role: "review".into()
            }
        );
    }

    #[test]
    fn classify_remote_address_with_host_port_target() {
        let addr = RoleAddress::parse("remote:host.example:8080/foo").unwrap();
        let out = classify_address(&addr, never, never, never).unwrap();
        assert_eq!(
            out,
            EntityRef::Remote {
                target: "host.example:8080".into(),
                role: "foo".into()
            }
        );
    }

    #[test]
    fn classify_mcp_address_not_implemented() {
        let addr = RoleAddress::parse("mcp:github/create_pr").unwrap();
        let err = classify_address(&addr, always, always, always).unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }

    #[test]
    fn pipeline_stage_admits_role_and_agent() {
        assert!(pipeline_stage_admissible(&EntityRef::Role("a".into())).is_ok());
        assert!(pipeline_stage_admissible(&EntityRef::Agent("b".into())).is_ok());
    }

    #[test]
    fn pipeline_stage_admits_remote() {
        // Phase 20D: remote stages are valid pipeline participants.
        let r = EntityRef::Remote {
            target: "staging".into(),
            role: "review".into(),
        };
        assert!(pipeline_stage_admissible(&r).is_ok());
    }

    #[test]
    fn pipeline_stage_rejects_macro_with_clear_error() {
        let err = pipeline_stage_admissible(&EntityRef::Macro("nightly".into())).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("nightly"), "msg = {msg}");
        assert!(msg.contains("pipeline stage"), "msg = {msg}");
        assert!(msg.contains("REPL"), "msg = {msg}");
    }

    #[test]
    fn entity_ref_kind_and_name() {
        let r = EntityRef::Role("a".into());
        assert_eq!(r.kind(), "role");
        assert_eq!(r.name(), "a");
        let a = EntityRef::Agent("b".into());
        assert_eq!(a.kind(), "agent");
        assert_eq!(a.name(), "b");
        let m = EntityRef::Macro("c".into());
        assert_eq!(m.kind(), "macro");
        assert_eq!(m.name(), "c");
        let r = EntityRef::Remote {
            target: "staging".into(),
            role: "review".into(),
        };
        assert_eq!(r.kind(), "remote");
        assert_eq!(r.name(), "review");
    }

    #[test]
    fn mcp_expansion_multiple_known_servers_join_with_comma() {
        let avail = servers(&["foo", "bar"]);
        let out = expand_mcp_servers_into_use_tools(
            "role",
            "x",
            &["foo".into(), "bar".into()],
            None,
            &avail,
        );
        assert_eq!(out, Some("foo:*,bar:*".into()));
    }
}
