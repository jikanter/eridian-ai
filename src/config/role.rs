use super::*;

use crate::client::{Message, MessageContent, MessageRole, Model};

use anyhow::{Context, Result};
use fancy_regex::Regex;
use indexmap::IndexMap;
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::read_to_string;
use std::sync::LazyLock;

pub const SHELL_ROLE: &str = "%shell%";
pub const EXPLAIN_SHELL_ROLE: &str = "%explain-shell%";
pub const CODE_ROLE: &str = "%code%";
pub const CREATE_TITLE_ROLE: &str = "%create-title%";

pub const INPUT_PLACEHOLDER: &str = "__INPUT__";

/// Leading text of the schema suffix injected into the system message when a
/// role has `output_schema`. Exposed so the Phase 9A/9B native-structured-output
/// path can strip the redundant suffix before sending it to the provider.
pub const OUTPUT_SCHEMA_SUFFIX_MARKER: &str =
    "You MUST respond with valid JSON conforming to this JSON Schema:";

#[derive(Embed)]
#[folder = "assets/roles/"]
struct RolesAsset;

/// Phase 26C: One knowledge-base reference declared on a role. Multiple
/// bindings fuse via RRF at query time (Phase 26F).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeBinding {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default = "default_binding_weight", skip_serializing_if = "is_default_weight")]
    pub weight: f32,
}

fn default_binding_weight() -> f32 {
    1.0
}

fn is_default_weight(w: &f32) -> bool {
    (*w - 1.0).abs() < f32::EPSILON
}

fn is_false_ref(b: &bool) -> bool {
    !*b
}

impl KnowledgeBinding {
    pub fn simple(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tags: Vec::new(),
            weight: 1.0,
        }
    }
}

/// Phase 26C: parse the flexible `knowledge:` frontmatter into a list of
/// bindings. Accepts three shapes:
/// - `knowledge: my-kb` (string → single binding)
/// - `knowledge: [kb-a, kb-b]` (list of strings → multiple simple bindings)
/// - `knowledge: [{name: kb-a, tags: [...], weight: 1.5}, ...]` (full form)
pub(crate) fn parse_knowledge_frontmatter_value(v: &Value) -> Vec<KnowledgeBinding> {
    if let Some(name) = v.as_str() {
        return vec![KnowledgeBinding::simple(name)];
    }
    if let Some(arr) = v.as_array() {
        let mut out = Vec::new();
        for item in arr {
            if let Some(name) = item.as_str() {
                out.push(KnowledgeBinding::simple(name));
            } else if item.is_object() {
                if let Ok(b) = serde_json::from_value::<KnowledgeBinding>(item.clone()) {
                    out.push(b);
                }
            }
        }
        return out;
    }
    Vec::new()
}

pub(crate) fn knowledge_bindings_to_export(bindings: &[KnowledgeBinding]) -> Value {
    // Prefer the most compact round-trippable form:
    // - one binding, no tags, default weight → a bare string
    // - many simple bindings, no tags, default weight → list of strings
    // - anything else → list of objects
    let all_simple = bindings
        .iter()
        .all(|b| b.tags.is_empty() && is_default_weight(&b.weight));
    if all_simple {
        if bindings.len() == 1 {
            return Value::String(bindings[0].name.clone());
        }
        let names: Vec<Value> = bindings
            .iter()
            .map(|b| Value::String(b.name.clone()))
            .collect();
        return Value::Array(names);
    }
    serde_json::json!(bindings)
}

static RE_METADATA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)-{3,}\s*(.*?)\s*-{3,}\s*(.*)").unwrap());

pub trait RoleLike {
    fn to_role(&self) -> Role;
    fn model(&self) -> &Model;
    fn temperature(&self) -> Option<f64>;
    fn top_p(&self) -> Option<f64>;
    fn use_tools(&self) -> Option<String>;
    fn set_model(&mut self, model: Model);
    fn set_temperature(&mut self, value: Option<f64>);
    fn set_top_p(&mut self, value: Option<f64>);
    fn set_use_tools(&mut self, value: Option<String>);
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Role {
    name: String,
    #[serde(default)]
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    /// Phase 14A: free-form capability tags for discovery (e.g. `code-review`,
    /// `summarization`). Distinct from `tags`: capabilities describe *what the
    /// role can do*, while tags are organizational labels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    capabilities: Vec<String>,
    #[serde(
        rename(serialize = "model", deserialize = "model"),
        skip_serializing_if = "Option::is_none"
    )]
    model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    use_tools: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_schema: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_schema: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    examples: Option<Vec<RoleExample>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pipeline: Option<Vec<PipelineNode>>,

    // Phase 11D: dollar budget for the whole pipeline, divided across stages
    // by `budget_weight`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pipeline_budget_usd: Option<f64>,

    // Phase 6B: Lifecycle hooks
    #[serde(skip_serializing_if = "Option::is_none")]
    pipe_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    save_to: Option<String>,

    // Phase 9C: Schema validation retry loop
    #[serde(skip_serializing_if = "Option::is_none")]
    schema_retries: Option<usize>,

    // Phase 10C: Pipeline stage retry on transient failures
    #[serde(skip_serializing_if = "Option::is_none")]
    stage_retries: Option<usize>,

    // Phase 10D: Model fallback chain — tried in order after primary model's
    // retries exhaust with a retryable error.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fallback_models: Vec<String>,

    // Phase 6C: Unified resource binding
    #[serde(
        rename(serialize = "mcp_servers", deserialize = "mcp_servers"),
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    role_mcp_servers: Vec<String>,

    // Phase 26C: Knowledge-base bindings. Parsed from flexible frontmatter:
    //   knowledge: my-kb
    //   knowledge: [kb-a, kb-b]
    //   knowledge:
    //     - name: kb-a
    //       tags: [kind:rule]
    //       weight: 1.5
    #[serde(default, skip_serializing_if = "Vec::is_empty", skip_deserializing)]
    knowledge_bindings: Vec<KnowledgeBinding>,

    /// Phase 26E: inject (default) auto-attaches retrieved facts to the
    /// user message; tool exposes `search_knowledge` for the LLM to call.
    #[serde(skip_serializing_if = "Option::is_none")]
    knowledge_mode: Option<String>,

    /// Phase 27D: when true, retrieved facts are prefixed with
    /// `[[fact-id]]` markers and the LLM is instructed to carry them
    /// through. The driver post-processes the response into a provenance
    /// table. Default false keeps the role behavior backward-compatible.
    #[serde(default, skip_serializing_if = "is_false_ref")]
    attributed_output: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    extends: Option<String>,
    #[serde(
        rename(serialize = "include", deserialize = "include"),
        skip_serializing_if = "Vec::is_empty"
    )]
    include: Vec<String>,
    #[serde(skip)]
    model: Model,
    #[serde(skip)]
    variables: Vec<RoleVariable>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RoleExample {
    pub input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RolePipelineStage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Phase 11D: relative share of the pipeline's `pipeline_budget_usd`.
    /// `None` means the implicit default of 1.0. See
    /// [`crate::context_budget::allocate_stage_budgets`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_weight: Option<f64>,
}

// Phase 21: DAG primitives — a pipeline is a list of `PipelineNode`s. The
// existing sequential stage is the `Stage` variant; `Parallel` is fan-out;
// `Switch` is conditional routing.
#[derive(Debug, Clone)]
pub enum PipelineNode {
    Stage(RolePipelineStage),
    Parallel(ParallelNode),
    Switch(SwitchNode),
}

#[derive(Debug, Clone)]
pub struct ParallelNode {
    pub branches: Vec<PipelineNode>,
    pub merge: MergeStrategy,
}

#[derive(Debug, Clone)]
pub enum MergeStrategy {
    /// Join outputs with `\n---\n` separator.
    Concatenate,
    /// Wrap outputs in a JSON array.
    JsonArray,
    /// Pipe the concatenated outputs through a merge role.
    CustomRole(String),
}

impl Default for MergeStrategy {
    fn default() -> Self {
        MergeStrategy::Concatenate
    }
}

#[derive(Debug, Clone)]
pub struct SwitchNode {
    pub branches: Vec<SwitchBranch>,
}

#[derive(Debug, Clone)]
pub struct SwitchBranch {
    /// `None` means the branch is the `otherwise:` fallback.
    pub predicate: Option<Predicate>,
    pub node: Box<PipelineNode>,
}

/// Deterministic predicate evaluated against the previous stage's output.
/// All checks are zero-token: parse output as JSON, walk `output_field`
/// (dotted path), compare. If output is not JSON or the field is missing,
/// the predicate fails (does not match).
#[derive(Debug, Clone, Default)]
pub struct Predicate {
    /// Dotted JSON path (e.g. `"category"` or `"meta.kind"`). When `None`,
    /// the comparison runs against the raw text output (the whole previous
    /// stage's body).
    pub output_field: Option<String>,
    pub equals: Option<serde_json::Value>,
    pub contains: Option<String>,
    pub gt: Option<f64>,
    pub lt: Option<f64>,
}

impl Predicate {
    /// Evaluate this predicate against the raw text output of the prior stage.
    /// Returns `true` when all configured conditions match.
    pub fn evaluate(&self, prior_output: &str) -> bool {
        // Resolve the value to compare against.
        let target: serde_json::Value = if let Some(field) = &self.output_field {
            let json: serde_json::Value = match serde_json::from_str(prior_output) {
                Ok(v) => v,
                Err(_) => return false,
            };
            match lookup_dotted_path(&json, field) {
                Some(v) => v.clone(),
                None => return false,
            }
        } else {
            serde_json::Value::String(prior_output.to_string())
        };

        if let Some(eq) = &self.equals {
            if !value_equals(&target, eq) {
                return false;
            }
        }
        if let Some(needle) = &self.contains {
            let haystack = match &target {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            if !haystack.contains(needle) {
                return false;
            }
        }
        if let Some(threshold) = self.gt {
            let n = match value_as_f64(&target) {
                Some(n) => n,
                None => return false,
            };
            if !(n > threshold) {
                return false;
            }
        }
        if let Some(threshold) = self.lt {
            let n = match value_as_f64(&target) {
                Some(n) => n,
                None => return false,
            };
            if !(n < threshold) {
                return false;
            }
        }
        // A predicate with no clauses set is treated as "always true". This is
        // mostly defensive — parsing rejects empty predicates and produces an
        // explicit `otherwise:` branch instead.
        true
    }
}

fn lookup_dotted_path<'a>(
    json: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    let mut cur = json;
    for segment in path.split('.') {
        if segment.is_empty() {
            return None;
        }
        cur = match cur {
            serde_json::Value::Object(map) => map.get(segment)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn value_as_f64(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn value_equals(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    // Allow loose equality across string/number: `"category": "bug"` should
    // match both `equals: "bug"` and `equals: bug` (YAML often unquotes).
    if a == b {
        return true;
    }
    if let (serde_json::Value::String(s), other) | (other, serde_json::Value::String(s)) =
        (a, b)
    {
        if let Some(n) = value_as_f64(other) {
            if let Ok(parsed) = s.parse::<f64>() {
                return (parsed - n).abs() < f64::EPSILON;
            }
        }
        // Bool stringification — "true" == true, "false" == false
        if let serde_json::Value::Bool(b) = other {
            return s.eq_ignore_ascii_case(if *b { "true" } else { "false" });
        }
    }
    false
}

/// Parse a single pipeline-node JSON value (originally YAML). The dispatch
/// is map-key based: `parallel:` → Parallel, `switch:` → Switch, otherwise
/// treat as a leaf Stage (`role:` + optional `model:`).
pub fn parse_pipeline_node(value: &serde_json::Value) -> Result<PipelineNode> {
    let map = value
        .as_object()
        .ok_or_else(|| anyhow!("Pipeline node must be a YAML mapping, got: {value}"))?;

    if let Some(parallel) = map.get("parallel") {
        let arr = parallel
            .as_array()
            .ok_or_else(|| anyhow!("`parallel:` must be a list of nodes"))?;
        if arr.is_empty() {
            bail!("`parallel:` requires at least one branch");
        }
        let branches: Result<Vec<PipelineNode>> =
            arr.iter().map(parse_pipeline_node).collect();
        let branches = branches?;
        let merge = match map.get("merge") {
            None => MergeStrategy::default(),
            Some(serde_json::Value::String(s)) => match s.as_str() {
                "concatenate" => MergeStrategy::Concatenate,
                "json_array" => MergeStrategy::JsonArray,
                other => bail!(
                    "Unknown merge strategy '{other}'. Use 'concatenate', \
                     'json_array', or a mapping with `custom_role:`"
                ),
            },
            Some(serde_json::Value::Object(o)) => {
                let role = o
                    .get("custom_role")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        anyhow!("merge mapping requires `custom_role: <role-name>`")
                    })?;
                MergeStrategy::CustomRole(role.to_string())
            }
            Some(other) => bail!("Invalid `merge:` value: {other}"),
        };
        return Ok(PipelineNode::Parallel(ParallelNode { branches, merge }));
    }

    if let Some(switch) = map.get("switch") {
        let arr = switch
            .as_array()
            .ok_or_else(|| anyhow!("`switch:` must be a list of conditional branches"))?;
        if arr.is_empty() {
            bail!("`switch:` requires at least one branch");
        }
        let mut branches: Vec<SwitchBranch> = Vec::with_capacity(arr.len());
        let mut seen_otherwise = false;
        for (idx, raw) in arr.iter().enumerate() {
            let m = raw.as_object().ok_or_else(|| {
                anyhow!("switch branch {} must be a mapping", idx + 1)
            })?;
            let has_otherwise = m.contains_key("otherwise");
            let has_when = m.contains_key("when");
            if has_otherwise && has_when {
                bail!(
                    "switch branch {} mixes `when:` and `otherwise:` — pick one",
                    idx + 1
                );
            }
            if !has_otherwise && !has_when {
                bail!(
                    "switch branch {} requires either `when:` or `otherwise:`",
                    idx + 1
                );
            }
            if has_otherwise {
                if seen_otherwise {
                    bail!("switch has more than one `otherwise:` branch");
                }
                seen_otherwise = true;
            }

            let predicate = if has_when {
                Some(parse_predicate(&m["when"]).with_context(|| {
                    format!("switch branch {} has invalid `when:` predicate", idx + 1)
                })?)
            } else {
                None
            };

            // Extract the body. If the branch carries a `role:`/`parallel:`/
            // `switch:` sibling, that is the body. Otherwise, if `otherwise:`
            // is itself a mapping describing a node, use that.
            let body = build_branch_body(m).with_context(|| {
                format!("switch branch {} has no executable body", idx + 1)
            })?;
            branches.push(SwitchBranch {
                predicate,
                node: Box::new(body),
            });
        }
        return Ok(PipelineNode::Switch(SwitchNode { branches }));
    }

    // Leaf stage — must have `role:`.
    let stage: RolePipelineStage = serde_json::from_value(value.clone())
        .with_context(|| format!("Invalid pipeline stage: {value}"))?;
    if stage.role.trim().is_empty() {
        bail!("Pipeline stage requires a non-empty `role:`");
    }
    Ok(PipelineNode::Stage(stage))
}

fn build_branch_body(
    m: &serde_json::Map<String, serde_json::Value>,
) -> Result<PipelineNode> {
    // Prefer a sibling `role:`/`parallel:`/`switch:` at the same level as
    // the predicate. This is the natural YAML shape and matches the design
    // doc samples.
    let mut body_map = serde_json::Map::new();
    let mut has_body = false;
    for key in ["role", "model", "parallel", "merge", "switch"] {
        if let Some(v) = m.get(key) {
            body_map.insert(key.to_string(), v.clone());
            if key != "model" && key != "merge" {
                has_body = true;
            }
        }
    }
    if has_body {
        return parse_pipeline_node(&serde_json::Value::Object(body_map));
    }
    // Fall back to a body nested under `otherwise:` (e.g.
    // `otherwise: { role: general-review }`).
    if let Some(serde_json::Value::Object(_)) = m.get("otherwise") {
        return parse_pipeline_node(&m["otherwise"]);
    }
    bail!("branch must specify `role:`, `parallel:`, or `switch:`")
}

fn parse_predicate(value: &serde_json::Value) -> Result<Predicate> {
    let map = value
        .as_object()
        .ok_or_else(|| anyhow!("`when:` must be a mapping"))?;
    let mut p = Predicate::default();
    let mut any = false;
    for (k, v) in map {
        match k.as_str() {
            "output_field" => {
                let s = v
                    .as_str()
                    .ok_or_else(|| anyhow!("`output_field` must be a string"))?;
                p.output_field = Some(s.to_string());
            }
            "equals" => {
                p.equals = Some(v.clone());
                any = true;
            }
            "contains" => {
                let s = v
                    .as_str()
                    .ok_or_else(|| anyhow!("`contains` must be a string"))?;
                p.contains = Some(s.to_string());
                any = true;
            }
            "gt" => {
                let n = value_as_f64(v)
                    .ok_or_else(|| anyhow!("`gt` must be a number"))?;
                p.gt = Some(n);
                any = true;
            }
            "lt" => {
                let n = value_as_f64(v)
                    .ok_or_else(|| anyhow!("`lt` must be a number"))?;
                p.lt = Some(n);
                any = true;
            }
            other => bail!("Unknown predicate key '{other}'"),
        }
    }
    if !any {
        bail!("Predicate requires at least one of: equals, contains, gt, lt");
    }
    Ok(p)
}

impl PipelineNode {
    /// Collect every leaf `Stage` reachable from this node. Used for
    /// preflight validation that doesn't need to honor routing — every
    /// declared role must exist.
    pub fn all_stages(&self) -> Vec<&RolePipelineStage> {
        let mut out = Vec::new();
        self.collect_stages(&mut out);
        out
    }

    fn collect_stages<'a>(&'a self, out: &mut Vec<&'a RolePipelineStage>) {
        match self {
            PipelineNode::Stage(s) => out.push(s),
            PipelineNode::Parallel(p) => {
                for b in &p.branches {
                    b.collect_stages(out);
                }
                if let MergeStrategy::CustomRole(_) = p.merge {
                    // The custom-role merger is a stage too; we synthesize a
                    // stub here so preflight can validate its existence
                    // without owning the RolePipelineStage memory.
                    // (See PipelineNode::merge_role_names for the actual
                    // collection path used by preflight.)
                }
            }
            PipelineNode::Switch(s) => {
                for b in &s.branches {
                    b.node.collect_stages(out);
                }
            }
        }
    }

    /// Collect every custom-role merger name reachable from this node.
    /// Preflight checks these as well — a missing merge role is a
    /// declarative error, even if no branch fans out at runtime.
    pub fn merge_role_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_merge_roles(&mut out);
        out
    }

