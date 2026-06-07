use crate::client::Model;
use crate::config::{
    is_path_descendant, pipeline_stage_admissible, Config, EntityRef, PartialConfig, PipelineNode,
    Role, Entity,
};
use crate::function::FunctionDeclaration;

use anyhow::{bail, Result};
use std::collections::HashSet;
use std::path::Path;

/// Pre-flight validation of model capabilities against what the role/input requires.
/// Runs before any API call; all checks are deterministic and zero-token.
/// This can be thought of as the beginnings of our 'aichat' compiler, allowing us to look forward
/// at the target model before submitting the data to the backend.
/// I have an exploration of this idea in docs/analysis/2026-04-16-model-aware-compilation.md
///
/// Returns `Err` for hard mismatches (tools vs. non-function-calling model, images vs.
/// non-vision model). The caller should surface the error as a config error.
pub fn validate_model_capabilities(
    model: &Model,
    role: &Role,
    functions: Option<&[FunctionDeclaration]>,
    has_images: bool,
) -> Result<()> {
    let will_send_tools = functions.map(|f| !f.is_empty()).unwrap_or(false);
    if will_send_tools && !model.data().supports_function_calling {
        bail!(
            "Preflight: role '{}' requires tool calling but model '{}' does not support it. \
             Remove `use_tools` from the role or switch to a function-calling model.",
            role.name(),
            model.id()
        );
    }

    if has_images && !model.data().supports_vision {
        bail!(
            "Preflight: input contains images but model '{}' does not support vision. \
             Switch to a vision-capable model.",
            model.id()
        );
    }

    Ok(())
}

/// Pre-flight validation for a pipeline: each stage's role must exist and its model
/// (explicit or inherited) must support the role's requirements.
///
/// Output-schema/input-schema compatibility between stage N and stage N+1 is a
/// deterministic check we _could_ do here, but JSON-schema compatibility is subtle
/// (subset relations, anyOf/oneOf, etc.) — defer to schema validation at runtime
/// rather than duplicate the logic.
pub fn validate_pipeline_stages(
    config: &Config,
    stages: &[(String, Option<String>)],
) -> Result<()> {
    for (index, (raw_name, model_id)) in stages.iter().enumerate() {
        // Phase 19B/C: classify the stage name first. Agents and macros need
        // different handling than roles.
        let entity = config.classify_entity(raw_name).map_err(|e| {
            anyhow::anyhow!(
                "Preflight: pipeline stage {} references unknown entity '{}': {}",
                index + 1,
                raw_name,
                e
            )
        })?;
        pipeline_stage_admissible(&entity).map_err(|e| {
            anyhow::anyhow!("Preflight: pipeline stage {}: {}", index + 1, e)
        })?;

        // Phase 19C: agent-stage capability validation requires async
        // `Agent::init` and is deferred to stage execution. We've confirmed
        // the agent name exists (classification passed) — that's the
        // strongest sync check we can offer here.
        //
        // Phase 20D: remote-stage validation needs an HTTP call to the
        // remote's `/v1/roles/{name}` and is deferred to execution as well.
        // Tool / model capability checks happen on the remote side; we just
        // confirm the address parsed.
        let role_name = match &entity {
            EntityRef::Role(name) => name.clone(),
            EntityRef::Agent(_) => continue,
            EntityRef::Remote { .. } => continue,
            EntityRef::Macro(_) => unreachable!("rejected by pipeline_stage_admissible"),
        };

        let role = config.retrieve_role(&role_name).map_err(|e| {
            anyhow::anyhow!(
                "Preflight: pipeline stage {} failed to load role '{}': {}",
                index + 1,
                role_name,
                e
            )
        })?;

        let model = match model_id {
            Some(id) => {
                let listed = crate::client::list_models(config, crate::client::ModelType::Chat);
                match listed.iter().find(|m| m.id() == *id) {
                    Some(m) => (*m).clone(),
                    None => bail!(
                        "Preflight: pipeline stage {} references unknown model '{}'",
                        index + 1,
                        id
                    ),
                }
            }
            None => role.model().clone(),
        };

        if role.use_tools().is_some() && !model.data().supports_function_calling {
            bail!(
                "Preflight: pipeline stage {} role '{}' requires tool calling but model \
                 '{}' does not support it",
                index + 1,
                role_name,
                model.id()
            );
        }
    }

    Ok(())
}

/// Phase 36C: parse a `use_tools` string into `(is_all, set)`. `"all"` is the
/// wildcard sentinel; otherwise it's a comma-separated, trimmed tool list.
fn parse_tool_set(s: &str) -> (bool, HashSet<String>) {
    let trimmed = s.trim();
    if trimmed == "all" {
        return (true, HashSet::new());
    }
    let set = trimmed
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect();
    (false, set)
}

/// Phase 36C: permission-escalation guard for pipeline stage `config_override:`.
/// An override may only *narrow* what a stage can do; it can never grant a
/// permission the parent (pipeline-owning) role lacks. This closes the
/// cross-stage privilege-contamination gap (divergence Theme 9).
///
/// Rules:
/// - `use_tools` — the override's tool set must be a subset of the parent's.
///   Parent `"all"` permits any narrowing; an override of `"all"` when the
///   parent is not `"all"` is rejected.
/// - `mcp_servers` — only `[]` (disable all) is supported; a non-empty list is
///   rejected ("not supported; use `[]`").
/// - `working_directory` — must resolve to a descendant of the parent's working
///   directory tree; a `..`-escape is rejected. (Symlink resolution is out of
///   scope — roles are trusted code; this guards accidents.)
/// - `temperature` / `top_p` / `max_output_tokens` — tuning knobs, no check.
///
/// `parent_cwd` is the parent's effective working directory (its
/// `config_override`-free cwd — normally aichat's process cwd). `stages` is the
/// list of `(zero-based stage index, stage role name, override)` for every
/// stage that declared a `config_override:`.
pub fn validate_pipeline_overrides(
    parent_role_name: &str,
    parent_use_tools: Option<&str>,
    parent_cwd: &Path,
    stages: &[(usize, String, PartialConfig)],
) -> Result<()> {
    for (index, stage_role, ov) in stages {
        let stage_no = index + 1;

        if let Some(child_tools) = &ov.use_tools {
            let (parent_all, parent_set) = match parent_use_tools {
                Some(s) => parse_tool_set(s),
                None => (false, HashSet::new()),
            };
            let (child_all, child_set) = parse_tool_set(child_tools);
            if !parent_all {
                if child_all {
                    bail!(
                        "Preflight: pipeline stage {stage_no} ('{stage_role}') config_override \
                         grants use_tools 'all', but parent role '{parent_role_name}' does not \
                         grant all tools. Overrides may only narrow tool permissions, never \
                         escalate them.\n  hint: set the stage's use_tools to a subset of the \
                         parent's, or grant 'all' on the parent role."
                    );
                }
                let mut extra: Vec<String> =
                    child_set.difference(&parent_set).cloned().collect();
                if !extra.is_empty() {
                    extra.sort();
                    let mut parent_list: Vec<String> = parent_set.into_iter().collect();
                    parent_list.sort();
                    bail!(
                        "Preflight: pipeline stage {stage_no} ('{stage_role}') config_override \
                         grants use_tools [{}] not held by parent role '{parent_role_name}' \
                         (parent grants: [{}]). Overrides may only narrow tool permissions, \
                         never escalate them.\n  hint: add the tool(s) to the parent role's \
                         use_tools, or remove them from the stage override.",
                        extra.join(", "),
                        parent_list.join(", "),
                    );
                }
            }
        }

        if let Some(servers) = &ov.mcp_servers {
            if !servers.is_empty() {
                bail!(
                    "Preflight: pipeline stage {stage_no} ('{stage_role}') config_override.\
                     mcp_servers re-selection is not supported in this release; use `[]` to \
                     disable all MCP servers for the stage."
                );
            }
        }

        if let Some(wd) = &ov.working_directory {
            if !is_path_descendant(parent_cwd, wd) {
                bail!(
                    "Preflight: pipeline stage {stage_no} ('{stage_role}') config_override.\
                     working_directory '{}' escapes the parent working directory tree '{}'. \
                     A stage may only narrow into a descendant directory.\n  hint: use a path \
                     within '{}'.",
                    wd.display(),
                    parent_cwd.display(),
                    parent_cwd.display(),
                );
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 15B: cross-stage JSON Schema containment.
//
// Pipeline stage N validates its output against `output_schema`; stage N+1
// validates its input against `input_schema`. The boundary is *compatible*
// when every document valid under N's `output_schema` is also valid under
// N+1's `input_schema` — i.e. the output schema is a SUBSET of (is contained
// by) the input schema. This is deterministic and zero-token: we never call a
// model, we reason about the declared schemas directly.
//
// The check is intentionally conservative. JSON Schema is expressive enough
// (anyOf/oneOf/allOf/$ref/not) that exact containment is undecidable in the
// general case; rather than risk false failures we analyze the common,
// decidable shapes (objects with `properties`/`required`, scalar `type`s,
// arrays) and return `Unknown` for anything we cannot prove either way. Only
// a PROVABLE violation produces `Fail`.
// ---------------------------------------------------------------------------

/// Outcome of a single output→input boundary containment check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainmentVerdict {
    /// Provably (or confidently) compatible.
    Ok,
    /// Provable containment violation — the downstream stage will reject some
    /// outputs the upstream stage is allowed to produce.
    Fail,
    /// A likely problem that is not a provable failure (e.g. a free-text
    /// upstream feeding a structured downstream). Surfaced but does not by
    /// itself fail a `--check`.
    Warn,
    /// Schema shape too complex to analyze statically.
    Unknown,
}

/// Structured result of [`schema_containment`].
#[derive(Debug, Clone, PartialEq)]
pub struct Containment {
    pub verdict: ContainmentVerdict,
    /// Fields the consumer requires that the producer does not guarantee.
    pub missing: Vec<String>,
    /// Fields the producer can emit that the consumer does not declare
    /// (informational unless the consumer forbids additional properties).
    pub extra: Vec<String>,
    /// Subset of `extra` that the consumer actively forbids
    /// (`additionalProperties: false`). These are hard failures.
    pub forbidden: Vec<String>,
    /// `(field, producer_type, consumer_type)` where declared types conflict.
    /// The field name `(root)` denotes a top-level type mismatch.
    pub type_mismatches: Vec<(String, String, String)>,
    /// Human-readable explanations (free-text upstream, unknown shapes, …).
    pub notes: Vec<String>,
}

impl Containment {
    fn empty(verdict: ContainmentVerdict) -> Self {
        Containment {
            verdict,
            missing: Vec::new(),
            extra: Vec::new(),
            forbidden: Vec::new(),
            type_mismatches: Vec::new(),
            notes: Vec::new(),
        }
    }
}

/// Phase 15B: does a document conforming to `producer` always conform to
/// `consumer`? `None` means "no schema declared" — an absent consumer accepts
/// anything; an absent producer emits free text.
pub fn schema_containment(
    producer: Option<&serde_json::Value>,
    consumer: Option<&serde_json::Value>,
) -> Containment {
    use serde_json::Value;

    // An absent consumer input_schema accepts any input — always compatible.
    let consumer = match consumer {
        None => return Containment::empty(ContainmentVerdict::Ok),
        Some(s) => s,
    };
    // An absent producer output_schema means the stage emits free text. The
    // text might happen to be JSON conforming to the consumer, but nothing
    // guarantees it — warn rather than fail.
    let producer = match producer {
        None => {
            let mut c = Containment::empty(ContainmentVerdict::Warn);
            c.notes.push(
                "upstream stage emits free text (no output_schema); downstream \
                 input_schema may reject non-JSON output"
                    .to_string(),
            );
            return c;
        }
        Some(s) => s,
    };

    // Combinators make exact containment undecidable for our purposes; don't
    // guess — report Unknown so callers fall back to runtime validation.
    if has_complex_combinators(producer) || has_complex_combinators(consumer) {
        let mut c = Containment::empty(ContainmentVerdict::Unknown);
        c.notes.push(
            "schema uses anyOf/oneOf/allOf/$ref/not; static containment not attempted"
                .to_string(),
        );
        return c;
    }

    let mut c = Containment::empty(ContainmentVerdict::Ok);

    // Top-level type compatibility. Only checkable when both declare a `type`.
    let producer_types = schema_type_set(producer);
    let consumer_types = schema_type_set(consumer);
    if let (Some(pt), Some(ct)) = (&producer_types, &consumer_types) {
        if !types_compatible(pt, ct) {
            c.type_mismatches
                .push(("(root)".to_string(), pt.join("|"), ct.join("|")));
            c.verdict = ContainmentVerdict::Fail;
            return c;
        }
    }

    // Field-level analysis applies only when the consumer expects an object.
    let consumer_is_object = consumer_types
        .as_ref()
        .map(|t| t.iter().any(|x| x == "object"))
        .unwrap_or_else(|| consumer.get("properties").is_some());

    if consumer_is_object {
        let cons_props = consumer.get("properties").and_then(Value::as_object);
        // Ordered list so report output is deterministic (matches the schema's
        // declared `required` order); HashSet for producer membership tests.
        let cons_required = required_list(consumer);
        let prod_props = producer.get("properties").and_then(Value::as_object);
        let prod_required = required_set(producer);
        let consumer_forbids_additional =
            consumer.get("additionalProperties").and_then(Value::as_bool) == Some(false);

        // Missing: a consumer-required field the producer does not guarantee.
        // The producer guarantees a field only if it is in the producer's own
        // `required` list — an optional field may be omitted by a conforming
        // document, breaking containment.
        for field in &cons_required {
            if !prod_required.contains(field) {
                c.missing.push(field.clone());
            }
        }

        // Type mismatches on fields declared by both sides.
        if let (Some(pp), Some(cp)) = (prod_props, cons_props) {
            for (name, cons_field) in cp {
                if let Some(prod_field) = pp.get(name) {
                    if let (Some(pt), Some(ct)) =
                        (schema_type_set(prod_field), schema_type_set(cons_field))
                    {
                        if !types_compatible(&pt, &ct) {
                            c.type_mismatches
                                .push((name.clone(), pt.join("|"), ct.join("|")));
                        }
                    }
                }
            }
        }

        // Extras: fields the producer can emit that the consumer does not
        // declare. Informational unless the consumer forbids additional
        // properties, in which case they are hard failures.
        if let Some(pp) = prod_props {
            for name in pp.keys() {
                let declared_by_consumer =
                    cons_props.map(|cp| cp.contains_key(name)).unwrap_or(false);
                if !declared_by_consumer {
                    c.extra.push(name.clone());
                    if consumer_forbids_additional {
                        c.forbidden.push(name.clone());
                    }
                }
            }
        }
    }

    if !c.missing.is_empty() || !c.type_mismatches.is_empty() || !c.forbidden.is_empty() {
        c.verdict = ContainmentVerdict::Fail;
    }
    c
}

/// Top-level combinator keys we decline to reason about statically.
fn has_complex_combinators(schema: &serde_json::Value) -> bool {
    match schema.as_object() {
        Some(obj) => ["anyOf", "oneOf", "allOf", "$ref", "not"]
            .iter()
            .any(|k| obj.contains_key(*k)),
        None => false,
    }
}

/// Normalize a schema's `type` keyword into a set of type names. Returns
/// `None` when no `type` is declared (an open shape we cannot constrain).
fn schema_type_set(schema: &serde_json::Value) -> Option<Vec<String>> {
    match schema.get("type") {
        Some(serde_json::Value::String(s)) => Some(vec![s.clone()]),
        Some(serde_json::Value::Array(arr)) => {
            let v: Vec<String> = arr
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect();
            if v.is_empty() {
                None
            } else {
                Some(v)
            }
        }
        _ => None,
    }
}

fn required_set(schema: &serde_json::Value) -> std::collections::HashSet<String> {
    required_list(schema).into_iter().collect()
}

/// The `required` array as an ordered list (preserves declared order for
/// deterministic reporting).
fn required_list(schema: &serde_json::Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// A producer value of type `prod` is accepted where `cons` is expected when
/// `prod == cons`, or `prod` is an `integer` and `cons` is a `number`
/// (every integer is a number).
fn type_allows(cons: &str, prod: &str) -> bool {
    cons == prod || (cons == "number" && prod == "integer")
}

/// Producer types are compatible with consumer types when every type the
/// producer may emit is accepted by some consumer type.
fn types_compatible(producer: &[String], consumer: &[String]) -> bool {
    producer
        .iter()
        .all(|p| consumer.iter().any(|c| type_allows(c, p)))
}

/// Phase 15B/15C: containment outcome for one stage→stage boundary, carrying
/// enough context for `--check` to render the design's report format.
#[derive(Debug, Clone)]
pub struct BoundaryReport {
    /// 1-based position of the upstream stage in the sequential stage list.
    pub from_pos: usize,
    pub from_role: String,
    /// 1-based position of the downstream stage.
    pub to_pos: usize,
    pub to_role: String,
    /// `Some(reason)` when the boundary could not be analyzed statically — an
    /// agent/remote/macro stage, or a role that failed to load. Containment is
    /// then meaningless and `--check` does not fail on it.
    pub skipped: Option<String>,
    /// Containment result; meaningful only when `skipped` is `None`.
    pub containment: Containment,
}

/// One side of a boundary: either a resolved role's schema (input or output),
/// or a reason the stage can't be analyzed.
enum StageSchema {
    Schema(Option<serde_json::Value>),
    Skip(String),
}

fn stage_schema(config: &Config, name: &str, want_output: bool) -> StageSchema {
    let entity = match config.classify_entity(name) {
        Ok(e) => e,
        Err(_) => return StageSchema::Skip(format!("stage '{name}' is unresolved")),
    };
    let role_name = match entity {
        EntityRef::Role(n) => n,
        EntityRef::Agent(_) => {
            return StageSchema::Skip(format!(
                "stage '{name}' is an agent (no static schema introspection)"
            ))
        }
        EntityRef::Remote { .. } => {
            return StageSchema::Skip(format!(
                "stage '{name}' is remote (schema lives on the remote)"
            ))
        }
        EntityRef::Macro(_) => {
            return StageSchema::Skip(format!("stage '{name}' is a macro"))
        }
    };
    match config.retrieve_role(&role_name) {
        Ok(role) => {
            let schema = if want_output {
                role.output_schema().cloned()
            } else {
                role.input_schema().cloned()
            };
            StageSchema::Schema(schema)
        }
        Err(e) => StageSchema::Skip(format!("role '{role_name}' failed to load: {e}")),
    }
}

/// Phase 15B/15C: walk a purely sequential stage list and run a containment
/// check at every adjacent boundary (output of stage N vs input of stage N+1).
/// Returns one [`BoundaryReport`] per boundary. Does not bail — the caller
/// (`--check`) decides how to surface failures. Stages that aren't roles
/// (agents/remotes/macros) produce a `skipped` boundary.
pub fn validate_pipeline_schema_containment(
    config: &Config,
    stages: &[(String, Option<String>)],
) -> Vec<BoundaryReport> {
    let mut reports = Vec::with_capacity(stages.len().saturating_sub(1));
    for (i, window) in stages.windows(2).enumerate() {
        let from = &window[0].0;
        let to = &window[1].0;
        let from_pos = i + 1;
        let to_pos = i + 2;

        let producer = stage_schema(config, from, true);
        let consumer = stage_schema(config, to, false);
        match (producer, consumer) {
            (StageSchema::Schema(out), StageSchema::Schema(inp)) => {
                reports.push(BoundaryReport {
                    from_pos,
                    from_role: from.clone(),
                    to_pos,
                    to_role: to.clone(),
                    skipped: None,
                    containment: schema_containment(out.as_ref(), inp.as_ref()),
                });
            }
            (StageSchema::Skip(reason), _) | (_, StageSchema::Skip(reason)) => {
                reports.push(BoundaryReport {
                    from_pos,
                    from_role: from.clone(),
                    to_pos,
                    to_role: to.clone(),
                    skipped: Some(reason),
                    containment: Containment::empty(ContainmentVerdict::Unknown),
                });
            }
        }
    }
    reports
}

/// Phase 33D: render one failed boundary for the preflight bail message,
/// mirroring the `--check` FAIL layout (missing / type-mismatch / forbidden).
fn format_boundary_failure(b: &BoundaryReport) -> String {
    let c = &b.containment;
    let mut s = format!(
        "  stage {} ({}) → stage {} ({})",
        b.from_pos, b.from_role, b.to_pos, b.to_role
    );
    if !c.missing.is_empty() {
        s.push_str(&format!(
            "\n    Missing (downstream requires, upstream does not provide): {}",
            c.missing.join(", ")
        ));
    }
    for (field, prod, cons) in &c.type_mismatches {
        s.push_str(&format!(
            "\n    Type mismatch on '{field}': upstream {prod} vs downstream {cons}"
        ));
    }
    if !c.forbidden.is_empty() {
        s.push_str(&format!(
            "\n    Forbidden (downstream additionalProperties: false): {}",
            c.forbidden.join(", ")
        ));
    }
    s.push_str(
        "\n    Fix: add a transform stage, or align the schemas so the upstream \
         output satisfies the downstream input.",
    );
    s
}

/// Phase 33D: turn the Phase 15B containment reports into an execution-time
/// policy. `Fail` boundaries (both stages declare schemas and the downstream
/// would provably reject some upstream output) bail preflight with a teaching
/// diff. `Warn` boundaries (e.g. a free-text upstream feeding a structured
/// downstream — one side undeclared) are surfaced as warnings but do not block.
/// `Ok` / `Unknown` / statically-unanalyzable (`skipped`) boundaries pass.
pub fn enforce_pipeline_shape(reports: &[BoundaryReport]) -> Result<()> {
    let mut failures: Vec<String> = Vec::new();
    for b in reports {
        if b.skipped.is_some() {
            continue;
        }
        match b.containment.verdict {
            ContainmentVerdict::Fail => failures.push(format_boundary_failure(b)),
            ContainmentVerdict::Warn => {
                if b.containment.notes.is_empty() {
                    warn!(
                        "Pipeline shape: stage {} ({}) → stage {} ({}): possible mismatch",
                        b.from_pos, b.from_role, b.to_pos, b.to_role
                    );
                } else {
                    for n in &b.containment.notes {
                        warn!(
                            "Pipeline shape: stage {} ({}) → stage {} ({}): {}",
                            b.from_pos, b.from_role, b.to_pos, b.to_role, n
                        );
                    }
                }
            }
            ContainmentVerdict::Ok | ContainmentVerdict::Unknown => {}
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        bail!(
            "Preflight: pipeline shape mismatch — the value flow has nowhere to land:\n{}",
            failures.join("\n")
        );
    }
}

/// Phase 33D: execution-time adjacent-stage shape check for a purely sequential
/// pipeline. Computes the Phase 15B containment at each boundary, then applies
/// the [`enforce_pipeline_shape`] policy (strict fail / soft warn). Non-sequential
/// DAGs are checked structurally elsewhere; cross-branch shape checking is out of
/// scope, so callers pass only the sequential leaf list (or skip).
pub fn validate_pipeline_shape(
    config: &Config,
    stages: &[(String, Option<String>)],
) -> Result<()> {
    let reports = validate_pipeline_schema_containment(config, stages);
    enforce_pipeline_shape(&reports)
}

/// Phase 21D: detect cycles in the pipeline-role reference graph.
/// A pipeline role A whose stages reference another pipeline role B
/// (which itself references A, directly or transitively) would loop
/// infinitely through tool dispatch. Catch the cycle deterministically
/// at preflight before any LLM call.
///
/// `entry` is the name of the role whose pipeline we're about to run.
/// `nodes` is its DAG. We walk every leaf stage; if the stage resolves
/// to another pipeline role, we recurse into that role's pipeline,
/// extending the visit chain. Repeating a name → cycle.
pub fn validate_pipeline_dag_cycles(
    config: &Config,
    entry: &str,
    nodes: &[PipelineNode],
) -> Result<()> {
    let mut chain: Vec<String> = vec![entry.to_string()];
    walk_pipeline_nodes(config, nodes, &mut chain)
}

fn walk_pipeline_nodes(
    config: &Config,
    nodes: &[PipelineNode],
    chain: &mut Vec<String>,
) -> Result<()> {
    for n in nodes {
        for stage in n.all_stages() {
            check_stage_for_cycle(config, &stage.role, chain)?;
        }
        for merger in n.merge_role_names() {
            check_stage_for_cycle(config, &merger, chain)?;
        }
    }
    Ok(())
}

fn check_stage_for_cycle(
    config: &Config,
    stage_role: &str,
    chain: &mut Vec<String>,
) -> Result<()> {
    // Reuse the role classifier so we don't double-error on agents/macros.
    // Pipeline-role cycles only apply to actual roles — agents have their
    // own tool semantics and macros aren't admissible as pipeline stages.
    let entity = match config.classify_entity(stage_role) {
        Ok(e) => e,
        Err(_) => return Ok(()), // unknown — surfaced separately by validate_pipeline_stages
    };
    let resolved_role_name = match entity {
        EntityRef::Role(name) => name,
        _ => return Ok(()),
    };

    if chain.iter().any(|s| s == &resolved_role_name) {
        let mut path = chain.clone();
        path.push(resolved_role_name.clone());
        bail!(
            "Preflight: pipeline cycle detected — {} (a role's pipeline cannot \
             transitively reference itself)",
            path.join(" -> ")
        );
    }

    let role = match config.retrieve_role(&resolved_role_name) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    if !role.is_pipeline() {
        return Ok(());
    }
    let nodes = match role.pipeline() {
        Some(n) => n.to_vec(),
        None => return Ok(()),
    };

    chain.push(resolved_role_name);
    let res = walk_pipeline_nodes(config, &nodes, chain);
    chain.pop();
    res
}

/// Phase 21D: walk the DAG and ensure every node's structural invariants
/// hold (delegates to `PipelineNode::structural_check`) and that no
/// switch declares dead branches. Currently `structural_check` covers
/// the empty-branches / double-otherwise cases; we additionally detect
/// `when:` branches placed *after* an `otherwise:` and warn — the
/// runtime order-evaluation makes them reachable, but YAML readers tend
/// to assume order-matters, and putting otherwise last is the
/// universally-clear pattern.
pub fn validate_pipeline_dag_structure(nodes: &[PipelineNode]) -> Result<()> {
    let mut seen: HashSet<usize> = HashSet::new();
    for (i, n) in nodes.iter().enumerate() {
        n.structural_check()?;
        if !seen.insert(i) {
            // Defensive — indexes are unique by construction.
        }
        check_switch_branch_order(n)?;
    }
    Ok(())
}

fn check_switch_branch_order(n: &PipelineNode) -> Result<()> {
    match n {
        PipelineNode::Stage(_) => Ok(()),
        PipelineNode::Parallel(p) => {
            for b in &p.branches {
                check_switch_branch_order(b)?;
            }
            Ok(())
        }
        PipelineNode::Switch(s) => {
            let mut saw_otherwise = false;
            for b in &s.branches {
                if saw_otherwise && b.predicate.is_some() {
                    bail!(
                        "Switch branch order is misleading: a `when:` clause \
                         appears after `otherwise:`. Move `otherwise:` to the \
                         last position so reading order matches evaluation."
                    );
                }
                if b.predicate.is_none() {
                    saw_otherwise = true;
                }
                check_switch_branch_order(&b.node)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ---- Phase 36C: validate_pipeline_overrides escalation guard ----

    fn ov_tools(tools: &str) -> PartialConfig {
        PartialConfig {
            use_tools: Some(tools.to_string()),
            ..Default::default()
        }
    }

    fn run_guard(parent_tools: Option<&str>, stage_ov: PartialConfig) -> Result<()> {
        validate_pipeline_overrides(
            "parent",
            parent_tools,
            Path::new("/home/u/proj"),
            &[(0, "stage".to_string(), stage_ov)],
        )
    }

    #[test]
    fn escalation_use_tools_rejected() {
        let err = run_guard(Some("fs_read,grep"), ov_tools("run_command")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("run_command"), "got: {msg}");
        assert!(msg.contains("narrow"), "got: {msg}");
    }

    #[test]
    fn narrowing_use_tools_accepted() {
        run_guard(Some("fs_read,fs_write,grep"), ov_tools("fs_read,grep")).unwrap();
        // Narrowing to no tools is allowed.
        run_guard(Some("fs_read"), ov_tools("")).unwrap();
    }

    #[test]
    fn escalation_use_tools_all_rejected_when_parent_not_all() {
        let err = run_guard(Some("fs_read"), ov_tools("all")).unwrap_err();
        assert!(err.to_string().contains("'all'"), "got: {err}");
    }

    #[test]
    fn parent_all_allows_any_override() {
        run_guard(Some("all"), ov_tools("fs_read,run_command")).unwrap();
        run_guard(Some("all"), ov_tools("all")).unwrap();
    }

    #[test]
    fn parent_none_rejects_any_tool_grant() {
        let err = run_guard(None, ov_tools("fs_read")).unwrap_err();
        assert!(err.to_string().contains("fs_read"), "got: {err}");
    }

    #[test]
    fn mcp_disable_allowed_nonempty_rejected() {
        let disable = PartialConfig {
            mcp_servers: Some(vec![]),
            ..Default::default()
        };
        run_guard(Some("all"), disable).unwrap();
        let reselect = PartialConfig {
            mcp_servers: Some(vec!["srv".into()]),
            ..Default::default()
        };
        let err = run_guard(Some("all"), reselect).unwrap_err();
        assert!(err.to_string().contains("not supported"), "got: {err}");
    }

    #[test]
    fn working_directory_descendant_accepted_escape_rejected() {
        let ok = PartialConfig {
            working_directory: Some(PathBuf::from("scratch/sub")),
            ..Default::default()
        };
        run_guard(Some("all"), ok).unwrap();
        let escape = PartialConfig {
            working_directory: Some(PathBuf::from("../../etc")),
            ..Default::default()
        };
        let err = run_guard(Some("all"), escape).unwrap_err();
        assert!(err.to_string().contains("escapes"), "got: {err}");
    }

    #[test]
    fn empty_overrides_list_is_noop() {
        validate_pipeline_overrides("parent", Some("fs_read"), Path::new("/x"), &[]).unwrap();
    }

    // ---- Phase 33D: enforce_pipeline_shape policy ----

    fn boundary(
        verdict: ContainmentVerdict,
        missing: &[&str],
        skipped: Option<&str>,
    ) -> BoundaryReport {
        BoundaryReport {
            from_pos: 1,
            from_role: "extract".into(),
            to_pos: 2,
            to_role: "review".into(),
            skipped: skipped.map(str::to_string),
            containment: Containment {
                verdict,
                missing: missing.iter().map(|s| s.to_string()).collect(),
                extra: vec![],
                forbidden: vec![],
                type_mismatches: vec![],
                notes: vec![],
            },
        }
    }

    #[test]
    fn enforce_shape_fails_on_provable_mismatch() {
        let reports = [boundary(ContainmentVerdict::Fail, &["summary"], None)];
        let err = enforce_pipeline_shape(&reports).unwrap_err().to_string();
        assert!(err.contains("shape mismatch"), "{err}");
        assert!(err.contains("summary"), "names the missing field: {err}");
        assert!(err.contains("extract") && err.contains("review"), "names the boundary: {err}");
    }

    #[test]
    fn enforce_shape_warns_but_passes_on_soft_mismatch() {
        let reports = [boundary(ContainmentVerdict::Warn, &[], None)];
        assert!(enforce_pipeline_shape(&reports).is_ok(), "Warn is soft, must not fail");
    }

    #[test]
    fn enforce_shape_passes_ok_and_unknown() {
        let reports = [
            boundary(ContainmentVerdict::Ok, &[], None),
            boundary(ContainmentVerdict::Unknown, &[], None),
        ];
        assert!(enforce_pipeline_shape(&reports).is_ok());
    }

    #[test]
    fn enforce_shape_ignores_skipped_even_if_fail() {
        // A boundary that couldn't be analyzed statically (agent/remote) is
        // marked skipped; it must never block, regardless of verdict.
        let reports = [boundary(ContainmentVerdict::Fail, &["x"], Some("stage is an agent"))];
        assert!(enforce_pipeline_shape(&reports).is_ok());
    }

    #[test]
    fn enforce_shape_empty_passes() {
        assert!(enforce_pipeline_shape(&[]).is_ok());
    }

    fn model_with(tools: bool, vision: bool) -> Model {
        let mut m = Model::new("test", "m");
        m.data_mut().supports_function_calling = tools;
        m.data_mut().supports_vision = vision;
        m
    }

    fn one_tool() -> Vec<FunctionDeclaration> {
        vec![FunctionDeclaration::tool_search()]
    }

    #[test]
    fn passes_when_no_tools_and_no_images() {
        let m = model_with(false, false);
        let r = Role::default();
        assert!(validate_model_capabilities(&m, &r, None, false).is_ok());
    }

    #[test]
    fn rejects_tools_on_non_function_calling_model() {
        let m = model_with(false, false);
        let r = Role::default();
        let decls = one_tool();
        let err = validate_model_capabilities(&m, &r, Some(&decls), false).unwrap_err();
        assert!(err.to_string().contains("does not support it"));
    }

    #[test]
    fn accepts_tools_on_function_calling_model() {
        let m = model_with(true, false);
        let r = Role::default();
        let decls = one_tool();
        assert!(validate_model_capabilities(&m, &r, Some(&decls), false).is_ok());
    }

    #[test]
    fn rejects_images_on_non_vision_model() {
        let m = model_with(false, false);
        let r = Role::default();
        let err = validate_model_capabilities(&m, &r, None, true).unwrap_err();
        assert!(err.to_string().contains("does not support vision"));
    }

    #[test]
    fn accepts_images_on_vision_model() {
        let m = model_with(false, true);
        let r = Role::default();
        assert!(validate_model_capabilities(&m, &r, None, true).is_ok());
    }

    // ----- Phase 21D: DAG structural validation -----

    fn yaml_node(yaml: &str) -> PipelineNode {
        let v: serde_json::Value = serde_norway::from_str(yaml).unwrap();
        crate::config::role::parse_pipeline_node(&v).unwrap()
    }

    #[test]
    fn dag_structural_rejects_when_after_otherwise() {
        let n = yaml_node(
            r#"
switch:
  - when: { contains: "x" }
    role: a
  - otherwise: true
    role: b
  - when: { contains: "y" }
    role: c
"#,
        );
        let err = validate_pipeline_dag_structure(&[n]).unwrap_err();
        assert!(err.to_string().contains("after `otherwise:`"));
    }

    #[test]
    fn dag_structural_accepts_otherwise_last() {
        let n = yaml_node(
            r#"
switch:
  - when: { contains: "x" }
    role: a
  - when: { contains: "y" }
    role: b
  - otherwise: true
    role: c
"#,
        );
        assert!(validate_pipeline_dag_structure(&[n]).is_ok());
    }

    // ----- Phase 15B: cross-stage schema containment -----

    fn schema(json: &str) -> serde_json::Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn containment_absent_consumer_accepts_anything() {
        let producer = schema(r#"{"type":"object","properties":{"x":{"type":"string"}}}"#);
        let c = schema_containment(Some(&producer), None);
        assert_eq!(c.verdict, ContainmentVerdict::Ok);
    }

    #[test]
    fn containment_both_absent_is_ok() {
        let c = schema_containment(None, None);
        assert_eq!(c.verdict, ContainmentVerdict::Ok);
    }

    #[test]
    fn containment_free_text_into_structured_warns() {
        // Producer has no output_schema → emits free text. Consumer expects an
        // object. Not a provable failure (the text might be JSON), but worth a
        // warning.
        let consumer = schema(r#"{"type":"object","properties":{"a":{"type":"string"}},"required":["a"]}"#);
        let c = schema_containment(None, Some(&consumer));
        assert_eq!(c.verdict, ContainmentVerdict::Warn);
        assert!(c.notes.iter().any(|n| n.contains("free text")));
    }

    #[test]
    fn containment_compatible_objects_ok() {
        let producer = schema(
            r#"{"type":"object","properties":{"issues":{"type":"array"},"severity":{"type":"string"}},"required":["issues","severity"]}"#,
        );
        let consumer = schema(
            r#"{"type":"object","properties":{"issues":{"type":"array"}},"required":["issues"]}"#,
        );
        let c = schema_containment(Some(&producer), Some(&consumer));
        assert_eq!(c.verdict, ContainmentVerdict::Ok, "got {c:?}");
    }

    #[test]
    fn containment_missing_required_field_fails() {
        // The design example: producer {text, metadata}; consumer requires
        // {content, language}.
        let producer = schema(
            r#"{"type":"object","properties":{"text":{"type":"string"},"metadata":{"type":"object"}},"required":["text","metadata"]}"#,
        );
        let consumer = schema(
            r#"{"type":"object","properties":{"content":{"type":"string"},"language":{"type":"string"}},"required":["content","language"]}"#,
        );
        let c = schema_containment(Some(&producer), Some(&consumer));
        assert_eq!(c.verdict, ContainmentVerdict::Fail);
        assert!(c.missing.contains(&"content".to_string()));
        assert!(c.missing.contains(&"language".to_string()));
        assert!(c.extra.contains(&"text".to_string()));
        assert!(c.extra.contains(&"metadata".to_string()));
    }

    #[test]
    fn containment_required_but_optional_upstream_fails() {
        // Consumer requires `id`; producer declares `id` but does not require
        // it, so a producer-valid document may omit `id`. Strict containment
        // violation.
        let producer = schema(r#"{"type":"object","properties":{"id":{"type":"string"}}}"#);
        let consumer = schema(
            r#"{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}"#,
        );
        let c = schema_containment(Some(&producer), Some(&consumer));
        assert_eq!(c.verdict, ContainmentVerdict::Fail);
        assert!(c.missing.contains(&"id".to_string()));
    }

    #[test]
    fn containment_type_mismatch_on_shared_field_fails() {
        let producer = schema(
            r#"{"type":"object","properties":{"count":{"type":"string"}},"required":["count"]}"#,
        );
        let consumer = schema(
            r#"{"type":"object","properties":{"count":{"type":"integer"}},"required":["count"]}"#,
        );
        let c = schema_containment(Some(&producer), Some(&consumer));
        assert_eq!(c.verdict, ContainmentVerdict::Fail);
        assert!(c
            .type_mismatches
            .iter()
            .any(|(f, _, _)| f == "count"));
    }

    #[test]
    fn containment_top_level_type_mismatch_fails() {
        let producer = schema(r#"{"type":"object","properties":{}}"#);
        let consumer = schema(r#"{"type":"string"}"#);
        let c = schema_containment(Some(&producer), Some(&consumer));
        assert_eq!(c.verdict, ContainmentVerdict::Fail);
        assert!(c
            .type_mismatches
            .iter()
            .any(|(f, _, _)| f == "(root)"));
    }

    #[test]
    fn containment_extra_field_ok_when_additional_allowed() {
        // Producer emits an extra field; consumer is open (additionalProperties
        // not false) → still compatible, extra is informational only.
        let producer = schema(
            r#"{"type":"object","properties":{"a":{"type":"string"},"b":{"type":"string"}},"required":["a"]}"#,
        );
        let consumer = schema(
            r#"{"type":"object","properties":{"a":{"type":"string"}},"required":["a"]}"#,
        );
        let c = schema_containment(Some(&producer), Some(&consumer));
        assert_eq!(c.verdict, ContainmentVerdict::Ok, "got {c:?}");
        assert!(c.extra.contains(&"b".to_string()));
        assert!(c.forbidden.is_empty());
    }

    #[test]
    fn containment_forbidden_extra_fails_when_additional_false() {
        let producer = schema(
            r#"{"type":"object","properties":{"a":{"type":"string"},"b":{"type":"string"}},"required":["a"]}"#,
        );
        let consumer = schema(
            r#"{"type":"object","properties":{"a":{"type":"string"}},"required":["a"],"additionalProperties":false}"#,
        );
        let c = schema_containment(Some(&producer), Some(&consumer));
        assert_eq!(c.verdict, ContainmentVerdict::Fail);
        assert!(c.forbidden.contains(&"b".to_string()));
    }

    #[test]
    fn containment_integer_is_a_number() {
        // Producer emits integers; consumer accepts numbers → compatible.
        let producer = schema(
            r#"{"type":"object","properties":{"n":{"type":"integer"}},"required":["n"]}"#,
        );
        let consumer = schema(
            r#"{"type":"object","properties":{"n":{"type":"number"}},"required":["n"]}"#,
        );
        let c = schema_containment(Some(&producer), Some(&consumer));
        assert_eq!(c.verdict, ContainmentVerdict::Ok, "got {c:?}");
    }

    #[test]
    fn containment_number_into_integer_fails() {
        // Reverse: producer emits arbitrary numbers, consumer wants integers.
        let producer = schema(
            r#"{"type":"object","properties":{"n":{"type":"number"}},"required":["n"]}"#,
        );
        let consumer = schema(
            r#"{"type":"object","properties":{"n":{"type":"integer"}},"required":["n"]}"#,
        );
        let c = schema_containment(Some(&producer), Some(&consumer));
        assert_eq!(c.verdict, ContainmentVerdict::Fail);
    }

    #[test]
    fn containment_complex_combinator_is_unknown() {
        let producer = schema(r#"{"type":"object","properties":{"a":{"type":"string"}}}"#);
        let consumer = schema(r#"{"anyOf":[{"type":"object"},{"type":"string"}]}"#);
        let c = schema_containment(Some(&producer), Some(&consumer));
        assert_eq!(c.verdict, ContainmentVerdict::Unknown);
    }

    #[test]
    fn containment_open_object_consumer_accepts_any_object() {
        // Consumer is just `{type: object}` — accepts any object regardless of
        // shape. A producer object is fine; its fields are listed as extras.
        let producer = schema(
            r#"{"type":"object","properties":{"a":{"type":"string"}},"required":["a"]}"#,
        );
        let consumer = schema(r#"{"type":"object"}"#);
        let c = schema_containment(Some(&producer), Some(&consumer));
        assert_eq!(c.verdict, ContainmentVerdict::Ok, "got {c:?}");
    }

    #[test]
    fn dag_structural_recurses_into_parallel_branches() {
        let n = yaml_node(
            r#"
parallel:
  - role: a
  - switch:
      - when: { contains: "x" }
        role: b
      - otherwise: true
        role: c
      - when: { contains: "y" }
        role: d
"#,
        );
        let err = validate_pipeline_dag_structure(&[n]).unwrap_err();
        assert!(err.to_string().contains("after `otherwise:`"));
    }
}