    fn collect_merge_roles(&self, out: &mut Vec<String>) {
        match self {
            PipelineNode::Stage(_) => {}
            PipelineNode::Parallel(p) => {
                if let MergeStrategy::CustomRole(name) = &p.merge {
                    out.push(name.clone());
                }
                for b in &p.branches {
                    b.collect_merge_roles(out);
                }
            }
            PipelineNode::Switch(s) => {
                for b in &s.branches {
                    b.node.collect_merge_roles(out);
                }
            }
        }
    }

    /// Phase 21D: shallow structural validation — every parallel/switch must
    /// have at least one branch, every switch must have at most one
    /// `otherwise:`, etc. Parser guarantees most of this; this is a defense
    /// for nodes constructed programmatically.
    pub fn structural_check(&self) -> Result<()> {
        match self {
            PipelineNode::Stage(s) => {
                if s.role.trim().is_empty() {
                    bail!("Stage has empty role name");
                }
            }
            PipelineNode::Parallel(p) => {
                if p.branches.is_empty() {
                    bail!("Parallel node has no branches");
                }
                for b in &p.branches {
                    b.structural_check()?;
                }
            }
            PipelineNode::Switch(s) => {
                if s.branches.is_empty() {
                    bail!("Switch node has no branches");
                }
                let mut otherwise_count = 0;
                for b in &s.branches {
                    if b.predicate.is_none() {
                        otherwise_count += 1;
                    }
                    b.node.structural_check()?;
                }
                if otherwise_count > 1 {
                    bail!("Switch node has more than one `otherwise:` branch");
                }
            }
        }
        Ok(())
    }
}

// Serde plumbing for PipelineNode: we use the structural parser above for
// deserialization (so YAML errors are friendlier) and round-trip to a
// minimal JSON shape for serialization. Round-tripping isn't a primary use
// case — pipelines live in source YAML — but `Role::export()` calls
// serde_json::json! on the field, so a Serialize impl keeps that path
// working.
impl<'de> serde::Deserialize<'de> for PipelineNode {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = serde_json::Value::deserialize(deserializer)?;
        parse_pipeline_node(&v).map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for PipelineNode {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            PipelineNode::Stage(s) => s.serialize(serializer),
            PipelineNode::Parallel(p) => {
                use serde::ser::SerializeMap;
                let mut m = serializer.serialize_map(Some(2))?;
                m.serialize_entry("parallel", &p.branches)?;
                match &p.merge {
                    MergeStrategy::Concatenate => {
                        m.serialize_entry("merge", "concatenate")?
                    }
                    MergeStrategy::JsonArray => {
                        m.serialize_entry("merge", "json_array")?
                    }
                    MergeStrategy::CustomRole(name) => {
                        let mut o = serde_json::Map::new();
                        o.insert(
                            "custom_role".into(),
                            serde_json::Value::String(name.clone()),
                        );
                        m.serialize_entry("merge", &o)?;
                    }
                }
                m.end()
            }
            PipelineNode::Switch(s) => {
                use serde::ser::SerializeMap;
                let mut m = serializer.serialize_map(Some(1))?;
                let arr: Vec<serde_json::Value> = s
                    .branches
                    .iter()
                    .map(|b| {
                        let mut o = serde_json::Map::new();
                        match &b.predicate {
                            Some(p) => {
                                let mut wp = serde_json::Map::new();
                                if let Some(f) = &p.output_field {
                                    wp.insert(
                                        "output_field".into(),
                                        serde_json::Value::String(f.clone()),
                                    );
                                }
                                if let Some(eq) = &p.equals {
                                    wp.insert("equals".into(), eq.clone());
                                }
                                if let Some(c) = &p.contains {
                                    wp.insert(
                                        "contains".into(),
                                        serde_json::Value::String(c.clone()),
                                    );
                                }
                                if let Some(g) = p.gt {
                                    wp.insert(
                                        "gt".into(),
                                        serde_json::Value::Number(
                                            serde_json::Number::from_f64(g).unwrap_or_else(
                                                || serde_json::Number::from(0),
                                            ),
                                        ),
                                    );
                                }
                                if let Some(l) = p.lt {
                                    wp.insert(
                                        "lt".into(),
                                        serde_json::Value::Number(
                                            serde_json::Number::from_f64(l).unwrap_or_else(
                                                || serde_json::Number::from(0),
                                            ),
                                        ),
                                    );
                                }
                                o.insert(
                                    "when".into(),
                                    serde_json::Value::Object(wp),
                                );
                            }
                            None => {
                                o.insert(
                                    "otherwise".into(),
                                    serde_json::Value::Bool(true),
                                );
                            }
                        }
                        // Inline the body so the YAML matches the design.
                        if let Ok(body) = serde_json::to_value(b.node.as_ref()) {
                            if let serde_json::Value::Object(body_map) = body {
                                for (k, v) in body_map {
                                    o.insert(k, v);
                                }
                            }
                        }
                        serde_json::Value::Object(o)
                    })
                    .collect();
                m.serialize_entry("switch", &arr)?;
                m.end()
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum VariableDefault {
    Value(String),
    Shell { shell: String },
}

impl VariableDefault {
    /// Resolve the default to a concrete string value.
    /// For `Value`, returns the string directly.
    /// For `Shell`, executes the command and returns stdout (trimmed).
    pub fn resolve(&self) -> anyhow::Result<String> {
        match self {
            VariableDefault::Value(s) => Ok(s.clone()),
            VariableDefault::Shell { shell } => {
                let output = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(shell)
                    .output()
                    .with_context(|| format!("Failed to execute shell variable: {shell}"))?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!(
                        "Shell variable command failed (exit {}): {}",
                        output.status,
                        stderr.trim()
                    );
                }
                Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
            }
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RoleVariable {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<VariableDefault>,
}

/// Phase 33B: render a resolved slot value into the string spliced into the
/// prompt at `{{name}}`. Scalars render bare (no quotes); arrays and objects
/// render as compact JSON by default, or pretty-printed when the property is
/// annotated `x-aichat: { render: pretty }`. `null` renders empty. Strings
/// pass through unchanged, so existing string-only `variables:` slots render
/// exactly as they did before this phase.
pub(crate) fn render_slot(value: &Value, pretty: bool) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(_) | Value::Number(_) => value.to_string(),
        Value::Array(_) | Value::Object(_) => {
            if pretty {
                serde_json::to_string_pretty(value).unwrap_or_default()
            } else {
                serde_json::to_string(value).unwrap_or_default()
            }
        }
    }
}

/// Phase 33A: a per-property default declared inside an `input_schema`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SlotDefault {
    /// A literal JSON default (`default: "main"`, `default: 3`, `default: [..]`).
    Literal(Value),
    /// A shell-injected default (`default: { shell: "date +%F" }`).
    Shell(String),
}

/// Phase 33A: one declared parameter slot extracted from an `input_schema`'s
/// `properties`. `pretty` reflects `x-aichat: { render: pretty }`; `required`
/// mirrors the schema's `required:` list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SchemaSlot {
    pub name: String,
    pub default: Option<SlotDefault>,
    pub required: bool,
    pub pretty: bool,
    /// Phase 33C: the property's declared JSON Schema `type` (`"string"`,
    /// `"integer"`, `"array"`, …), used to coerce a CLI `-v` string into the
    /// right JSON type. `None` when the property declares no `type`.
    pub slot_type: Option<String>,
}

impl SlotDefault {
    /// Resolve to a concrete JSON value. Literals pass through; shell directives
    /// run the command and yield its trimmed stdout as a string (reusing
    /// [`VariableDefault`]'s shell execution).
    pub(crate) fn resolve(&self) -> anyhow::Result<Value> {
        match self {
            SlotDefault::Literal(v) => Ok(v.clone()),
            SlotDefault::Shell(cmd) => VariableDefault::Shell { shell: cmd.clone() }
                .resolve()
                .map(Value::String),
        }
    }
}

/// Phase 33A/33B/33E: fold a role's two declared input channels — the legacy
/// string-only `variables:` block and the typed `input_schema:` properties —
/// into one rendered `{{name}} -> string` map.
///
/// Precedence per slot: CLI `-v` > declared default. `variables:` keeps today's
/// semantics exactly, including the hard error when a variable has no value and
/// no default. `input_schema` properties are **additive**: a property with
/// neither a CLI value nor a `default:` is left unresolved (skipped) rather than
/// erroring — the schema's own message validation still enforces `required:`,
/// so roles that pass their payload as the message (not via `-v`) keep working.
/// On a name collision the schema property wins (the schema is the source of
/// truth, Phase 33E). Typed values are rendered with [`render_slot`].
pub(crate) fn resolve_slots(
    variables: &[RoleVariable],
    input_schema: Option<&Value>,
    cli: Option<&IndexMap<String, String>>,
) -> anyhow::Result<IndexMap<String, String>> {
    let mut typed: IndexMap<String, (Value, bool)> = IndexMap::new();

    for var in variables {
        let value = cli
            .and_then(|m| m.get(&var.name))
            .cloned()
            .or_else(|| {
                var.default.as_ref().and_then(|d| match d.resolve() {
                    Ok(v) => Some(v),
                    Err(e) => {
                        warn!("Shell variable '{}' failed: {e}", var.name);
                        None
                    }
                })
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Role variable '{}' is required but not provided (use -v {}=VALUE)",
                    var.name,
                    var.name
                )
            })?;
        typed.insert(var.name.clone(), (Value::String(value), false));
    }

    if let Some(schema) = input_schema {
        for slot in schema_slots(schema) {
            // Phase 33C: a CLI `-v` value is coerced against the property's
            // declared type (and propagates the error if it doesn't fit);
            // otherwise fall back to the declared default.
            let value: Option<Value> = match cli.and_then(|m| m.get(&slot.name)) {
                Some(raw) => Some(coerce_cli_value(&slot.name, raw, slot.slot_type.as_deref())?),
                None => slot.default.as_ref().and_then(|d| match d.resolve() {
                    Ok(v) => Some(v),
                    Err(e) => {
                        warn!("Shell default for '{}' failed: {e}", slot.name);
                        None
                    }
                }),
            };
            if let Some(v) = value {
                typed.insert(slot.name.clone(), (v, slot.pretty));
            }
        }
    }

    Ok(typed
        .into_iter()
        .map(|(k, (v, pretty))| (k, render_slot(&v, pretty)))
        .collect())
}

/// Phase 33A: flatten an `input_schema`'s `properties` into the declared
/// parameter slots. A `default` that is an object with the single key `shell`
/// (a string) is read as a shell directive; any other `default` is a literal
/// JSON value. Returns an empty vec for a schema with no `properties`.
pub(crate) fn schema_slots(schema: &Value) -> Vec<SchemaSlot> {
    let props = match schema.get("properties").and_then(Value::as_object) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let required: std::collections::HashSet<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    props
        .iter()
        .map(|(name, prop)| {
            let default = prop.get("default").map(|d| match d.as_object() {
                Some(obj)
                    if obj.len() == 1
                        && obj.get("shell").and_then(Value::as_str).is_some() =>
                {
                    SlotDefault::Shell(obj["shell"].as_str().unwrap().to_string())
                }
                _ => SlotDefault::Literal(d.clone()),
            });
            let pretty = prop
                .get("x-aichat")
                .and_then(|x| x.get("render"))
                .and_then(Value::as_str)
                == Some("pretty");
            let slot_type = prop
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string);
            SchemaSlot {
                name: name.clone(),
                default,
                required: required.contains(name.as_str()),
                pretty,
                slot_type,
            }
        })
        .collect()
}

/// Phase 33C: coerce a CLI `-v name=value` string into the JSON type declared
/// by the matching schema property.
///
/// - `@path` reads the file: parsed as JSON for non-string slots, used verbatim
///   for a `string` slot (so a text file can populate a string slot).
/// - `string` (or an undeclared type) keeps the raw value.
/// - `integer` / `number` / `boolean` parse to the scalar, erroring with a
///   message that names the property, the value, and the expected type.
/// - `array` / `object` parse as JSON and must produce the matching container.
pub(crate) fn coerce_cli_value(
    name: &str,
    raw: &str,
    slot_type: Option<&str>,
) -> anyhow::Result<Value> {
    // `@path` pulls the value from a file. For a string slot the file content is
    // the value verbatim; otherwise the content is parsed as JSON.
    let (text, from_file) = match raw.strip_prefix('@') {
        Some(path) => {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("-v {name}=@{path}: failed to read file"))?;
            (content, true)
        }
        None => (raw.to_string(), false),
    };
    let body = if from_file { text.trim() } else { text.as_str() };

    let coerce_err = |expected: &str| {
        anyhow::anyhow!("-v {name}={body}: value is not a valid {expected}")
    };

    match slot_type {
        None | Some("string") => Ok(Value::String(if from_file {
            text.clone()
        } else {
            raw.to_string()
        })),
        Some("integer") => body
            .parse::<i64>()
            .map(|n| Value::Number(n.into()))
            .map_err(|_| coerce_err("integer")),
        Some("number") => body
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| coerce_err("number")),
        Some("boolean") => body
            .parse::<bool>()
            .map(Value::Bool)
            .map_err(|_| coerce_err("boolean")),
        Some(t @ ("array" | "object")) => {
            let v: Value = serde_json::from_str(body).map_err(|_| coerce_err(t))?;
            let ok = (t == "array" && v.is_array()) || (t == "object" && v.is_object());
            if ok {
                Ok(v)
            } else {
                Err(coerce_err(t))
            }
        }
        // Unknown / unsupported type annotation: leave it a string.
        Some(_) => Ok(Value::String(raw.to_string())),
    }
}

/// Phase 33C: the name of the schema property annotated
/// `x-aichat: { source: stdin }`, if any. That slot receives the stdin/message
/// content (the conventional name is `body`). Returns the first such property;
/// `None` when no property opts in (the common case — existing roles).
pub(crate) fn stdin_slot(schema: &Value) -> Option<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)?
        .iter()
        .find(|(_, prop)| {
            prop.get("x-aichat")
                .and_then(|x| x.get("source"))
                .and_then(Value::as_str)
                == Some("stdin")
        })
        .map(|(name, _)| name.clone())
}

#[derive(Debug)]
struct RawRoleParts {
    metadata: serde_json::Map<String, serde_json::Value>,
    prompt: String,
    extends: Option<String>,
    includes: Vec<String>,
    variables: Vec<RoleVariable>,
}

fn parse_raw_frontmatter(content: &str) -> RawRoleParts {
    let mut metadata = serde_json::Map::new();
    let mut prompt = content.trim().to_string();
    let mut extends = None;
    let mut includes = Vec::new();
    let mut variables = Vec::new();

    if let Ok(Some(caps)) = RE_METADATA.captures(content) {
        if let (Some(metadata_value), Some(prompt_value)) = (caps.get(1), caps.get(2)) {
            let meta_str = metadata_value.as_str().trim();
            prompt = prompt_value.as_str().trim().to_string();

            match serde_yaml::from_str::<Value>(meta_str) {
                Ok(value) => {
                    if let Some(map) = value.as_object() {
                        for (key, value) in map {
                            match key.as_str() {
                                "extends" => {
                                    extends = value.as_str().map(|v| v.to_string());
                                }
                                "include" => {
                                    if let Some(arr) = value.as_array() {
                                        includes = arr
                                            .iter()
                                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                            .collect();
                                    }
                                }
                                "variables" => {
                                    if let Some(arr) = value.as_array() {
                                        variables = arr
                                            .iter()
                                            .filter_map(|v| match serde_json::from_value::<RoleVariable>(v.clone()) {
                                                Ok(var) => Some(var),
                                                Err(e) => {
                                                    warn!("Skipping invalid variable in frontmatter: {e}");
                                                    None
                                                }
                                            })
                                            .collect();
                                    }
                                }
                                _ => {
                                    metadata.insert(key.clone(), value.clone());
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Role frontmatter has invalid YAML: {e}");
                }
            }
        }
    }

    RawRoleParts {
        metadata,
        prompt,
        extends,
        includes,
        variables,
    }
}

fn read_raw_role_content(name: &str) -> Result<String> {
    let names = Config::list_roles(false);
    if names.contains(&name.to_string()) {
        let path = Config::role_file(name);
        return read_to_string(&path)
            .with_context(|| format!("Failed to read role file '{}'", path.display()));
    }
    let content =
        RolesAsset::get(&format!("{name}.md")).ok_or_else(|| anyhow!("Unknown role `{name}`"))?;
    let content = unsafe { std::str::from_utf8_unchecked(&content.data) };
    Ok(content.to_string())
}

fn resolve_role_content(name: &str, visited: &mut Vec<String>) -> Result<RawRoleParts> {
    if visited.contains(&name.to_string()) {
        let chain = visited.join(" -> ");
        bail!("Circular role inheritance: {chain} -> {name}");
    }
    visited.push(name.to_string());

    let content = read_raw_role_content(name)?;
    let mut parts = parse_raw_frontmatter(&content);

    // Resolve includes (each with a fresh visited set seeded with current name)
    let mut include_prompts = Vec::new();
    for include_name in &parts.includes {
        let mut include_visited = vec![name.to_string()];
        let include_parts = resolve_role_content(include_name, &mut include_visited)?;
        if !include_parts.prompt.is_empty() {
            include_prompts.push(include_parts.prompt);
        }
    }

    // Resolve extends
    if let Some(parent_name) = parts.extends.clone() {
        let parent = resolve_role_content(&parent_name, visited)?;

        // Merge metadata: parent defaults, child overrides
        let child_metadata = parts.metadata;
        parts.metadata = parent.metadata;
        for (key, value) in child_metadata {
            parts.metadata.insert(key, value);
        }

        // Merge variables: parent first, child overrides defaults by name
        let child_variables = parts.variables;
        parts.variables = parent.variables;
        for cv in child_variables {
            if let Some(existing) = parts.variables.iter_mut().find(|v| v.name == cv.name) {
                if cv.default.is_some() {
                    existing.default = cv.default;
                }
            } else {
                parts.variables.push(cv);
            }
        }

        // Concatenate prompts: includes -> parent -> child
        // De-hoist __INPUT__ from parent when child extends it:
        // - If child re-declares __INPUT__, strip the parent's (child wins)
        // - If child doesn't, relocate parent's __INPUT__ to end of combined prompt
        let parent_has_input = parent.prompt.contains(INPUT_PLACEHOLDER);
        let child_has_input = parts.prompt.contains(INPUT_PLACEHOLDER);

        let mut prompt_parts = include_prompts;
        if !parent.prompt.is_empty() {
            let parent_prompt = if parent_has_input {
                parent
                    .prompt
                    .replace(INPUT_PLACEHOLDER, "")
                    .trim()
                    .to_string()
            } else {
                parent.prompt
            };
            if !parent_prompt.is_empty() {
                prompt_parts.push(parent_prompt);
            }
        }
        if !parts.prompt.is_empty() {
            prompt_parts.push(parts.prompt);
        }
        let mut combined = prompt_parts.join("\n\n");
        if parent_has_input && !child_has_input {
            combined = format!("{combined}\n\n{INPUT_PLACEHOLDER}");
        }
        parts.prompt = combined;
    } else if !include_prompts.is_empty() {
        // No extends, just prepend includes
        let mut prompt_parts = include_prompts;
        if !parts.prompt.is_empty() {
            prompt_parts.push(parts.prompt);
        }
        parts.prompt = prompt_parts.join("\n\n");
    }

    // parts.includes = Vec::new();

    visited.pop();

    Ok(parts)
}

fn compose_role_content(parts: &RawRoleParts) -> String {
    let mut metadata = parts.metadata.clone();
    if let Some(extends) = &parts.extends {
        metadata.insert("extends".to_string(), serde_json::json!(extends));
    }
    if !parts.includes.is_empty() {
        metadata.insert("include".to_string(), serde_json::json!(parts.includes));
    }
    if metadata.is_empty() {
        parts.prompt.clone()
    } else {
        let yaml = serde_yaml::to_string(&Value::Object(metadata)).unwrap_or_default();
        if parts.prompt.is_empty() {
            format!("---\n{yaml}---")
        } else {
            format!("---\n{yaml}---\n\n{}", parts.prompt)
        }
    }
}

/// Phase 16F / 16G: federation-safe projection of a `Role`.
///
/// Designed for the `/v1/roles` and `/v1/roles/{name}` endpoints. Surfaces
/// only the fields a remote caller needs to decide whether to invoke the
/// role and what shape its I/O takes. Deliberately omits anything the
/// server's operator would consider sensitive:
///
/// - `prompt` body (may contain proprietary instructions, secrets, internal
///   tone/voice guidance)
/// - shell-injective variable defaults (`{shell: "cmd"}`) — those are
///   commands, not data
/// - `pipe_to` / `save_to` (server-local shell commands and filesystem paths)
/// - `mcp_servers` / `use_tools` (internal binding wiring)
/// - pipeline stage definitions beyond a length count (stage role names are
///   server-local identifiers; exposing them leaks the server's role
///   namespace)
///
/// Schemas (`input_schema`, `output_schema`) are included verbatim: they are
/// the contract the remote caller needs to satisfy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolePublicView {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    pub has_pipeline: bool,
    pub pipeline_length: usize,
    /// Phase 14B port summary, e.g. `"text"`, `"json{a, b}"`, `"any"`.
    pub port_input: String,
    pub port_output: String,
    /// Opt-in only: populated when callers explicitly request the prompt
    /// body (e.g. the local playground via `?include_prompt=1`). Default
    /// `From<&Role>` leaves this `None` so federated/remote `/v1/roles`
    /// responses still hide it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

impl RolePublicView {
    pub fn with_prompt(mut self, role: &Role) -> Self {
        self.prompt = Some(role.prompt().to_string());
        self
    }
}

impl From<&Role> for RolePublicView {
    fn from(role: &Role) -> Self {
        RolePublicView {
            name: role.name().to_string(),
            description: role.description().map(str::to_string),
            tags: role.tags().map(<[String]>::to_vec),
            capabilities: role.capabilities().to_vec(),
            // Prefer the user-declared model_id; fall back to the resolved
            // model's id if one is attached, otherwise omit.
            model: role
                .model_id()
                .map(str::to_string)
                .or_else(|| {
                    let id = role.model.id();
                    if id.is_empty() {
                        None
                    } else {
                        Some(id)
                    }
                }),
            input_schema: role.input_schema().cloned(),
            output_schema: role.output_schema().cloned(),
            has_pipeline: role.is_pipeline(),
            pipeline_length: role.pipeline().map(<[PipelineNode]>::len).unwrap_or(0),
            port_input: role.port_input_summary(),
            port_output: role.port_output_summary(),
            prompt: None,
        }
    }
}

impl Role {
    pub fn resolve(name: &str) -> Result<Self> {
        let mut visited = Vec::new();
        let parts = resolve_role_content(name, &mut visited)?;
        let variables = parts.variables.clone();
        let content = compose_role_content(&parts);
        let mut role = Role::new(name, &content);
        role.variables = variables;
        Ok(role)
    }

    pub fn new(name: &str, content: &str) -> Self {
        let mut metadata = "";
        let mut prompt = content.trim();
        if let Ok(Some(caps)) = RE_METADATA.captures(content) {
            if let (Some(metadata_value), Some(prompt_value)) = (caps.get(1), caps.get(2)) {
                metadata = metadata_value.as_str().trim();
                prompt = prompt_value.as_str().trim();
            }
        }
        let mut prompt = prompt.to_string();
        interpolate_variables(&mut prompt);
        let mut role = Self {
            name: name.to_string(),
            prompt,
            extends: None,
            include: Vec::new(),
            ..Default::default()
        };
        if !metadata.is_empty() {
            match serde_yaml::from_str::<Value>(metadata) {
                Ok(value) => {
                    if let Some(value) = value.as_object() {
                        for (key, value) in value {
                            match key.as_str() {
                                "model" => role.model_id = value.as_str().map(|v| v.to_string()),
                                "temperature" => role.temperature = value.as_f64(),
                                "top_p" => role.top_p = value.as_f64(),
                                "use_tools" | "tools" => {
                                    role.use_tools = value.as_str().map(|v| v.to_string())
                                }
                                "input_schema" => role.input_schema = Some(value.clone()),
                                "output_schema" => role.output_schema = Some(value.clone()),
                                "description" => {
                                    role.description = value.as_str().map(|v| v.to_string())
                                }
                                "tags" => {
                                    if let Some(arr) = value.as_array() {
                                        role.tags = Some(
                                            arr.iter()
                                                .filter_map(|v| v.as_str().map(String::from))
                                                .collect()
                                        );
                                    }
                                }
                                "capabilities" => {
                                    if let Some(arr) = value.as_array() {
                                        role.capabilities = arr
                                            .iter()
                                            .filter_map(|v| v.as_str().map(String::from))
                                            .collect();
                                    }
                                }
                                "examples" => {
                                    if let Some(arr) = value.as_array() {
                                        role.examples = Some(
                                            arr.iter()
                                                .filter_map(|v| match serde_json::from_value::<RoleExample>(v.clone()) {
                                                    Ok(ex) => Some(ex),
                                                    Err(e) => {
                                                        warn!("Skipping invalid example in role '{}': {e}", name);
                                                        None
                                                    }
                                                })
                                                .collect(),
                                        );
                                    }
                                }
                                "pipeline" => {
                                    if let Some(arr) = value.as_array() {
                                        role.pipeline = Some(
                                            arr.iter()
                                                .filter_map(|v| match parse_pipeline_node(v) {
                                                    Ok(node) => Some(node),
                                                    Err(e) => {
                                                        warn!("Skipping invalid pipeline node in role '{}': {e}", name);
                                                        None
                                                    }
                                                })
                                                .collect(),
                                        );
                                    }
                                }
                                // Phase 11D: total dollar budget for the pipeline.
                                "pipeline_budget_usd" => {
                                    role.pipeline_budget_usd = value.as_f64();
                                }
                                "extends" => role.extends = value.as_str().map(|v| v.to_string()),
                                "include" => {
                                    if let Some(arr) = value.as_array() {
                                        role.include = arr
                                            .iter()
                                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                            .collect();
                                    }
                                }
                                "variables" => {
                                    if let Some(arr) = value.as_array() {
                                        role.variables = arr
                                            .iter()
                                            .filter_map(|v| match serde_json::from_value::<RoleVariable>(v.clone()) {
                                                Ok(var) => Some(var),
                                                Err(e) => {
                                                    warn!("Skipping invalid variable in role '{}': {e}", name);
                                                    None
                                                }
                                            })
                                            .collect();
                                    }
                                }
                                // Phase 6B: Lifecycle hooks
                                "pipe_to" => role.pipe_to = value.as_str().map(|v| v.to_string()),
                                "save_to" => role.save_to = value.as_str().map(|v| v.to_string()),
                                // Phase 9C: Schema validation retry loop
                                "schema_retries" => {
                                    role.schema_retries = value.as_u64().map(|v| v as usize)
                                }
                                // Phase 10C: Pipeline stage retry
                                "stage_retries" => {
                                    role.stage_retries = value.as_u64().map(|v| v as usize)
                                }
                                // Phase 10D: Pipeline model fallback chain
                                "fallback_models" => {
                                    if let Some(arr) = value.as_array() {
                                        role.fallback_models = arr
                                            .iter()
                                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                            .collect();
                                    }
                                }
                                // Phase 6C: Unified resource binding
                                "mcp_servers" => {
                                    if let Some(arr) = value.as_array() {
                                        role.role_mcp_servers = arr
                                            .iter()
                                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                            .collect();
                                    }
                                }
                                // Phase 26C: Knowledge-base binding(s)
                                "knowledge" => {
                                    role.knowledge_bindings =
                                        parse_knowledge_frontmatter_value(value);
                                }
                                "knowledge_mode" => {
                                    role.knowledge_mode =
                                        value.as_str().map(|v| v.to_string())
                                }
                                // Phase 27D: per-fact citation markers in LLM output
                                "attributed_output" => {
                                    role.attributed_output =
                                        value.as_bool().unwrap_or(false);
                                }
                                // Phase 6B: Lifecycle hooks
                                _ => (),
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Role '{}' has invalid YAML metadata: {e}", name);
                }
            }
        }
        role
    }

    pub fn builtin(name: &str) -> Result<Self> {
        let content = RolesAsset::get(&format!("{name}.md"))
            .ok_or_else(|| anyhow!("Unknown role `{name}`"))?;
        let content = unsafe { std::str::from_utf8_unchecked(&content.data) };
        Ok(Role::new(name, content))
    }

    pub fn list_builtin_role_names() -> Vec<String> {
        RolesAsset::iter()
            .filter_map(|v| v.strip_suffix(".md").map(|v| v.to_string()))
            .collect()
    }

    pub fn list_builtin_roles() -> Vec<Self> {
        RolesAsset::iter()
            .filter_map(|v| Role::builtin(&v).ok())
            .collect()
    }

    pub fn has_args(&self) -> bool {
        self.name.contains('#')
    }

    pub fn export(&self) -> String {
        let mut meta = serde_json::Map::new();
        if let Some(desc) = &self.description {
            meta.insert("description".into(), Value::String(desc.clone()));
        }
        if let Some(tags) = &self.tags {
            meta.insert("tags".into(), serde_json::to_value(tags).unwrap_or_default());
        }
        if !self.capabilities.is_empty() {
            meta.insert(
                "capabilities".into(),
                serde_json::to_value(&self.capabilities).unwrap_or_default(),
            );
        }
        if let Some(model) = self.model_id() {
            meta.insert("model".into(), Value::String(model.to_string()));
        }
        if let Some(temperature) = self.temperature() {
            meta.insert("temperature".into(), serde_json::json!(temperature));
        }
        if let Some(top_p) = self.top_p() {
            meta.insert("top_p".into(), serde_json::json!(top_p));
        }
        if let Some(use_tools) = self.use_tools() {
            meta.insert("use_tools".into(), Value::String(use_tools.to_string()));
        }
        if let Some(s) = &self.input_schema {
            meta.insert("input_schema".into(), s.clone());
        }
        if let Some(s) = &self.output_schema {
            meta.insert("output_schema".into(), s.clone());
        }
        if let Some(examples) = &self.examples {
            meta.insert("examples".into(), serde_json::json!(examples));
        }
        if let Some(pipeline) = &self.pipeline {
            meta.insert("pipeline".into(), serde_json::json!(pipeline));
        }
        if let Some(budget) = self.pipeline_budget_usd {
            meta.insert("pipeline_budget_usd".into(), serde_json::json!(budget));
        }
        if let Some(pipe_to) = &self.pipe_to {
            meta.insert("pipe_to".into(), Value::String(pipe_to.clone()));
        }
        if let Some(save_to) = &self.save_to {
            meta.insert("save_to".into(), Value::String(save_to.clone()));
        }
        if let Some(n) = self.schema_retries {
            meta.insert("schema_retries".into(), serde_json::json!(n));
        }
        if let Some(n) = self.stage_retries {
            meta.insert("stage_retries".into(), serde_json::json!(n));
        }
        if !self.fallback_models.is_empty() {
            meta.insert(
                "fallback_models".into(),
                serde_json::json!(self.fallback_models),
            );
        }
        if !self.role_mcp_servers.is_empty() {
            meta.insert(
                "mcp_servers".into(),
                serde_json::json!(self.role_mcp_servers),
            );
        }
        if !self.knowledge_bindings.is_empty() {
            meta.insert(
                "knowledge".into(),
                knowledge_bindings_to_export(&self.knowledge_bindings),
            );
        }
        if let Some(mode) = &self.knowledge_mode {
            meta.insert("knowledge_mode".into(), Value::String(mode.clone()));
        }
        if self.attributed_output {
            meta.insert("attributed_output".into(), Value::Bool(true));
        }
        if let Some(extends) = &self.extends {
            meta.insert("extends".into(), serde_json::json!(extends));
        }
        if !self.include.is_empty() {
            meta.insert("include".into(), serde_json::json!(self.include));
        }
        if meta.is_empty() {
            format!("{}\n", self.prompt)
        } else {
            let yaml = serde_yaml::to_string(&Value::Object(meta)).unwrap_or_default();
            if self.prompt.is_empty() {
                format!("---\n{yaml}---\n")
            } else {
                format!("---\n{yaml}---\n\n{}\n", self.prompt)
            }
        }
    }

    pub fn save(&mut self, role_name: &str, role_path: &Path, is_repl: bool) -> Result<()> {
        ensure_parent_exists(role_path)?;

        let content = self.export();
        std::fs::write(role_path, content).with_context(|| {
            format!(
                "Failed to write role {} to {}",
                self.name,
                role_path.display()
            )
        })?;

        if is_repl {
            println!("✓ Saved role to '{}'.", role_path.display());
        }

        if role_name != self.name {
            self.name = role_name.to_string();
        }

        Ok(())
    }

    pub fn sync<T: RoleLike>(&mut self, role_like: &T) {
        let model = role_like.model();
        let temperature = role_like.temperature();
        let top_p = role_like.top_p();
        let use_tools = role_like.use_tools();
        self.batch_set(model, temperature, top_p, use_tools);
    }

    pub fn batch_set(
        &mut self,
        model: &Model,
        temperature: Option<f64>,
        top_p: Option<f64>,
        use_tools: Option<String>,
    ) {
        self.set_model(model.clone());
        if temperature.is_some() {
            self.set_temperature(temperature);
        }
        if top_p.is_some() {
            self.set_top_p(top_p);
        }
        if use_tools.is_some() {
            self.set_use_tools(use_tools);
        }
    }

    pub fn is_derived(&self) -> bool {
        self.name.is_empty()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn is_empty_prompt(&self) -> bool {
        self.prompt.is_empty()
    }

    pub fn input_schema(&self) -> Option<&Value> {
        self.input_schema.as_ref()
    }

    pub fn output_schema(&self) -> Option<&Value> {
        self.output_schema.as_ref()
    }

    pub fn has_output_schema(&self) -> bool {
        self.output_schema.is_some()
    }

    /// Phase 14B: human-readable summary of the role's input port. Returns
    /// `"any"` (no schema), `"text"` (string schema), `"json{a, b, c}"` (object
    /// schema), `"array"` (array schema), or `"json"` (other shapes).
    pub fn port_input_summary(&self) -> String {
        port_summary_from_schema(self.input_schema.as_ref())
    }

    /// Phase 14B: human-readable summary of the role's output port. Defaults
    /// to `"text"` when no `output_schema` is declared (the LLM emits free
    /// text by default).
    pub fn port_output_summary(&self) -> String {
        port_summary_from_schema_for_output(self.output_schema.as_ref())
    }

    /// Phase 14B: does the input port accept the given type-string? Tolerant
    /// match against the human form returned by `port_input_summary` — e.g.
    /// `"json"` matches `"json{a, b}"`.
    pub fn port_accepts(&self, type_query: &str) -> bool {
        port_signature_matches(&self.port_input_summary(), type_query)
    }

    /// Phase 14B: does the output port produce the given type-string?
    pub fn port_produces(&self, type_query: &str) -> bool {
        port_signature_matches(&self.port_output_summary(), type_query)
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn tags(&self) -> Option<&[String]> {
        self.tags.as_deref()
    }

    /// Phase 14A: capability tags declared by this role for discovery.
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    pub fn description_or_derived(&self) -> String {
        if let Some(desc) = &self.description {
            return desc.clone();
        }
        let prompt = self.prompt.trim();
        if prompt.is_empty() {
            return String::new();
        }
        // Take first sentence (up to first period+space or newline, max 100 chars)
        let end = prompt
            .find(". ")
            .or_else(|| prompt.find('\n'))
            .unwrap_or(prompt.len())
            .min(100);
        let mut desc = prompt[..end].to_string();
        if desc.len() < prompt.len() && !desc.ends_with('.') {
            desc.push('.');
        }
        desc
    }

    pub fn examples(&self) -> Option<&[RoleExample]> {
        self.examples.as_deref()
    }

    /// Phase 21: returns the full DAG node list. Sequential stages, fan-out
    /// (`Parallel`), and conditional routing (`Switch`) all live here.
    pub fn pipeline(&self) -> Option<&[PipelineNode]> {
        self.pipeline.as_deref()
    }

    /// Backward-compatible view: returns the leaf stage list when the
    /// pipeline is purely sequential (every top-level node is a `Stage`).
    /// For DAG pipelines, returns `None` — callers that need a flat list
    /// should use `pipeline_all_stages()` instead, which walks the tree.
    pub fn pipeline_sequential(&self) -> Option<Vec<RolePipelineStage>> {
        let nodes = self.pipeline.as_ref()?;
        let mut out = Vec::with_capacity(nodes.len());
        for n in nodes {
            match n {
                PipelineNode::Stage(s) => out.push(s.clone()),
                _ => return None,
            }
        }
        Some(out)
    }

    /// Phase 21: every leaf stage reachable from the pipeline, including
    /// stages inside parallel branches and switch arms. Used for preflight
    /// validation — `validate_pipeline_stages` checks each one exists.
    pub fn pipeline_all_stages(&self) -> Vec<RolePipelineStage> {
        let nodes = match &self.pipeline {
            Some(n) => n,
            None => return Vec::new(),
        };
        let mut out = Vec::new();
        for n in nodes {
            for s in n.all_stages() {
                out.push(s.clone());
            }
        }
        out
    }

    /// Phase 21: custom-role merger names reachable from any parallel node.
    pub fn pipeline_merge_roles(&self) -> Vec<String> {
        let nodes = match &self.pipeline {
            Some(n) => n,
            None => return Vec::new(),
        };
        let mut out = Vec::new();
        for n in nodes {
            for name in n.merge_role_names() {
                out.push(name);
            }
        }
        out
    }

    pub fn is_pipeline(&self) -> bool {
        self.pipeline.as_ref().is_some_and(|p| !p.is_empty())
    }

    /// Phase 21: true iff the pipeline contains any DAG primitive
    /// (`parallel:` or `switch:`). Purely sequential pipelines return false.
    pub fn pipeline_has_dag(&self) -> bool {
        match &self.pipeline {
            Some(nodes) => nodes
                .iter()
                .any(|n| !matches!(n, PipelineNode::Stage(_))),
            None => false,
        }
    }

    pub fn pipe_to(&self) -> Option<&str> {
        self.pipe_to.as_deref()
    }

    pub fn save_to(&self) -> Option<&str> {
        self.save_to.as_deref()
    }

    /// Phase 9C: Maximum number of schema validation retries on output failure.
    /// `None` means unset (consumer applies its default, typically 1).
    /// `Some(0)` means fail fast (no retries). `Some(n)` means up to n retries.
    pub fn schema_retries(&self) -> Option<usize> {
        self.schema_retries
    }

    /// Phase 10C: Maximum number of pipeline stage retries on transient failure
    /// (classified via `is_retryable_stage_error`). `None` means unset (consumer
    /// applies its default, typically 1). `Some(0)` means fail fast.
    pub fn stage_retries(&self) -> Option<usize> {
        self.stage_retries
    }

    /// Phase 10D: Ordered list of model IDs to try after the primary model
    /// exhausts its retry budget with a retryable error. Empty means no
    /// fallbacks — the error propagates after primary retries exhaust.
    pub fn fallback_models(&self) -> &[String] {
        &self.fallback_models
    }

    /// Phase 11D: Total dollar budget for this role's pipeline, divided
    /// across stages by `budget_weight`. `None` means no budget enforced.
    pub fn pipeline_budget_usd(&self) -> Option<f64> {
        self.pipeline_budget_usd
    }

    /// Phase 26C: Knowledge-base bindings declared by this role.
    pub fn knowledge_bindings(&self) -> &[KnowledgeBinding] {
        &self.knowledge_bindings
    }

    /// Phase 26E: "inject" (default) or "tool". When `tool`, the
    /// `search_knowledge` synthetic tool is exposed instead of
    /// auto-injecting retrieved facts into the user message.
    pub fn knowledge_mode(&self) -> Option<&str> {
        self.knowledge_mode.as_deref()
    }

    /// Phase 27D: whether to surface per-fact `[[fact-id]]` citation
    /// markers in injected context and post-process the model output into
    /// a provenance table.
    pub fn attributed_output(&self) -> bool {
        self.attributed_output
    }

    pub fn set_output_schema(&mut self, value: Option<Value>) {
        self.output_schema = value;
    }

    pub fn set_input_schema(&mut self, value: Option<Value>) {
        self.input_schema = value;
    }

    pub fn set_pipe_to(&mut self, value: Option<String>) {
        self.pipe_to = value;
    }

    pub fn set_save_to(&mut self, value: Option<String>) {
        self.save_to = value;
    }

    pub fn role_mcp_servers(&self) -> &[String] {
        &self.role_mcp_servers
    }

    /// Phase 12A: parent role this one extends (post-resolution this is None;
    /// preserved when `Role::new` is called directly on raw frontmatter).
    pub fn extends(&self) -> Option<&str> {
        self.extends.as_deref()
    }

    /// Phase 12A: included role names (post-resolution this is empty;
    /// preserved when `Role::new` is called directly on raw frontmatter).
    pub fn include(&self) -> &[String] {
        &self.include
    }

    pub fn variables(&self) -> &[RoleVariable] {
        &self.variables
    }

    pub fn apply_variables(&mut self, vars: &IndexMap<String, String>) {
        for (k, v) in vars {
            self.prompt = self.prompt.replace(&format!("{{{{{k}}}}}"), v);
        }
    }

    /// Phase 33C: true when this role's `input_schema` declares a property with
    /// `x-aichat: { source: stdin }`. Such roles take their message as free text
    /// (routed into that slot), so the raw message is not validated against the
    /// object schema.
    pub fn has_stdin_slot(&self) -> bool {
        self.input_schema
            .as_ref()
            .map(|s| stdin_slot(s).is_some())
            .unwrap_or(false)
    }

    /// Phase 33C: route the `x-aichat: { source: stdin }` slot to the input by
    /// rewriting its `{{name}}` token to the `INPUT_PLACEHOLDER` sentinel, so the
    /// existing embedded-prompt machinery splices the message there at build
    /// time. No-op when no slot opts into stdin.
    pub fn route_stdin_slot(&mut self) {
        if let Some(schema) = &self.input_schema {
            if let Some(name) = stdin_slot(schema) {
                self.prompt = self
                    .prompt
                    .replace(&format!("{{{{{name}}}}}"), INPUT_PLACEHOLDER);
            }
        }
    }

    pub fn is_embedded_prompt(&self) -> bool {
        self.prompt.contains(INPUT_PLACEHOLDER)
    }

    pub fn echo_messages(&self, input: &Input) -> String {
        let input_markdown = input.render();
        if self.is_embedded_prompt() {
            let mut role = self.clone();
            role.prompt = role.prompt.replace(INPUT_PLACEHOLDER, &input_markdown);
            role.export().trim().to_string()
        } else if self.is_empty_prompt() && self.export().trim().is_empty() {
            input_markdown
        } else {
            let role_export = self.export();
            format!("{}\n\n{}", role_export.trim(), input_markdown)
        }
    }

    pub fn build_messages(&self, input: &Input) -> Vec<Message> {
        let mut content = input.message_content();
        let mut messages = if self.is_empty_prompt() {
            vec![Message::new(MessageRole::User, content)]
        } else if self.is_embedded_prompt() {
            content.merge_prompt(|v: &str| self.prompt.replace(INPUT_PLACEHOLDER, v));
            vec![Message::new(MessageRole::User, content)]
        } else {
            let mut messages = vec![];
            let (system, cases) = parse_structure_prompt(&self.prompt);
            let system_text = if let Some(schema) = &self.output_schema {
                let schema_str = serde_json::to_string_pretty(schema).unwrap_or_default();
                let suffix = format!(
                    "{OUTPUT_SCHEMA_SUFFIX_MARKER}\n```json\n{schema_str}\n```\nDo not include any text outside the JSON object."
                );
                if system.is_empty() {
                    suffix.trim_start().to_string()
                } else {
                    format!("{system}\n\n{suffix}")
                }
            } else {
                system.to_string()
            };
            if !system_text.is_empty() {
                messages.push(Message::new(
                    MessageRole::System,
                    MessageContent::Text(system_text),
                ));
            }
            if !cases.is_empty() {
                messages.extend(cases.into_iter().flat_map(|(i, o)| {
                    vec![
                        Message::new(MessageRole::User, MessageContent::Text(i.to_string())),
                        Message::new(MessageRole::Assistant, MessageContent::Text(o.to_string())),
                    ]
                }));
            }
            messages.push(Message::new(MessageRole::User, content));
            messages
        };
        // Inject tool use examples into the system prompt if present
        if let Some(examples) = &self.examples {
            if !examples.is_empty() && self.use_tools.is_some() {
                let mut example_text = String::from("\n\n## Tool Usage Examples\n");
                for (i, ex) in examples.iter().enumerate() {
                    example_text.push_str(&format!("{}. \"{}\"", i + 1, ex.input));
                    if let Some(args) = &ex.args {
                        example_text.push_str(&format!(
                            " -> call with {}",
                            serde_json::to_string(args).unwrap_or_default()
                        ));
                    }
                    example_text.push('\n');
                }
                // Append to system message if one exists, otherwise prepend as system
                if let Some(msg) = messages
                    .iter_mut()
                    .find(|m| matches!(m.role, MessageRole::System))
                {
                    if let MessageContent::Text(ref mut t) = msg.content {
                        t.push_str(&example_text);
                    }
                } else {
                    messages.insert(
                        0,
                        Message::new(
                            MessageRole::System,
                            MessageContent::Text(example_text.trim_start().to_string()),
                        ),
                    );
                }
            }
        }

        // Phase 9C: schema validation retry — replay the failed assistant
        // output and a corrective user turn before any continue_output.
        if let Some((failed_output, retry_prompt)) = input.retry_feedback() {
            messages.push(Message::new(
                MessageRole::Assistant,
                MessageContent::Text(failed_output.to_string()),
            ));
            messages.push(Message::new(
                MessageRole::User,
                MessageContent::Text(retry_prompt.to_string()),
            ));
        }

        if let Some(text) = input.continue_output() {
            messages.push(Message::new(
                MessageRole::Assistant,
                MessageContent::Text(text.into()),
            ));
        }
        messages
    }
}

impl RoleLike for Role {
    fn to_role(&self) -> Role {
        self.clone()
    }

    fn model(&self) -> &Model {
        &self.model
    }

    fn temperature(&self) -> Option<f64> {
        self.temperature
    }

    fn top_p(&self) -> Option<f64> {
        self.top_p
    }

    fn use_tools(&self) -> Option<String> {
        self.use_tools.clone()
    }

    fn set_model(&mut self, model: Model) {
        if !self.model().id().is_empty() {
            self.model_id = Some(model.id().to_string());
        }
        interpolate_variables_with_model(&mut self.prompt, Some(&model));
        self.model = model;
    }

    fn set_temperature(&mut self, value: Option<f64>) {
        self.temperature = value;
    }

    fn set_top_p(&mut self, value: Option<f64>) {
        self.top_p = value;
    }

    fn set_use_tools(&mut self, value: Option<String>) {
        self.use_tools = value;
    }
}

/// A single schema violation with path information for trace diagnostics.
#[derive(Debug, Clone)]
pub struct SchemaViolation {
    pub message: String,
    pub instance_path: String,
    pub schema_path: String,
}

/// Result of detailed schema validation — carries the raw text and violations for trace output.
#[derive(Debug, Clone)]
pub struct SchemaValidationResult {
    pub direction: String,
    pub raw_text: String,
    pub violations: Vec<SchemaViolation>,
}

impl SchemaValidationResult {
    pub fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }

    /// Format the terse error message (same as the old validate_schema output).
    pub fn terse_error(&self) -> String {
        let lines: Vec<String> = self
            .violations
            .iter()
            .map(|v| format!("  - {}", v.message))
            .collect();
        format!(
            "Schema {} validation failed:\n{}",
            self.direction,
            lines.join("\n")
        )
    }
}

/// Validate text against a JSON schema, returning rich violation details.
/// Used by trace to show the raw output and per-violation paths.
pub fn validate_schema_detailed(
    context: &str,
    schema: &Value,
    text: &str,
) -> Result<SchemaValidationResult> {
    let trimmed = text.trim();
    let data: Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => {
            return Ok(SchemaValidationResult {
                direction: context.to_string(),
                raw_text: trimmed.to_string(),
                violations: vec![SchemaViolation {
                    message: "not valid JSON".to_string(),
                    instance_path: String::new(),
                    schema_path: String::new(),
                }],
            });
        }
    };
    let validator =
        jsonschema::validator_for(schema).map_err(|e| anyhow!("Invalid {context} schema: {e}"))?;
    let violations: Vec<SchemaViolation> = validator
        .iter_errors(&data)
        .map(|e| SchemaViolation {
            message: e.to_string(),
            instance_path: e.instance_path.to_string(),
            schema_path: e.schema_path.to_string(),
        })
        .collect();
    Ok(SchemaValidationResult {
        direction: context.to_string(),
        raw_text: trimmed.to_string(),
        violations,
    })
}

pub fn validate_schema(context: &str, schema: &Value, text: &str) -> Result<()> {
    let result = validate_schema_detailed(context, schema, text)?;
    if !result.is_ok() {
        if result.violations.len() == 1 && result.violations[0].message == "not valid JSON" {
            bail!("Schema {context} validation failed: not valid JSON");
        }
        bail!("{}", result.terse_error());
    }
    Ok(())
}

/// Validate schema with optional trace emission. When a TraceEmitter is provided,
/// emits a [schema] trace event (human or JSONL) before propagating any error.
pub fn validate_schema_traced(
    context: &str,
    schema: &Value,
    text: &str,
    tracer: Option<&crate::utils::trace::TraceEmitter>,
) -> Result<()> {
    match tracer {
        Some(t) => {
            let result = validate_schema_detailed(context, schema, text)?;
            t.emit_schema_validation(&result);
            if !result.is_ok() {
                if result.violations.len() == 1 && result.violations[0].message == "not valid JSON"
                {
                    bail!("Schema {context} validation failed: not valid JSON");
                }
                bail!("{}", result.terse_error());
            }
            Ok(())
        }
        None => validate_schema(context, schema, text),
    }
}

/// Phase 6B: Execute lifecycle hooks after LLM output is generated.
pub fn run_lifecycle_hooks(role: &Role, output: &str) -> Result<()> {
    if let Some(pipe_to) = role.pipe_to() {
        pipe_output_to_command(pipe_to, output)?;
    }
    if let Some(save_to) = role.save_to() {
        save_output_to_path(save_to, output)?;
    }
    Ok(())
}

fn pipe_output_to_command(cmd: &str, output: &str) -> Result<()> {
    use std::io::Write;
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to spawn pipe_to command: {cmd}"))?;
    if let Some(ref mut stdin) = child.stdin {
        stdin
            .write_all(output.as_bytes())
            .with_context(|| format!("Failed to write to pipe_to command: {cmd}"))?;
    }
    let status = child
        .wait()
        .with_context(|| format!("Failed to wait for pipe_to command: {cmd}"))?;
    if !status.success() {
        warn!("pipe_to command '{cmd}' exited with {status}");
    }
    Ok(())
}

fn save_output_to_path(template: &str, output: &str) -> Result<()> {
    let path = template.replace("{{timestamp}}", &now());
    let path = std::path::Path::new(&path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create directory for save_to: {}",
                parent.display()
            )
        })?;
    }
    std::fs::write(path, output)
        .with_context(|| format!("Failed to write save_to: {}", path.display()))?;
    debug!(
        "save_to: wrote {} bytes to {}",
        output.len(),
        path.display()
    );
    Ok(())
}

/// Phase 13A: produce the contents of a new role file that `extends:` an
/// existing one. The parent's metadata is inspected so the new file's
/// frontmatter carries commented-out hints for the fields the parent
/// declares (model/temperature/top_p/use_tools/input_schema/output_schema).
/// The parent prompt body is *not* duplicated — it is inherited via the
/// extends chain. Pure function so it can be unit-tested without touching
/// the filesystem.
pub fn render_forked_role(source_name: &str, parent_metadata: &serde_json::Map<String, Value>) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "extends: {source_name}");

    // Order the hint lines the way a reader scans frontmatter: model first,
    // then sampling, then I/O contracts, then tools. Each commented line
    // shows the parent's current value so the fork starts as a no-op
    // override the user can quickly toggle on.
    let hint_keys: &[&str] = &[
        "model",
        "temperature",
        "top_p",
        "use_tools",
        "input_schema",
        "output_schema",
    ];
    for key in hint_keys {
        if let Some(value) = parent_metadata.get(*key) {
            push_commented_yaml_field(&mut out, key, value);
        } else {
            push_commented_placeholder(&mut out, key);
        }
    }
    out.push_str("---\n");

    // Encourage the writer to put *additions* here. The parent prompt is
    // inherited; this body is appended after it during resolution (see
    // `resolve_role_content`).
    out.push_str("# Add your prompt additions here. The parent prompt is inherited.\n");
    out
}

/// Phase 13A: format a single frontmatter field as YAML and prefix each line
/// with `# ` so it sits in the new file as a hint, not an active override.
/// Handles scalar values inline and falls back to multi-line emission for
/// objects/arrays (input_schema, output_schema).
fn push_commented_yaml_field(out: &mut String, key: &str, value: &Value) {
    use std::fmt::Write;

    match value {
        Value::String(s) => {
            let _ = writeln!(out, "# {key}: {}", yaml_scalar(s));
        }
        Value::Bool(b) => {
            let _ = writeln!(out, "# {key}: {b}");
        }
        Value::Number(n) => {
            let _ = writeln!(out, "# {key}: {n}");
        }
        Value::Null => {
            let _ = writeln!(out, "# {key}: null");
        }
        _ => {
            // Object/array — emit on multiple lines so the YAML stays valid
            // when the user uncomments.
            let yaml = serde_yaml::to_string(value).unwrap_or_else(|_| "{}".to_string());
            let _ = writeln!(out, "# {key}:");
            for line in yaml.lines() {
                if line.is_empty() {
                    continue;
                }
                let _ = writeln!(out, "#   {line}");
            }
        }
    }
}

fn push_commented_placeholder(out: &mut String, key: &str) {
    use std::fmt::Write;

    let placeholder = match key {
        "model" => "claude:claude-sonnet-4-6",
        "temperature" => "0.7",
        "top_p" => "1.0",
        "use_tools" => "search,fs",
        "input_schema" => "{ type: object, properties: { ... } }",
        "output_schema" => "{ type: object, properties: { ... } }",
        _ => "<value>",
    };
    let _ = writeln!(out, "# {key}: {placeholder}");
}

/// Quote a YAML scalar only if it'd be parsed as something other than a
/// plain string (contains a colon, leading dash, etc). Good enough for the
/// commented hints — the user is going to read and edit these by hand.
fn yaml_scalar(s: &str) -> String {
    let needs_quotes = s.is_empty()
        || s.starts_with(' ')
        || s.starts_with('-')
        || s.starts_with('#')
        || s.contains(':')
        || s.contains('"')
        || s.contains('\'')
        || s.contains('\n');
    if needs_quotes {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// Phase 13A: source role lookup that exposes its raw frontmatter (not the
/// resolved/merged one). Used by `fork_role` so the hint comments reflect
/// what the parent actually declares — without inheriting its own parent's
/// fields a second time.
pub fn read_role_raw_metadata(name: &str) -> Result<serde_json::Map<String, Value>> {
    let content = read_raw_role_content(name)?;
    let parts = parse_raw_frontmatter(&content);
    Ok(parts.metadata)
}

/// Phase 13B: format a multi-line teaching error for a pipeline stage whose
/// input failed schema validation. Shows the actual JSON keys produced by
/// the upstream stage (or the raw text if non-JSON) alongside the consumer
/// schema's required/declared properties, plus a hint suggesting a transform
/// role between the two. Returns `None` when the input parses but is not an
/// object — there's no key-level diff to compute in that case, so the caller
/// should fall back to the terse error.
pub fn format_pipeline_input_schema_error(
    stage_num: usize,
    role_name: &str,
    schema: &Value,
    input_text: &str,
    underlying: &str,
) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "pipeline stage {stage_num} input schema validation failed (role '{role_name}'):"
    );
    let _ = writeln!(out, "  {}", underlying.trim());

    let (consumer_required, consumer_props) = schema_object_field_names(schema);
    let produced = parse_input_field_names(input_text);

    out.push('\n');
    match produced {
        Some(ref keys) => {
            let _ = writeln!(out, "  Stage {} produced: {}", stage_num - 1, format_field_list(keys));
        }
        None => {
            let preview: String = input_text.chars().take(60).collect();
            let suffix = if input_text.chars().count() > 60 { "..." } else { "" };
            let _ = writeln!(
                out,
                "  Stage {} produced: <non-JSON> {:?}{}",
                stage_num - 1,
                preview,
                suffix
            );
        }
    }
    let consumer_summary = if !consumer_required.is_empty() {
        format_field_list(&consumer_required)
    } else if !consumer_props.is_empty() {
        format_field_list(&consumer_props)
    } else {
        "<no declared properties>".to_string()
    };
    let _ = writeln!(out, "  Stage {stage_num} expects: {consumer_summary}");

    if let Some(producer_keys) = produced {
        let missing: Vec<&String> = consumer_required
            .iter()
            .filter(|k| !producer_keys.contains(k))
            .collect();
        let extra: Vec<&String> = producer_keys
            .iter()
            .filter(|k| !consumer_props.contains(k) && !consumer_required.contains(k))
            .collect();
        if !missing.is_empty() {
            let _ = writeln!(
                out,
                "  Missing fields: {}",
                missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            );
        }
        if !extra.is_empty() {
            let _ = writeln!(
                out,
                "  Extra fields: {}",
                extra.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            );
        }
    }
    out.push('\n');
    out.push_str(
        "  hint: shape mismatches between adjacent stages are usually fixed by a\n\
         \x20       transform role between them. To start one:\n\
         \x20       aichat --fork-role <parent> my-adapter\n",
    );
    out
}

fn schema_object_field_names(schema: &Value) -> (Vec<String>, Vec<String>) {
    let required = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|obj| obj.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    (required, props)
}

fn parse_input_field_names(input_text: &str) -> Option<Vec<String>> {
    let parsed: Value = serde_json::from_str(input_text.trim()).ok()?;
    let obj = parsed.as_object()?;
    Some(obj.keys().cloned().collect())
}

fn format_field_list(keys: &[String]) -> String {
    if keys.is_empty() {
        "{}".to_string()
    } else {
        format!("{{ {} }}", keys.join(", "))
    }
}

/// Phase 13D: snapshot of a resolved role's authoring-relevant fields, in a
/// form that's easy to render either as human-readable text or as JSON.
/// Lives here so both the CLI renderer and any future surface (server,
/// pi extension) can reuse the same shape.
#[derive(Debug, Clone, Serialize)]
pub struct RoleExplanation {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    pub builtin: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub input: String,
    pub output: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knowledge: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fallback_models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipe_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save_to: Option<String>,
    pub has_pipeline: bool,
    pub pipeline_stage_count: usize,
    pub pipeline_has_dag: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pipeline_stage_roles: Vec<String>,
    pub embedded_input: bool,
    pub prompt_length: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_extends: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub raw_includes: Vec<String>,
}

/// Phase 13D: build an explanation for the given role name. The role is
/// loaded via the normal resolution path (so `extends:` is applied and the
/// prompt is fully materialized) AND its raw frontmatter is re-read to
/// expose the `extends:`/`include:` declarations that resolution would
/// otherwise flatten away.
pub fn build_role_explanation(name: &str) -> Result<RoleExplanation> {
    let role = Role::resolve(name)?;

    // Re-read the raw file so we can show extends/include even after
    // resolution has merged them.
    let raw = parse_raw_frontmatter(&read_raw_role_content(name)?);

    // A role is "builtin" iff there's no user-config file shadowing it.
    let on_disk = Config::list_roles(false);
    let builtin = !on_disk.contains(&name.to_string())
        && Role::list_builtin_role_names().contains(&name.to_string());
    let source_path = if builtin {
        Some(format!("<builtin asset: {name}.md>"))
    } else if on_disk.contains(&name.to_string()) {
        Some(Config::role_file(name).display().to_string())
    } else {
        None
    };

    let tools_str = role.use_tools().unwrap_or_default();
    let tools: Vec<String> = tools_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let knowledge = if role.knowledge_bindings().is_empty() {
        None
    } else {
        Some(
            role.knowledge_bindings()
                .iter()
                .map(|b| b.name.clone())
                .collect::<Vec<_>>(),
        )
    };

    let pipeline_stage_roles: Vec<String> = role
        .pipeline_all_stages()
        .iter()
        .map(|s| s.role.clone())
        .collect();

    Ok(RoleExplanation {
        name: role.name().to_string(),
        description: role.description_or_derived(),
        source_path,
        builtin,
        model: role.model_id().map(String::from),
        temperature: role.temperature(),
        top_p: role.top_p(),
        capabilities: role.capabilities().to_vec(),
        tags: role.tags().map(|s| s.to_vec()).unwrap_or_default(),
        input: role.port_input_summary(),
        output: role.port_output_summary(),
        tools,
        knowledge,
        fallback_models: role.fallback_models().to_vec(),
        pipe_to: role.pipe_to().map(String::from),
        save_to: role.save_to().map(String::from),
        has_pipeline: role.is_pipeline(),
        pipeline_stage_count: role.pipeline_all_stages().len(),
        pipeline_has_dag: role.pipeline_has_dag(),
        pipeline_stage_roles,
        embedded_input: role.is_embedded_prompt(),
        prompt_length: role.prompt().len(),
        raw_extends: raw.extends,
        raw_includes: raw.includes,
    })
}

/// Phase 13D: render an explanation as human-readable text. JSON output is
/// handled by the caller via `serde_json::to_string_pretty(&exp)`.
pub fn format_role_explanation(exp: &RoleExplanation) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let _ = writeln!(out, "Role: {}", exp.name);
    if !exp.description.is_empty() {
        let _ = writeln!(out, "  description: {}", exp.description);
    }
    if let Some(path) = &exp.source_path {
        let _ = writeln!(out, "  source: {path}");
    }
    if let Some(model) = &exp.model {
        let _ = writeln!(out, "  model: {model}");
    } else {
        out.push_str("  model: <default>\n");
    }
    if !exp.fallback_models.is_empty() {
        let _ = writeln!(out, "  fallback_models: [{}]", exp.fallback_models.join(", "));
    }
    if let Some(t) = exp.temperature {
        let _ = writeln!(out, "  temperature: {t}");
    }
    if let Some(p) = exp.top_p {
        let _ = writeln!(out, "  top_p: {p}");
    }
    let _ = writeln!(out, "  in: {}  out: {}", exp.input, exp.output);
    if !exp.capabilities.is_empty() {
        let _ = writeln!(out, "  capabilities: [{}]", exp.capabilities.join(", "));
    }
    if !exp.tags.is_empty() {
        let _ = writeln!(out, "  tags: [{}]", exp.tags.join(", "));
    }
    if !exp.tools.is_empty() {
        let _ = writeln!(out, "  tools: [{}]", exp.tools.join(", "));
    }
    if let Some(kbs) = &exp.knowledge {
        let _ = writeln!(out, "  knowledge: [{}]", kbs.join(", "));
    }
    if let Some(parent) = &exp.raw_extends {
        let _ = writeln!(out, "  extends: {parent}");
    }
    if !exp.raw_includes.is_empty() {
        let _ = writeln!(out, "  includes: [{}]", exp.raw_includes.join(", "));
    }
    if exp.has_pipeline {
        let kind = if exp.pipeline_has_dag { "DAG" } else { "sequential" };
        let _ = writeln!(
            out,
            "  pipeline: {} stage{} ({kind})",
            exp.pipeline_stage_count,
            if exp.pipeline_stage_count == 1 { "" } else { "s" }
        );
        if !exp.pipeline_stage_roles.is_empty() {
            let _ = writeln!(out, "    stages: {}", exp.pipeline_stage_roles.join(" -> "));
        }
    }
    if let Some(pipe) = &exp.pipe_to {
        let _ = writeln!(out, "  pipe_to: {pipe}");
    }
    if let Some(save) = &exp.save_to {
        let _ = writeln!(out, "  save_to: {save}");
    }
    let _ = writeln!(
        out,
        "  prompt: {} char{}{}",
        exp.prompt_length,
        if exp.prompt_length == 1 { "" } else { "s" },
        if exp.embedded_input { " (embeds __INPUT__)" } else { "" }
    );
    out
}

/// Phase 14B: render a JSON Schema fragment into a one-line human-readable
/// type. Used for `port_input_summary` and `port_output_summary`. Returns
/// `"any"` when no schema is declared so the same helper drives both ports.
fn port_summary_from_schema(schema: Option<&Value>) -> String {
    let Some(schema) = schema else {
        return "any".to_string();
    };
    port_summary_render(schema)
}

/// For the output port, treat "no schema" as `"text"` rather than `"any"` —
/// the LLM produces free text unless told otherwise.
fn port_summary_from_schema_for_output(schema: Option<&Value>) -> String {
    let Some(schema) = schema else {
        return "text".to_string();
    };
    port_summary_render(schema)
}

fn port_summary_render(schema: &Value) -> String {
    let kind = schema.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match kind {
        "string" => "text".to_string(),
        "array" => "array".to_string(),
        "object" => {
            // List the top-level property names, comma-joined inside `json{}`.
            // No properties? Just `"json"` — reflects an open-shape object.
            if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
                if props.is_empty() {
                    "json".to_string()
                } else {
                    let names: Vec<&str> = props.keys().map(|s| s.as_str()).collect();
                    format!("json{{{}}}", names.join(", "))
                }
            } else {
                "json".to_string()
            }
        }
        "" => "json".to_string(),
        other => other.to_string(),
    }
}

/// Tolerant match between a port summary and a user-supplied query.
/// `"json"` matches `"json{...}"`; otherwise prefix-match is used so that
/// `"text"` matches itself but not `"text-something"` accidentally.
fn port_signature_matches(summary: &str, query: &str) -> bool {
    let s = summary.trim();
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    if s == q {
        return true;
    }
    // `json` is the broad family; `json{...}` is a narrower form.
    if q == "json" && (s == "json" || s.starts_with("json{")) {
        return true;
    }
    false
}

fn parse_structure_prompt(prompt: &str) -> (&str, Vec<(&str, &str)>) {
    let mut text = prompt;
    let mut search_input = true;
    let mut system = None;
    let mut parts = vec![];
    loop {
        let search = if search_input {
            "### INPUT:"
        } else {
            "### OUTPUT:"
        };
        match text.find(search) {
            Some(idx) => {
                if system.is_none() {
                    system = Some(&text[..idx])
                } else {
                    parts.push(&text[..idx])
                }
                search_input = !search_input;
                text = &text[(idx + search.len())..];
            }
            None => {
                if !text.is_empty() {
                    if system.is_none() {
                        system = Some(text)
                    } else {
                        parts.push(text)
                    }
                }
                break;
            }
        }
    }
    let parts_len = parts.len();
    if parts_len > 0 && parts_len % 2 == 0 {
        let cases: Vec<(&str, &str)> = parts
            .iter()
            .step_by(2)
            .zip(parts.iter().skip(1).step_by(2))
            .map(|(i, o)| (i.trim(), o.trim()))
            .collect();
        let system = system.map(|v| v.trim()).unwrap_or_default();
        return (system, cases);
    }

    (prompt, vec![])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- Phase 33B: type-aware slot rendering ----

    #[test]
    fn render_slot_string_passes_through_unquoted() {
        assert_eq!(render_slot(&json!("main"), false), "main");
    }

    #[test]
    fn render_slot_scalars_render_bare() {
        assert_eq!(render_slot(&json!(3), false), "3");
        assert_eq!(render_slot(&json!(true), false), "true");
        assert_eq!(render_slot(&json!(1.5), false), "1.5");
    }

    #[test]
    fn render_slot_null_is_empty() {
        assert_eq!(render_slot(&json!(null), false), "");
    }

    #[test]
    fn render_slot_array_is_compact_json_by_default() {
        assert_eq!(render_slot(&json!(["a", "b"]), false), r#"["a","b"]"#);
    }

    #[test]
    fn render_slot_object_is_compact_json_by_default() {
        assert_eq!(render_slot(&json!({"k": 1}), false), r#"{"k":1}"#);
    }

    #[test]
    fn render_slot_pretty_expands_arrays() {
        let out = render_slot(&json!(["a", "b"]), true);
        assert!(out.contains('\n'), "pretty render should be multi-line: {out}");
        assert!(out.contains("\"a\""));
    }

    // ---- Phase 33A: schema slot extraction ----

    fn slot<'a>(slots: &'a [SchemaSlot], name: &str) -> &'a SchemaSlot {
        slots.iter().find(|s| s.name == name).expect("slot present")
    }

    #[test]
    fn schema_slots_empty_without_properties() {
        assert!(schema_slots(&json!({"type": "object"})).is_empty());
    }

    #[test]
    fn schema_slots_reads_literal_defaults_by_type() {
        let schema = json!({
            "type": "object",
            "properties": {
                "target": { "type": "string", "default": "main" },
                "depth":  { "type": "integer", "default": 3 },
                "tags":   { "type": "array", "default": ["a", "b"] }
            }
        });
        let slots = schema_slots(&schema);
        assert_eq!(slot(&slots, "target").default, Some(SlotDefault::Literal(json!("main"))));
        assert_eq!(slot(&slots, "depth").default, Some(SlotDefault::Literal(json!(3))));
        assert_eq!(slot(&slots, "tags").default, Some(SlotDefault::Literal(json!(["a", "b"]))));
    }

    #[test]
    fn schema_slots_reads_shell_default() {
        let schema = json!({
            "type": "object",
            "properties": { "today": { "type": "string", "default": { "shell": "date +%F" } } }
        });
        let slots = schema_slots(&schema);
        assert_eq!(slot(&slots, "today").default, Some(SlotDefault::Shell("date +%F".to_string())));
    }

    #[test]
    fn schema_slots_marks_required_and_pretty() {
        let schema = json!({
            "type": "object",
            "properties": {
                "files": { "type": "array", "x-aichat": { "render": "pretty" } },
                "target": { "type": "string", "default": "main" }
            },
            "required": ["files"]
        });
        let slots = schema_slots(&schema);
        let files = slot(&slots, "files");
        assert!(files.required, "files is required");
        assert!(files.pretty, "files renders pretty");
        assert!(files.default.is_none());
        assert!(!slot(&slots, "target").required);
    }

    // ---- Phase 33A/33B/33E: resolve_slots (unification core) ----

    fn vars(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn resolve_slots_fills_schema_literal_default() {
        let schema = json!({
            "type": "object",
            "properties": { "target": { "type": "string", "default": "main" } }
        });
        let out = resolve_slots(&[], Some(&schema), None).unwrap();
        assert_eq!(out.get("target").map(String::as_str), Some("main"));
    }

    #[test]
    fn resolve_slots_cli_overrides_schema_default() {
        let schema = json!({
            "type": "object",
            "properties": { "target": { "type": "string", "default": "main" } }
        });
        let cli = vars(&[("target", "dev")]);
        let out = resolve_slots(&[], Some(&schema), Some(&cli)).unwrap();
        assert_eq!(out.get("target").map(String::as_str), Some("dev"));
    }

    #[test]
    fn resolve_slots_renders_typed_defaults() {
        let schema = json!({
            "type": "object",
            "properties": {
                "depth": { "type": "integer", "default": 3 },
                "tags":  { "type": "array", "default": ["a", "b"] }
            }
        });
        let out = resolve_slots(&[], Some(&schema), None).unwrap();
        assert_eq!(out.get("depth").map(String::as_str), Some("3"));
        assert_eq!(out.get("tags").map(String::as_str), Some(r#"["a","b"]"#));
    }

    #[test]
    fn resolve_slots_skips_required_property_without_value() {
        // A required schema property with no default and no -v must NOT error
        // here — message validation still enforces it. It is simply absent.
        let schema = json!({
            "type": "object",
            "properties": { "files": { "type": "array" } },
            "required": ["files"]
        });
        let out = resolve_slots(&[], Some(&schema), None).unwrap();
        assert!(!out.contains_key("files"), "unresolved required prop is skipped, not errored");
    }

    #[test]
    fn resolve_slots_variable_required_without_value_errors() {
        let var = RoleVariable { name: "who".into(), default: None };
        let err = resolve_slots(&[var], None, None).unwrap_err();
        assert!(err.to_string().contains("required but not provided"), "{err}");
    }

    #[test]
    fn resolve_slots_variable_default_and_cli() {
        let var = RoleVariable {
            name: "target".into(),
            default: Some(VariableDefault::Value("main".into())),
        };
        let out = resolve_slots(std::slice::from_ref(&var), None, None).unwrap();
        assert_eq!(out.get("target").map(String::as_str), Some("main"));

        let cli = vars(&[("target", "dev")]);
        let out = resolve_slots(&[var], None, Some(&cli)).unwrap();
        assert_eq!(out.get("target").map(String::as_str), Some("dev"));
    }

    #[test]
    fn resolve_slots_schema_wins_on_name_collision() {
        // A variable and a schema property share a name; the schema's typed
        // default is the source of truth.
        let var = RoleVariable {
            name: "depth".into(),
            default: Some(VariableDefault::Value("legacy".into())),
        };
        let schema = json!({
            "type": "object",
            "properties": { "depth": { "type": "integer", "default": 7 } }
        });
        let out = resolve_slots(&[var], Some(&schema), None).unwrap();
        assert_eq!(out.get("depth").map(String::as_str), Some("7"));
    }

    #[test]
    fn resolve_slots_shell_default_runs() {
        let schema = json!({
            "type": "object",
            "properties": { "greeting": { "type": "string", "default": { "shell": "printf hi" } } }
        });
        let out = resolve_slots(&[], Some(&schema), None).unwrap();
        assert_eq!(out.get("greeting").map(String::as_str), Some("hi"));
    }

    // ---- Phase 33C: CLI value coercion ----

    #[test]
    fn coerce_string_and_undeclared_keep_raw() {
        assert_eq!(coerce_cli_value("a", "hello", Some("string")).unwrap(), json!("hello"));
        assert_eq!(coerce_cli_value("a", "hello", None).unwrap(), json!("hello"));
    }

    #[test]
    fn coerce_integer_parses_or_errors() {
        assert_eq!(coerce_cli_value("depth", "5", Some("integer")).unwrap(), json!(5));
        let err = coerce_cli_value("depth", "abc", Some("integer")).unwrap_err().to_string();
        assert!(err.contains("depth") && err.contains("integer"), "{err}");
    }

    #[test]
    fn coerce_number_and_boolean() {
        assert_eq!(coerce_cli_value("x", "1.5", Some("number")).unwrap(), json!(1.5));
        assert_eq!(coerce_cli_value("ok", "true", Some("boolean")).unwrap(), json!(true));
        assert!(coerce_cli_value("ok", "yes", Some("boolean")).is_err());
    }

    #[test]
    fn coerce_array_and_object_parse_json() {
        assert_eq!(
            coerce_cli_value("files", r#"["a","b"]"#, Some("array")).unwrap(),
            json!(["a", "b"])
        );
        assert_eq!(
            coerce_cli_value("cfg", r#"{"k":1}"#, Some("object")).unwrap(),
            json!({"k": 1})
        );
    }

    #[test]
    fn coerce_array_rejects_non_array_json() {
        // Valid JSON, but a scalar where an array was declared.
        let err = coerce_cli_value("files", "42", Some("array")).unwrap_err().to_string();
        assert!(err.contains("files") && err.contains("array"), "{err}");
    }

    #[test]
    fn coerce_at_file_reads_json_for_container_and_text_for_string() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let json_path = dir.path().join("cfg.json");
        std::fs::File::create(&json_path).unwrap().write_all(br#"{"k":1}"#).unwrap();
        let arg = format!("@{}", json_path.display());
        assert_eq!(coerce_cli_value("cfg", &arg, Some("object")).unwrap(), json!({"k": 1}));

        let txt_path = dir.path().join("note.txt");
        std::fs::File::create(&txt_path).unwrap().write_all(b"plain text").unwrap();
        let targ = format!("@{}", txt_path.display());
        assert_eq!(coerce_cli_value("body", &targ, Some("string")).unwrap(), json!("plain text"));
    }

    #[test]
    fn stdin_slot_finds_annotated_property() {
        let schema = json!({
            "type": "object",
            "properties": {
                "target": { "type": "string" },
                "body": { "type": "string", "x-aichat": { "source": "stdin" } }
            }
        });
        assert_eq!(stdin_slot(&schema).as_deref(), Some("body"));
    }

    #[test]
    fn stdin_slot_honors_custom_name() {
        let schema = json!({
            "type": "object",
            "properties": { "payload": { "type": "string", "x-aichat": { "source": "stdin" } } }
        });
        assert_eq!(stdin_slot(&schema).as_deref(), Some("payload"));
    }

    #[test]
    fn stdin_slot_none_when_unannotated() {
        let schema = json!({
            "type": "object",
            "properties": { "target": { "type": "string", "default": "main" } }
        });
        assert_eq!(stdin_slot(&schema), None);
    }

    #[test]
    fn coerce_cli_value_flows_through_resolve_slots() {
        let schema = json!({
            "type": "object",
            "properties": { "depth": { "type": "integer", "default": 1 } }
        });
        // A bad -v value for a typed slot fails resolution (coercion is wired in).
        let bad = vars(&[("depth", "abc")]);
        assert!(
            resolve_slots(&[], Some(&schema), Some(&bad)).is_err(),
            "non-integer -v for an integer slot must error through resolve_slots"
        );
        // A good value coerces and renders.
        let good = vars(&[("depth", "5")]);
        let out = resolve_slots(&[], Some(&schema), Some(&good)).unwrap();
        assert_eq!(out.get("depth").map(String::as_str), Some("5"));
    }

    #[test]
    fn test_parse_structure_prompt1() {
        let prompt = r#"
System message
### INPUT:
Input 1
### OUTPUT:
Output 1
"#;
        assert_eq!(
            parse_structure_prompt(prompt),
            ("System message", vec![("Input 1", "Output 1")])
        );
    }

    #[test]
    fn test_parse_structure_prompt2() {
        let prompt = r#"
### INPUT:
Input 1
### OUTPUT:
Output 1
"#;
        assert_eq!(
            parse_structure_prompt(prompt),
            ("", vec![("Input 1", "Output 1")])
        );
    }

    #[test]
    fn test_parse_structure_prompt3() {
        let prompt = r#"
System message
### INPUT:
Input 1
"#;
        assert_eq!(parse_structure_prompt(prompt), (prompt, vec![]));
    }

    #[test]
    fn test_parse_raw_frontmatter_basic() {
        let content = "---\nmodel: gpt-4\ntemperature: 0.5\n---\nYou are helpful.";
        let parts = parse_raw_frontmatter(content);
        assert_eq!(parts.prompt, "You are helpful.");
        assert!(parts.extends.is_none());
        assert!(parts.includes.is_empty());
        assert_eq!(
            parts.metadata.get("model"),
            Some(&serde_json::Value::String("gpt-4".to_string()))
        );
        assert_eq!(
            parts.metadata.get("temperature"),
            Some(&serde_json::json!(0.5))
        );
    }

    #[test]
    fn test_parse_raw_frontmatter_extends() {
        let content = "---\nextends: \"%code%\"\ntemperature: 0.3\n---\nFocus on security.";
        let parts = parse_raw_frontmatter(content);
        assert_eq!(parts.extends, Some("%code%".to_string()));
        assert_eq!(parts.prompt, "Focus on security.");
        // extends should NOT be in metadata
        assert!(parts.metadata.get("extends").is_none());
        assert_eq!(
            parts.metadata.get("temperature"),
            Some(&serde_json::json!(0.3))
        );
    }

    #[test]
    fn test_parse_raw_frontmatter_include() {
        let content =
            "---\ninclude:\n  - safety-guardrails\n  - output-json\n---\nYou are a data analyst.";
        let parts = parse_raw_frontmatter(content);
        assert_eq!(
            parts.includes,
            vec!["safety-guardrails".to_string(), "output-json".to_string()]
        );
        assert_eq!(parts.prompt, "You are a data analyst.");
        // include should NOT be in metadata
        assert!(parts.metadata.get("include").is_none());
    }

    #[test]
    fn test_parse_raw_frontmatter_no_frontmatter() {
        let content = "Just a plain prompt.";
        let parts = parse_raw_frontmatter(content);
        assert_eq!(parts.prompt, "Just a plain prompt.");
        assert!(parts.extends.is_none());
        assert!(parts.includes.is_empty());
        assert!(parts.metadata.is_empty());
    }

    #[test]
    fn test_resolve_builtin_passthrough() {
        // Builtins with no extends/include should resolve unchanged
        let parts = resolve_role_content("%code%", &mut Vec::new());
        assert!(parts.is_ok());
        let parts = parts.unwrap();
        assert!(parts.extends.is_none());
        assert!(parts.includes.is_empty());
        assert!(parts.prompt.contains("Provide only code"));
    }

    #[test]
    fn test_metadata_merge() {
        // Simulate parent with temperature 0.5 and child with temperature 0.8
        let mut parent = RawRoleParts {
            metadata: serde_json::Map::new(),
            prompt: "Parent prompt.".to_string(),
            extends: None,
            includes: Vec::new(),
            variables: Vec::new(),
        };
        parent
            .metadata
            .insert("temperature".to_string(), serde_json::json!(0.5));
        parent
            .metadata
            .insert("model".to_string(), serde_json::json!("gpt-4"));

        let mut child_metadata = serde_json::Map::new();
        child_metadata.insert("temperature".to_string(), serde_json::json!(0.8));

        // Merge: parent defaults, child overrides
        let mut merged = parent.metadata.clone();
        for (key, value) in child_metadata {
            merged.insert(key, value);
        }

        assert_eq!(merged.get("temperature"), Some(&serde_json::json!(0.8)));
        assert_eq!(merged.get("model"), Some(&serde_json::json!("gpt-4")));
    }

    #[test]
    fn test_prompt_ordering() {
        // Verify ordering: includes -> parent -> child
        let include_prompt = "Safety first.";
        let parent_prompt = "Be helpful.";
        let child_prompt = "Focus on code review.";

        let mut prompt_parts = vec![include_prompt.to_string()];
        prompt_parts.push(parent_prompt.to_string());
        prompt_parts.push(child_prompt.to_string());
        let result = prompt_parts.join("\n\n");

        assert_eq!(
            result,
            "Safety first.\n\nBe helpful.\n\nFocus on code review."
        );
        // Verify ordering
        let safety_pos = result.find("Safety first.").unwrap();
        let helpful_pos = result.find("Be helpful.").unwrap();
        let review_pos = result.find("Focus on code review.").unwrap();
        assert!(safety_pos < helpful_pos);
        assert!(helpful_pos < review_pos);
    }

    #[test]
    fn test_cycle_detection() {
        // Simulate cycle: A -> B -> A
        let mut visited = vec!["A".to_string(), "B".to_string()];
        let result = resolve_role_content("A", &mut visited);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Circular role inheritance"),
            "Error should mention circular inheritance: {err}"
        );
        assert!(
            err.contains("A -> B -> A"),
            "Error should show chain: {err}"
        );
    }

    #[test]
    fn test_compose_role_content_no_metadata() {
        let parts = RawRoleParts {
            metadata: serde_json::Map::new(),
            prompt: "Just a prompt.".to_string(),
            extends: None,
            includes: Vec::new(),
            variables: Vec::new(),
        };
        let content = compose_role_content(&parts);
        assert_eq!(content, "Just a prompt.");
    }

    #[test]
    fn test_role_with_schemas() {
        let content = r#"---
model: openai:gpt-4o
output_schema:
  type: object
  properties:
    entities:
      type: array
      items:
        type: object
        properties:
          name:
            type: string
          type:
            type: string
        required: [name, type]
  required: [entities]
---
Extract all named entities."#;
        let role = Role::new("test-schema", content);
        assert!(role.output_schema().is_some());
        assert!(role.has_output_schema());
        assert!(role.input_schema().is_none());
        let schema = role.output_schema().unwrap();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["entities"].is_object());
    }

    #[test]
    fn test_validate_schema_success() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        });
        let result = validate_schema("output", &schema, r#"{"name": "Alice"}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_schema_failure() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        });
        let result = validate_schema("output", &schema, r#"{"age": 30}"#);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Schema output validation failed"));
    }

    #[test]
    fn test_validate_schema_not_json() {
        let schema = serde_json::json!({ "type": "object" });
        let result = validate_schema("input", &schema, "not json at all");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not valid JSON"));
    }

    #[test]
    fn test_role_capabilities_parsed_and_exported() {
        let content = r#"---
capabilities: [code-review, security-audit, rust]
---
You are a code reviewer."#;
        let role = Role::new("reviewer", content);
        assert_eq!(
            role.capabilities(),
            &[
                "code-review".to_string(),
                "security-audit".to_string(),
                "rust".to_string()
            ]
        );
        // Round-trip: export and re-parse preserves capabilities
        let exported = role.export();
        assert!(exported.contains("capabilities"));
        let reparsed = Role::new("reviewer", &exported);
        assert_eq!(reparsed.capabilities(), role.capabilities());
    }

    #[test]
    fn test_role_capabilities_empty_when_absent() {
        let content = "---\nmodel: gpt-4\n---\nNo caps declared.";
        let role = Role::new("plain", content);
        assert!(role.capabilities().is_empty());
        // Empty capabilities should not appear in the exported frontmatter
        assert!(!role.export().contains("capabilities"));
    }

    #[test]
    fn test_port_summary_no_schemas() {
        let role = Role::new("plain", "---\n---\nHi.");
        assert_eq!(role.port_input_summary(), "any");
        assert_eq!(role.port_output_summary(), "text");
    }

    #[test]
    fn test_port_summary_string_schema() {
        let content = r#"---
input_schema:
  type: string
---
text in."#;
        let role = Role::new("text-in", content);
        assert_eq!(role.port_input_summary(), "text");
    }

    #[test]
    fn test_port_summary_object_schema_lists_properties() {
        let content = r#"---
input_schema:
  type: object
  properties:
    code: { type: string }
    language: { type: string }
output_schema:
  type: object
  properties:
    issues: { type: array }
    severity: { type: string }
---
review."#;
        let role = Role::new("reviewer", content);
        assert_eq!(role.port_input_summary(), "json{code, language}");
        assert_eq!(role.port_output_summary(), "json{issues, severity}");
    }

    #[test]
    fn test_port_accepts_and_produces() {
        let content = r#"---
input_schema:
  type: object
  properties:
    text: { type: string }
output_schema:
  type: object
  properties:
    label: { type: string }
---
classify."#;
        let role = Role::new("classifier", content);
        // Tolerant: bare "json" matches "json{...}"
        assert!(role.port_accepts("json"));
        assert!(role.port_produces("json"));
        // Exact summary matches
        assert!(role.port_accepts("json{text}"));
        assert!(role.port_produces("json{label}"));
        // Mismatched type does not match
        assert!(!role.port_accepts("text"));
        assert!(!role.port_produces("array"));
    }

    #[test]
    fn test_port_accepts_text_for_no_input_schema() {
        // No input_schema → "any". User asking for "any" matches; "text" does not.
        let role = Role::new("plain", "---\n---\nplain.");
        assert!(role.port_accepts("any"));
        assert!(!role.port_accepts("text"));
        // Output defaults to "text" when no output_schema declared
        assert!(role.port_produces("text"));
    }

    #[test]
    fn test_role_without_schemas() {
        let content = "---\nmodel: gpt-4\n---\nYou are helpful.";
        let role = Role::new("no-schema", content);
        assert!(role.input_schema().is_none());
        assert!(role.output_schema().is_none());
        assert!(!role.has_output_schema());
    }

    #[test]
    fn test_compose_role_content_with_metadata() {
        let mut parts = RawRoleParts {
            metadata: serde_json::Map::new(),
            prompt: "You are helpful.".to_string(),
            extends: None,
            includes: Vec::new(),
            variables: Vec::new(),
        };
        parts
            .metadata
            .insert("temperature".to_string(), serde_json::json!(0.5));
        let content = compose_role_content(&parts);
        assert!(content.contains("---"));
        assert!(content.contains("temperature"));
        assert!(content.contains("You are helpful."));
        // Verify it can be parsed back by Role::new
        let role = Role::new("test", &content);
        assert_eq!(role.temperature(), Some(0.5));
        assert!(role.prompt().contains("You are helpful."));
    }

    #[test]
    fn test_parse_role_variables_from_frontmatter() {
        let content = r#"---
variables:
  - name: language
  - name: tone
    default: formal
---
Translate to {{language}} in a {{tone}} tone."#;
        let parts = parse_raw_frontmatter(content);
        assert_eq!(parts.variables.len(), 2);
        assert_eq!(parts.variables[0].name, "language");
        assert!(parts.variables[0].default.is_none());
        assert_eq!(parts.variables[1].name, "tone");
        assert!(matches!(
            &parts.variables[1].default,
            Some(VariableDefault::Value(s)) if s == "formal"
        ));
        // variables should NOT be in metadata
        assert!(parts.metadata.get("variables").is_none());
    }

    #[test]
    fn test_role_variable_with_default() {
        let content = r#"---
variables:
  - name: lang
    default: english
---
Respond in {{lang}}."#;
        let role = Role::new("test-default", content);
        assert_eq!(role.variables().len(), 1);
        assert!(matches!(
            &role.variables()[0].default,
            Some(VariableDefault::Value(s)) if s == "english"
        ));
    }

    #[test]
    fn test_role_variable_apply() {
        let content = r#"---
variables:
  - name: language
---
Translate to {{language}}."#;
        let mut role = Role::new("test-apply", content);
        let mut vars = IndexMap::new();
        vars.insert("language".to_string(), "french".to_string());
        role.apply_variables(&vars);
        assert_eq!(role.prompt(), "Translate to french.");
    }

    #[test]
    fn test_role_variables_empty() {
        let content = "---\nmodel: gpt-4\n---\nYou are helpful.";
        let role = Role::new("no-vars", content);
        assert!(role.variables().is_empty());
    }

    #[test]
    fn test_role_variables_coexist_with_system_vars() {
        let content = r#"---
variables:
  - name: lang
---
OS is {{__os__}}, translate to {{lang}}."#;
        let mut role = Role::new("test-coexist", content);
        let mut vars = IndexMap::new();
        vars.insert("lang".to_string(), "spanish".to_string());
        role.apply_variables(&vars);
        // Role variable replaced, system var still present (resolved later by set_model)
        assert!(role.prompt().contains("translate to spanish"));
        // __os__ is resolved during Role::new via interpolate_variables
        assert!(!role.prompt().contains("{{__os__}}"));
    }

    #[test]
    fn test_dehoist_input_placeholder_auto_tail() {
        // When parent has __INPUT__ and child doesn't, __INPUT__ moves to end
        let parent = RawRoleParts {
            metadata: serde_json::Map::new(),
            prompt: "Parent instructions.\n\nMy request is: __INPUT__".to_string(),
            extends: None,
            includes: Vec::new(),
            variables: Vec::new(),
        };
        let child_prompt = "Child refinement.".to_string();

        // Simulate the de-hoist logic from resolve_role_content
        let parent_has_input = parent.prompt.contains(INPUT_PLACEHOLDER);
        let child_has_input = child_prompt.contains(INPUT_PLACEHOLDER);

        let mut prompt_parts = Vec::new();
        let parent_cleaned = parent
            .prompt
            .replace(INPUT_PLACEHOLDER, "")
            .trim()
            .to_string();
        prompt_parts.push(parent_cleaned);
        prompt_parts.push(child_prompt);
        let mut combined = prompt_parts.join("\n\n");
        if parent_has_input && !child_has_input {
            combined = format!("{combined}\n\n{INPUT_PLACEHOLDER}");
        }

        // __INPUT__ should be at the very end, after child instructions
        assert!(combined.ends_with(INPUT_PLACEHOLDER));
        let input_pos = combined.rfind(INPUT_PLACEHOLDER).unwrap();
        let child_pos = combined.find("Child refinement.").unwrap();
        assert!(
            child_pos < input_pos,
            "Child instructions should precede __INPUT__"
        );
        // Only one __INPUT__ in the result
        assert_eq!(combined.matches(INPUT_PLACEHOLDER).count(), 1);
    }

    #[test]
    fn test_dehoist_input_placeholder_child_wins() {
        // When child re-declares __INPUT__, parent's is stripped and child's is used
        let parent = RawRoleParts {
            metadata: serde_json::Map::new(),
            prompt: "Parent instructions.\n\nMy request is: __INPUT__".to_string(),
            extends: None,
            includes: Vec::new(),
            variables: Vec::new(),
        };
        let child_prompt = "Child instructions.\n\nRewrite this: __INPUT__".to_string();

        let parent_has_input = parent.prompt.contains(INPUT_PLACEHOLDER);
        let child_has_input = child_prompt.contains(INPUT_PLACEHOLDER);

        let mut prompt_parts = Vec::new();
        let parent_cleaned = parent
            .prompt
            .replace(INPUT_PLACEHOLDER, "")
            .trim()
            .to_string();
        prompt_parts.push(parent_cleaned);
        prompt_parts.push(child_prompt);
        let combined = prompt_parts.join("\n\n");

        // Should NOT auto-append since child has __INPUT__
        assert!(child_has_input);
        assert!(parent_has_input);
        // Only one __INPUT__ in the result (the child's)
        assert_eq!(combined.matches(INPUT_PLACEHOLDER).count(), 1);
        // __INPUT__ should appear after "Rewrite this:"
        assert!(combined.contains("Rewrite this: __INPUT__"));
        // Parent's "My request is:" should NOT have __INPUT__
        assert!(!combined.contains("My request is: __INPUT__"));
    }

    // ---- Phase 6A: Shell-injective variables ----

    #[test]
    fn test_shell_variable_default_parsing() {
        let content = r#"---
variables:
  - name: git_diff
    default:
      shell: "echo hello_world"
  - name: language
    default: english
---
Review {{git_diff}} in {{language}}."#;
        let parts = parse_raw_frontmatter(content);
        assert_eq!(parts.variables.len(), 2);
        assert!(matches!(
            &parts.variables[0].default,
            Some(VariableDefault::Shell { shell }) if shell == "echo hello_world"
        ));
        assert!(matches!(
            &parts.variables[1].default,
            Some(VariableDefault::Value(s)) if s == "english"
        ));
    }

    #[test]
    fn test_shell_variable_resolve_success() {
        let default = VariableDefault::Shell {
            shell: "echo hello_shell".to_string(),
        };
        let result = default.resolve();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "hello_shell");
    }

    #[test]
    fn test_shell_variable_resolve_trims_whitespace() {
        let default = VariableDefault::Shell {
            shell: "echo '  padded  '".to_string(),
        };
        let result = default.resolve();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "padded");
    }

    #[test]
    fn test_shell_variable_resolve_failure() {
        let default = VariableDefault::Shell {
            shell: "exit 1".to_string(),
        };
        let result = default.resolve();
        assert!(result.is_err());
    }

    #[test]
    fn test_shell_variable_multiline_output() {
        let default = VariableDefault::Shell {
            shell: "echo 'line1\nline2\nline3'".to_string(),
        };
        let result = default.resolve().unwrap();
        assert!(result.contains("line1"));
        assert!(result.contains("line3"));
    }

    #[test]
    fn test_value_variable_resolve() {
        let default = VariableDefault::Value("plain_value".to_string());
        let result = default.resolve();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "plain_value");
    }

    #[test]
    fn test_shell_variable_in_role_new() {
        let content = r#"---
variables:
  - name: context
    default:
      shell: "echo injected_context"
---
The context is: {{context}}."#;
        let role = Role::new("test-shell-var", content);
        assert_eq!(role.variables().len(), 1);
        assert!(matches!(
            &role.variables()[0].default,
            Some(VariableDefault::Shell { shell }) if shell == "echo injected_context"
        ));
    }

    // ---- Phase 6B: Lifecycle hooks ----

    #[test]
    fn test_pipe_to_parsing() {
        let content = r#"---
pipe_to: pbcopy
---
Summarize this."#;
        let role = Role::new("test-pipe-to", content);
        assert_eq!(role.pipe_to(), Some("pbcopy"));
    }

    #[test]
    fn test_save_to_parsing() {
        let content = r#"---
save_to: "./logs/{{timestamp}}.md"
---
Summarize this."#;
        let role = Role::new("test-save-to", content);
        assert!(role.save_to().unwrap().contains("{{timestamp}}"));
    }

    #[test]
    fn test_both_hooks_parsing() {
        let content = r#"---
pipe_to: "pbcopy"
save_to: "./output.md"
---
Do work."#;
        let role = Role::new("test-both-hooks", content);
        assert_eq!(role.pipe_to(), Some("pbcopy"));
        assert_eq!(role.save_to(), Some("./output.md"));
    }

    #[test]
    fn test_no_hooks_by_default() {
        let content = "---\nmodel: gpt-4\n---\nYou are helpful.";
        let role = Role::new("no-hooks", content);
        assert!(role.pipe_to().is_none());
        assert!(role.save_to().is_none());
    }

    #[test]
    fn test_pipe_output_to_command() {
        // pipe_to "cat" should succeed (eats stdin)
        let result = pipe_output_to_command("cat > /dev/null", "hello world");
        assert!(result.is_ok());
    }

    #[test]
    fn test_save_output_to_path() {
        let dir = std::env::temp_dir().join("aichat_test_save_to");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("output.md");
        let result = save_output_to_path(path.to_str().unwrap(), "test content");
        assert!(result.is_ok());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "test content");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_save_to_timestamp_interpolation() {
        let dir = std::env::temp_dir().join("aichat_test_save_ts");
        let _ = std::fs::remove_dir_all(&dir);
        let template = format!("{}/{{{{timestamp}}}}.md", dir.display());
        let result = save_output_to_path(&template, "timestamped");
        assert!(result.is_ok());
        // The file should exist with a timestamp-based name (not literally "{{timestamp}}")
        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
        let filename = entries[0].as_ref().unwrap().file_name();
        let filename = filename.to_str().unwrap();
        assert!(!filename.contains("{{timestamp}}"));
        assert!(filename.ends_with(".md"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_hooks_in_export() {
        let content = r#"---
pipe_to: pbcopy
save_to: "./logs/out.md"
---
Test prompt."#;
        let role = Role::new("test-hooks-export", content);
        let exported = role.export();
        assert!(exported.contains("pipe_to"));
        assert!(exported.contains("pbcopy"));
        assert!(exported.contains("save_to"));
        assert!(exported.contains("./logs/out.md"));
    }

    // ---- Phase 6C: Unified resource binding ----

    #[test]
    fn test_mcp_servers_parsing() {
        let content = r#"---
mcp_servers:
  - sqlite-server
  - github-server
---
You have database access."#;
        let role = Role::new("test-mcp-bind", content);
        assert_eq!(role.role_mcp_servers(), &["sqlite-server", "github-server"]);
    }

    #[test]
    fn test_mcp_servers_empty_by_default() {
        let content = "---\nmodel: gpt-4\n---\nYou are helpful.";
        let role = Role::new("no-mcp", content);
        assert!(role.role_mcp_servers().is_empty());
    }

    #[test]
    fn test_mcp_servers_in_export() {
        let content = r#"---
mcp_servers:
  - my-server
---
Test."#;
        let role = Role::new("test-mcp-export", content);
        let exported = role.export();
        assert!(exported.contains("mcp_servers"));
        assert!(exported.contains("my-server"));
    }

    #[test]
    fn test_all_phase6_fields_coexist() {
        let content = r#"---
model: gpt-4
pipe_to: "pbcopy"
save_to: "./out.md"
mcp_servers:
  - my-db
variables:
  - name: ctx
    default:
      shell: "echo hello"
---
Context: {{ctx}}."#;
        let role = Role::new("test-all-p6", content);
        assert_eq!(role.pipe_to(), Some("pbcopy"));
        assert_eq!(role.save_to(), Some("./out.md"));
        assert_eq!(role.role_mcp_servers(), &["my-db"]);
        assert_eq!(role.variables().len(), 1);
        assert!(matches!(
            &role.variables()[0].default,
            Some(VariableDefault::Shell { shell }) if shell == "echo hello"
        ));
    }

    #[test]
    fn test_set_output_schema() {
        let mut role = Role::new("test", "prompt");
        assert!(role.output_schema().is_none());
        let schema =
            serde_json::json!({"type": "object", "properties": {"name": {"type": "string"}}});
        role.set_output_schema(Some(schema.clone()));
        assert_eq!(role.output_schema(), Some(&schema));
        role.set_output_schema(None);
        assert!(role.output_schema().is_none());
    }

    #[test]
    fn test_set_input_schema() {
        let mut role = Role::new("test", "prompt");
        assert!(role.input_schema().is_none());
        let schema =
            serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}});
        role.set_input_schema(Some(schema.clone()));
        assert_eq!(role.input_schema(), Some(&schema));
        role.set_input_schema(None);
        assert!(role.input_schema().is_none());
    }

    #[test]
    fn test_set_pipe_to() {
        let mut role = Role::new("test", "prompt");
        assert!(role.pipe_to().is_none());
        role.set_pipe_to(Some("pbcopy".to_string()));
        assert_eq!(role.pipe_to(), Some("pbcopy"));
        role.set_pipe_to(None);
        assert!(role.pipe_to().is_none());
    }

    #[test]
    fn test_set_save_to() {
        let mut role = Role::new("test", "prompt");
        assert!(role.save_to().is_none());
        role.set_save_to(Some("./output.md".to_string()));
        assert_eq!(role.save_to(), Some("./output.md"));
        role.set_save_to(None);
        assert!(role.save_to().is_none());
    }

    #[test]
    fn test_validate_schema_detailed_success() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
            "required": ["name"]
        });
        let result = validate_schema_detailed("output", &schema, r#"{"name": "Alice"}"#).unwrap();
        assert!(result.is_ok());
        assert!(result.violations.is_empty());
        assert_eq!(result.direction, "output");
        assert_eq!(result.raw_text, r#"{"name": "Alice"}"#);
    }

    #[test]
    fn test_validate_schema_detailed_not_json() {
        let schema = serde_json::json!({"type": "object"});
        let result = validate_schema_detailed("input", &schema, "not json at all").unwrap();
        assert!(!result.is_ok());
        assert_eq!(result.violations.len(), 1);
        assert_eq!(result.violations[0].message, "not valid JSON");
        assert_eq!(result.raw_text, "not json at all");
    }

    #[test]
    fn test_validate_schema_detailed_type_mismatch_has_paths() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "count": { "type": "integer" }
            },
            "required": ["count"]
        });
        let result = validate_schema_detailed("output", &schema, r#"{"count": "foo"}"#).unwrap();
        assert!(!result.is_ok());
        assert_eq!(result.violations.len(), 1);
        let v = &result.violations[0];
        assert!(
            !v.instance_path.is_empty(),
            "instance_path should be populated"
        );
        assert!(!v.schema_path.is_empty(), "schema_path should be populated");
        assert!(
            v.message.contains("integer"),
            "message should mention expected type"
        );
    }

    #[test]
    fn test_validate_schema_detailed_nested_array_paths() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "qty": { "type": "integer" }
                        },
                        "required": ["name", "qty"]
                    }
                }
            },
            "required": ["items"]
        });
        let input = r#"{"items": [{"name": "apple", "qty": "bad"}]}"#;
        let result = validate_schema_detailed("output", &schema, input).unwrap();
        assert!(!result.is_ok());
        assert_eq!(result.violations.len(), 1);
        let v = &result.violations[0];
        // instance_path should point into items/0/qty
        assert!(
            v.instance_path.contains("0"),
            "path should contain array index"
        );
    }

    #[test]
    fn test_validate_schema_detailed_multiple_violations() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "a": { "type": "string" },
                "b": { "type": "integer" }
            },
            "required": ["a", "b"]
        });
        let result =
            validate_schema_detailed("output", &schema, r#"{"a": 123, "b": "wrong"}"#).unwrap();
        assert!(!result.is_ok());
        assert!(
            result.violations.len() >= 2,
            "should have at least 2 violations"
        );
    }

    #[test]
    fn test_validate_schema_detailed_terse_error_format() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "x": { "type": "integer" } },
            "required": ["x"]
        });
        let result = validate_schema_detailed("output", &schema, r#"{"x": "bad"}"#).unwrap();
        let terse = result.terse_error();
        assert!(terse.starts_with("Schema output validation failed:"));
        assert!(terse.contains("  - "));
    }

    #[test]
    fn test_validate_schema_detailed_preserves_raw_text() {
        let schema = serde_json::json!({"type": "object"});
        let raw = "  {\"extra\": true}  ";
        let result = validate_schema_detailed("output", &schema, raw).unwrap();
        assert!(result.is_ok());
        assert_eq!(result.raw_text, raw.trim());
    }

    // ---- Phase 9C: Schema validation retry loop ----

    #[test]
    fn test_schema_retries_default_none() {
        // No frontmatter field -> None, consumer applies its own default
        let content = "---\nmodel: gpt-4\n---\nPrompt.";
        let role = Role::new("no-retries", content);
        assert_eq!(role.schema_retries(), None);
    }

    #[test]
    fn test_schema_retries_parsed_from_frontmatter() {
        let content = r#"---
schema_retries: 2
---
Prompt."#;
        let role = Role::new("with-retries", content);
        assert_eq!(role.schema_retries(), Some(2));
    }

    #[test]
    fn test_schema_retries_zero_means_fail_fast() {
        let content = r#"---
schema_retries: 0
---
Prompt."#;
        let role = Role::new("zero-retries", content);
        assert_eq!(role.schema_retries(), Some(0));
    }

    #[test]
    fn test_schema_retries_in_export() {
        let content = r#"---
schema_retries: 3
---
Prompt."#;
        let role = Role::new("export-retries", content);
        let exported = role.export();
        assert!(exported.contains("schema_retries"));
        assert!(exported.contains("3"));
    }

    // ---- Phase 10C: Pipeline stage retry ----

    #[test]
    fn test_stage_retries_default_none() {
        let content = "---\nmodel: gpt-4\n---\nPrompt.";
        let role = Role::new("no-stage-retries", content);
        assert_eq!(role.stage_retries(), None);
    }

    #[test]
    fn test_stage_retries_parsed_from_frontmatter() {
        let content = r#"---
stage_retries: 2
---
Prompt."#;
        let role = Role::new("with-stage-retries", content);
        assert_eq!(role.stage_retries(), Some(2));
    }

    #[test]
    fn test_stage_retries_zero_means_fail_fast() {
        let content = r#"---
stage_retries: 0
---
Prompt."#;
        let role = Role::new("zero-stage-retries", content);
        assert_eq!(role.stage_retries(), Some(0));
    }

    #[test]
    fn test_stage_retries_in_export() {
        let content = r#"---
stage_retries: 3
---
Prompt."#;
        let role = Role::new("export-stage-retries", content);
        let exported = role.export();
        assert!(exported.contains("stage_retries"));
        assert!(exported.contains("3"));
    }

    #[test]
    fn test_stage_retries_coexists_with_schema_retries() {
        let content = r#"---
schema_retries: 2
stage_retries: 1
---
Prompt."#;
        let role = Role::new("both-retries", content);
        assert_eq!(role.schema_retries(), Some(2));
        assert_eq!(role.stage_retries(), Some(1));
    }

    // ---- Phase 10D: Pipeline model fallback ----

    #[test]
    fn test_fallback_models_default_empty() {
        let content = "---\nmodel: gpt-4\n---\nPrompt.";
        let role = Role::new("no-fallback", content);
        assert!(role.fallback_models().is_empty());
    }

    #[test]
    fn test_fallback_models_parsed_from_frontmatter() {
        let content = r#"---
model: deepseek:deepseek-chat
fallback_models:
  - openai:gpt-4o-mini
  - openai:gpt-4o
---
Prompt."#;
        let role = Role::new("fallback-chain", content);
        assert_eq!(
            role.fallback_models(),
            &["openai:gpt-4o-mini", "openai:gpt-4o"]
        );
    }

    #[test]
    fn test_fallback_models_in_export() {
        let content = r#"---
model: a
fallback_models:
  - b
  - c
---
Prompt."#;
        let role = Role::new("export-fallbacks", content);
        let exported = role.export();
        assert!(exported.contains("fallback_models"));
        assert!(exported.contains("b"));
        assert!(exported.contains("c"));
    }

    #[test]
    fn test_fallback_models_empty_list_is_not_exported() {
        let content = "---\nmodel: a\n---\nPrompt.";
        let role = Role::new("no-export-when-empty", content);
        let exported = role.export();
        assert!(
            !exported.contains("fallback_models"),
            "empty fallback list must not round-trip as an empty key"
        );
    }

    // ---- Phase 11D: Pipeline budget propagation ----

    #[test]
    fn test_pipeline_budget_usd_default_none() {
        let content = "---\nmodel: a\n---\nPrompt.";
        let role = Role::new("no-budget", content);
        assert_eq!(role.pipeline_budget_usd(), None);
    }

    #[test]
    fn test_pipeline_budget_usd_parsed_from_frontmatter() {
        let content = r#"---
pipeline_budget_usd: 0.05
pipeline:
  - role: extract
  - role: review
---
Prompt."#;
        let role = Role::new("budgeted-pipeline", content);
        assert_eq!(role.pipeline_budget_usd(), Some(0.05));
    }

    #[test]
    fn test_pipeline_budget_usd_in_export() {
        let content = r#"---
pipeline_budget_usd: 0.10
---
Prompt."#;
        let role = Role::new("export-budget", content);
        let exported = role.export();
        assert!(
            exported.contains("pipeline_budget_usd"),
            "budget must round-trip: {exported}"
        );
        assert!(exported.contains("0.1"));
    }

    #[test]
    fn test_pipeline_budget_allocates_proportionally_via_allocator() {
        // End-to-end: role frontmatter → pipeline_sequential() → allocator.
        // Verifies the wiring `invoke_role`/`run()` rely on lines up: a 4 USD
        // budget split across stages with weights [default=1, 2, default=1]
        // gives [1.0, 2.0, 1.0] (totals 4.0).
        let content = r#"---
pipeline_budget_usd: 4.0
pipeline:
  - role: extract
  - role: review
    budget_weight: 2.0
  - role: format
---
Prompt."#;
        let role = Role::new("alloc-end-to-end", content);
        let total = role.pipeline_budget_usd().expect("budget set");
        let weights: Vec<Option<f64>> = role
            .pipeline_sequential()
            .unwrap()
            .iter()
            .map(|s| s.budget_weight)
            .collect();
        let shares = crate::context_budget::allocate_stage_budgets(&weights, total);
        assert_eq!(shares.len(), 3);
        assert!((shares[0] - 1.0).abs() < 1e-9);
        assert!((shares[1] - 2.0).abs() < 1e-9);
        assert!((shares[2] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_pipeline_stage_budget_weight_parsed() {
        let content = r#"---
pipeline_budget_usd: 1.0
pipeline:
  - role: extract
  - role: review
    budget_weight: 2.0
  - role: format
---
Prompt."#;
        let role = Role::new("weighted-stages", content);
        let stages = role.pipeline_sequential().expect("sequential pipeline");
        assert_eq!(stages.len(), 3);
        assert_eq!(stages[0].budget_weight, None);
        assert_eq!(stages[1].budget_weight, Some(2.0));
        assert_eq!(stages[2].budget_weight, None);
    }

    // ---- Phase 26C: Knowledge-base bindings ----

    #[test]
    fn test_knowledge_bindings_default_empty() {
        let content = "---\nmodel: a\n---\nPrompt.";
        let role = Role::new("no-kb", content);
        assert!(role.knowledge_bindings().is_empty());
        assert!(role.knowledge_mode().is_none());
    }

    #[test]
    fn test_knowledge_binding_from_bare_string() {
        let content = "---\nknowledge: my-docs\n---\nPrompt.";
        let role = Role::new("single", content);
        assert_eq!(role.knowledge_bindings().len(), 1);
        assert_eq!(role.knowledge_bindings()[0].name, "my-docs");
        assert!(role.knowledge_bindings()[0].tags.is_empty());
        assert!((role.knowledge_bindings()[0].weight - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_knowledge_bindings_from_string_list() {
        let content = r#"---
knowledge:
  - kb-a
  - kb-b
---
Prompt."#;
        let role = Role::new("multi-strings", content);
        let ids: Vec<_> = role
            .knowledge_bindings()
            .iter()
            .map(|b| b.name.as_str())
            .collect();
        assert_eq!(ids, vec!["kb-a", "kb-b"]);
    }

    #[test]
    fn test_knowledge_bindings_from_object_list() {
        let content = r#"---
knowledge:
  - name: kb-a
    tags: [kind:rule, topic:retrieval]
    weight: 1.5
  - name: kb-b
---
Prompt."#;
        let role = Role::new("full-form", content);
        let b = role.knowledge_bindings();
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].name, "kb-a");
        assert_eq!(b[0].tags, vec!["kind:rule", "topic:retrieval"]);
        assert!((b[0].weight - 1.5).abs() < f32::EPSILON);
        assert_eq!(b[1].name, "kb-b");
        assert!(b[1].tags.is_empty());
        assert!((b[1].weight - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_knowledge_mode_parsed() {
        let content = "---\nknowledge: my-kb\nknowledge_mode: tool\n---\nPrompt.";
        let role = Role::new("tool-mode", content);
        assert_eq!(role.knowledge_mode(), Some("tool"));
    }

    #[test]
    fn test_knowledge_exports_compact_when_simple() {
        // Single simple binding exports as bare string.
        let content = "---\nknowledge: my-kb\n---\nPrompt.";
        let role = Role::new("simple-single", content);
        let exported = role.export();
        assert!(exported.contains("knowledge: my-kb"));
        assert!(!exported.contains("- name:"));
    }

    #[test]
    fn test_knowledge_exports_list_when_multiple_simple() {
        let content = r#"---
knowledge:
  - a
  - b
---
Prompt."#;
        let role = Role::new("simple-list", content);
        let exported = role.export();
        assert!(exported.contains("knowledge:"));
        // Simple strings survive as a flat list.
        assert!(!exported.contains("name: a"));
    }

    #[test]
    fn test_knowledge_exports_object_form_when_tags_or_weight_set() {
        let content = r#"---
knowledge:
  - name: kb-a
    tags: [kind:rule]
---
Prompt."#;
        let role = Role::new("object-form", content);
        let exported = role.export();
        assert!(exported.contains("name: kb-a"));
        assert!(exported.contains("tags:"));
        assert!(exported.contains("kind:rule"));
    }

    #[test]
    fn test_knowledge_empty_list_not_exported() {
        let content = "---\nmodel: a\n---\nPrompt.";
        let role = Role::new("no-kb", content);
        let exported = role.export();
        assert!(!exported.contains("knowledge:"));
    }

    #[test]
    fn test_build_messages_appends_retry_feedback() {
        // Role::build_messages is the point that actually injects the retry
        // turn pair — this covers the wiring from Input -> messages.
        // Use an Input shape that goes through from_str, then attach feedback.
        let content = "---\nmodel: gpt-4\n---\nYou are helpful.";
        let role = Role::new("retry-test", content);

        // Build a synthetic input by hand — we can't call from_str without a
        // full config, but we can construct just the pieces build_messages
        // reads: role() is self, message_content() is a text part, and
        // retry_feedback() drives the injection.
        // Easier: call Role::build_messages directly with a minimal Input.
        // We construct the Input via from_str in a Config-using test elsewhere.
        // Here: directly assert that when retry_feedback is set, two extra
        // messages are appended (Assistant failed_output + User retry_prompt).

        // Build messages without retry (baseline count).
        let config = crate::config::Config::default();
        let global: crate::config::GlobalConfig =
            std::sync::Arc::new(parking_lot::RwLock::new(config));
        let input = crate::config::Input::from_str(&global, "hello", Some(role.clone()));
        let baseline = role.build_messages(&input);
        let baseline_count = baseline.len();

        let input = input.with_retry_prompt(
            "{\"broken\": true",
            "Your previous output failed schema validation. Please retry.",
        );
        let messages = role.build_messages(&input);
        assert_eq!(messages.len(), baseline_count + 2);
        // Last two messages must be Assistant(failed) + User(retry_prompt)
        let a = &messages[messages.len() - 2];
        let u = &messages[messages.len() - 1];
        assert!(matches!(a.role, crate::client::MessageRole::Assistant));
        assert!(matches!(u.role, crate::client::MessageRole::User));
        if let crate::client::MessageContent::Text(t) = &a.content {
            assert!(t.contains("broken"));
        } else {
            panic!("failed assistant message must be text");
        }
        if let crate::client::MessageContent::Text(t) = &u.content {
            assert!(t.contains("failed schema validation"));
        } else {
            panic!("retry user message must be text");
        }
    }

    #[test]
    fn test_schema_retries_roundtrip() {
        let content = r#"---
schema_retries: 2
output_schema:
  type: object
---
Prompt."#;
        let role = Role::new("roundtrip", content);
        assert_eq!(role.schema_retries(), Some(2));
        let exported = role.export();
        let round = Role::new("roundtrip", &exported);
        assert_eq!(round.schema_retries(), Some(2));
    }

    // ----- Phase 21: DAG primitives -----

    fn yaml_to_node(yaml: &str) -> Result<PipelineNode> {
        let v: serde_json::Value = serde_yaml::from_str(yaml)?;
        parse_pipeline_node(&v)
    }

    #[test]
    fn pipeline_node_sequential_stage_parses() {
        let node = yaml_to_node("role: summarize\nmodel: claude-haiku").unwrap();
        match node {
            PipelineNode::Stage(s) => {
                assert_eq!(s.role, "summarize");
                assert_eq!(s.model.as_deref(), Some("claude-haiku"));
            }
            _ => panic!("expected Stage"),
        }
    }

    #[test]
    fn pipeline_node_parallel_parses_default_merge() {
        let yaml = r#"
parallel:
  - role: security-review
  - role: style-review
"#;
        let node = yaml_to_node(yaml).unwrap();
        match node {
            PipelineNode::Parallel(p) => {
                assert_eq!(p.branches.len(), 2);
                assert!(matches!(p.merge, MergeStrategy::Concatenate));
            }
            _ => panic!("expected Parallel"),
        }
    }

    #[test]
    fn pipeline_node_parallel_json_array_merge() {
        let yaml = r#"
parallel:
  - role: a
  - role: b
merge: json_array
"#;
        let node = yaml_to_node(yaml).unwrap();
        match node {
            PipelineNode::Parallel(p) => {
                assert!(matches!(p.merge, MergeStrategy::JsonArray));
            }
            _ => panic!("expected Parallel"),
        }
    }

    #[test]
    fn pipeline_node_parallel_custom_role_merge() {
        let yaml = r#"
parallel:
  - role: a
  - role: b
merge:
  custom_role: synthesize
"#;
        let node = yaml_to_node(yaml).unwrap();
        match node {
            PipelineNode::Parallel(p) => match p.merge {
                MergeStrategy::CustomRole(r) => assert_eq!(r, "synthesize"),
                _ => panic!("expected CustomRole"),
            },
            _ => panic!("expected Parallel"),
        }
    }

    #[test]
    fn pipeline_node_parallel_rejects_empty_branches() {
        let yaml = r#"
parallel: []
"#;
        let err = yaml_to_node(yaml).unwrap_err();
        assert!(err.to_string().contains("at least one branch"));
    }

    #[test]
    fn pipeline_node_parallel_rejects_unknown_merge() {
        let yaml = r#"
parallel:
  - role: a
merge: weird
"#;
        let err = yaml_to_node(yaml).unwrap_err();
        assert!(err.to_string().contains("Unknown merge"));
    }

    #[test]
    fn pipeline_node_switch_parses_when_and_otherwise() {
        let yaml = r#"
switch:
  - when: { output_field: "category", equals: "bug" }
    role: bug-triage
  - when: { output_field: "category", equals: "feature" }
    role: feature-review
  - otherwise: true
    role: general-review
"#;
        let node = yaml_to_node(yaml).unwrap();
        match node {
            PipelineNode::Switch(s) => {
                assert_eq!(s.branches.len(), 3);
                assert!(s.branches[0].predicate.is_some());
                assert!(s.branches[2].predicate.is_none());
                match s.branches[2].node.as_ref() {
                    PipelineNode::Stage(stg) => {
                        assert_eq!(stg.role, "general-review")
                    }
                    _ => panic!("expected Stage in otherwise"),
                }
            }
            _ => panic!("expected Switch"),
        }
    }

    #[test]
    fn pipeline_node_switch_rejects_double_otherwise() {
        let yaml = r#"
switch:
  - otherwise: true
    role: a
  - otherwise: true
    role: b
"#;
        let err = yaml_to_node(yaml).unwrap_err();
        assert!(err.to_string().contains("more than one `otherwise:`"));
    }

    #[test]
    fn pipeline_node_switch_rejects_branch_with_no_predicate_or_otherwise() {
        let yaml = r#"
switch:
  - role: lonely
"#;
        let err = yaml_to_node(yaml).unwrap_err();
        assert!(err.to_string().contains("requires either `when:` or `otherwise:`"));
    }

    #[test]
    fn pipeline_node_switch_rejects_when_with_no_body() {
        let yaml = r#"
switch:
  - when: { contains: "foo" }
"#;
        let err = yaml_to_node(yaml).unwrap_err();
        // anyhow chain: outer context names the branch, inner gives the rule.
        let full = format!("{err:#}");
        assert!(full.contains("has no executable body"));
        assert!(full.contains("must specify `role:`, `parallel:`, or `switch:`"));
    }

    #[test]
    fn pipeline_node_nested_parallel_inside_switch() {
        let yaml = r#"
switch:
  - when: { output_field: "kind", equals: "deep" }
    parallel:
      - role: a
      - role: b
    merge: json_array
  - otherwise: true
    role: quick
"#;
        let node = yaml_to_node(yaml).unwrap();
        match node {
            PipelineNode::Switch(s) => match s.branches[0].node.as_ref() {
                PipelineNode::Parallel(p) => {
                    assert_eq!(p.branches.len(), 2);
                    assert!(matches!(p.merge, MergeStrategy::JsonArray));
                }
                _ => panic!("expected Parallel inside Switch"),
            },
            _ => panic!("expected Switch"),
        }
    }

    #[test]
    fn predicate_equals_matches_text_output() {
        let p = Predicate {
            contains: Some("error".into()),
            ..Default::default()
        };
        assert!(p.evaluate("there was an error in the build"));
        assert!(!p.evaluate("all clean"));
    }

    #[test]
    fn predicate_equals_on_json_field() {
        let p = Predicate {
            output_field: Some("category".into()),
            equals: Some(serde_json::Value::String("bug".into())),
            ..Default::default()
        };
        assert!(p.evaluate(r#"{"category": "bug", "id": 7}"#));
        assert!(!p.evaluate(r#"{"category": "feature"}"#));
        assert!(!p.evaluate("not json at all"));
    }

    #[test]
    fn predicate_dotted_field() {
        let p = Predicate {
            output_field: Some("meta.kind".into()),
            equals: Some(serde_json::Value::String("urgent".into())),
            ..Default::default()
        };
        assert!(p.evaluate(r#"{"meta": {"kind": "urgent"}}"#));
        assert!(!p.evaluate(r#"{"meta": {"kind": "low"}}"#));
        assert!(!p.evaluate(r#"{"meta": "not-an-object"}"#));
    }

    #[test]
    fn predicate_gt_lt_numeric() {
        let p_gt = Predicate {
            output_field: Some("score".into()),
            gt: Some(0.5),
            ..Default::default()
        };
        assert!(p_gt.evaluate(r#"{"score": 0.9}"#));
        assert!(!p_gt.evaluate(r#"{"score": 0.1}"#));
        let p_lt = Predicate {
            output_field: Some("score".into()),
            lt: Some(0.5),
            ..Default::default()
        };
        assert!(p_lt.evaluate(r#"{"score": 0.1}"#));
        assert!(!p_lt.evaluate(r#"{"score": 0.9}"#));
    }

    #[test]
    fn predicate_loose_string_number_equality() {
        // YAML often unquotes numbers — `equals: 1` and `equals: "1"`
        // should both match a string "1" or number 1.
        let p = Predicate {
            output_field: Some("v".into()),
            equals: Some(serde_json::Value::Number(serde_json::Number::from(1))),
            ..Default::default()
        };
        assert!(p.evaluate(r#"{"v": 1}"#));
        assert!(p.evaluate(r#"{"v": "1"}"#));
        assert!(!p.evaluate(r#"{"v": 2}"#));
    }

    #[test]
    fn role_frontmatter_parses_full_dag() {
        let content = r#"---
pipeline:
  - role: extract
  - parallel:
      - role: security-review
      - role: style-review
    merge: concatenate
  - role: synthesize
---
Body."#;
        let role = Role::new("dag-role", content);
        assert!(role.is_pipeline());
        assert!(role.pipeline_has_dag());
        let nodes = role.pipeline().unwrap();
        assert_eq!(nodes.len(), 3);
        assert!(matches!(nodes[1], PipelineNode::Parallel(_)));
        // pipeline_all_stages walks into branches.
        let all = role.pipeline_all_stages();
        let names: Vec<String> = all.iter().map(|s| s.role.clone()).collect();
        assert_eq!(
            names,
            vec!["extract", "security-review", "style-review", "synthesize"]
        );
        // Sequential view bails on DAG.
        assert!(role.pipeline_sequential().is_none());
    }

    #[test]
    fn role_frontmatter_parses_pure_sequential_via_new_path() {
        // Sequential pipelines still appear in pipeline_sequential.
        let content = r#"---
pipeline:
  - role: a
  - role: b
    model: claude-haiku
---
Body."#;
        let role = Role::new("seq", content);
        assert!(!role.pipeline_has_dag());
        let seq = role.pipeline_sequential().unwrap();
        assert_eq!(seq.len(), 2);
        assert_eq!(seq[1].model.as_deref(), Some("claude-haiku"));
    }

    #[test]
    fn structural_check_rejects_empty_stage_role() {
        let node = PipelineNode::Stage(RolePipelineStage {
            role: " ".into(),
            model: None,
            budget_weight: None,
        });
        let err = node.structural_check().unwrap_err();
        assert!(err.to_string().contains("empty role"));
    }

    #[test]
    fn pipeline_merge_roles_collected_recursively() {
        let yaml = r#"
parallel:
  - role: a
  - parallel:
      - role: b
      - role: c
    merge:
      custom_role: inner-merge
merge:
  custom_role: outer-merge
"#;
        let node = yaml_to_node(yaml).unwrap();
        let mergers = node.merge_role_names();
        assert!(mergers.contains(&"inner-merge".to_string()));
        assert!(mergers.contains(&"outer-merge".to_string()));
        assert_eq!(mergers.len(), 2);
    }

    // ---- Phase 16F/16G: RolePublicView ----

    #[test]
    fn public_view_redacts_prompt_body() {
        let content = r#"---
description: A summarizer
---
INTERNAL: secret system prompt body that must not leak."#;
        let role = Role::new("summarize", content);
        let view = RolePublicView::from(&role);
        let json = serde_json::to_string(&view).unwrap();
        assert!(
            !json.contains("INTERNAL"),
            "prompt body must not leak via public view: {json}"
        );
        assert!(
            !json.contains("secret"),
            "prompt body must not leak via public view: {json}"
        );
    }

    #[test]
    fn public_view_exposes_metadata_and_schemas() {
        let content = r#"---
description: Classifies text
tags: [text, classification]
capabilities: [text-classification, label]
model: openai:gpt-4o
input_schema:
  type: string
output_schema:
  type: object
  properties:
    label: { type: string }
---
Secret prompt."#;
        let role = Role::new("classify", content);
        let view = RolePublicView::from(&role);
        assert_eq!(view.name, "classify");
        assert_eq!(view.description.as_deref(), Some("Classifies text"));
        assert_eq!(
            view.tags.as_deref(),
            Some(&["text".to_string(), "classification".to_string()][..])
        );
        assert_eq!(
            view.capabilities,
            vec!["text-classification".to_string(), "label".to_string()]
        );
        assert_eq!(view.model.as_deref(), Some("openai:gpt-4o"));
        assert!(view.input_schema.is_some());
        assert!(view.output_schema.is_some());
    }

    #[test]
    fn public_view_reports_pipeline_shape_without_stage_names() {
        let content = r#"---
description: Multi-stage role
pipeline:
  - role: stage-one
  - role: stage-two
  - role: stage-three
---
"#;
        let role = Role::new("pipe", content);
        let view = RolePublicView::from(&role);
        assert!(view.has_pipeline);
        assert_eq!(view.pipeline_length, 3);
        // Stage role names belong to the server's namespace; the public view
        // must not echo them.
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("stage-one"), "stage names leaked: {json}");
        assert!(!json.contains("stage-two"), "stage names leaked: {json}");
        assert!(!json.contains("stage-three"), "stage names leaked: {json}");
    }

    #[test]
    fn public_view_port_signatures_default_to_text() {
        // No schema means free-form text I/O — Phase 14B convention.
        let role = Role::new("plain", "---\n---\n");
        let view = RolePublicView::from(&role);
        assert_eq!(view.port_input, "any");
        assert_eq!(view.port_output, "text");
    }

    #[test]
    fn public_view_skips_empty_optional_fields_in_json() {
        let role = Role::new("bare", "---\n---\n");
        let view = RolePublicView::from(&role);
        let json: serde_json::Value = serde_json::to_value(&view).unwrap();
        let obj = json.as_object().unwrap();
        // Required fields always present
        assert!(obj.contains_key("name"));
        assert!(obj.contains_key("has_pipeline"));
        assert!(obj.contains_key("pipeline_length"));
        assert!(obj.contains_key("port_input"));
        assert!(obj.contains_key("port_output"));
        // Optional fields elided when empty
        assert!(!obj.contains_key("description"));
        assert!(!obj.contains_key("tags"));
        assert!(!obj.contains_key("capabilities"));
        assert!(!obj.contains_key("input_schema"));
        assert!(!obj.contains_key("output_schema"));
    }
}
