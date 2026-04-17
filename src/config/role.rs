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
    pipeline: Option<Vec<RolePipelineStage>>,

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
                                                .filter_map(|v| match serde_json::from_value::<RolePipelineStage>(v.clone()) {
                                                    Ok(stage) => Some(stage),
                                                    Err(e) => {
                                                        warn!("Skipping invalid pipeline stage in role '{}': {e}", name);
                                                        None
                                                    }
                                                })
                                                .collect(),
                                        );
                                    }
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

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn tags(&self) -> Option<&[String]> {
        self.tags.as_deref()
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

    pub fn pipeline(&self) -> Option<&[RolePipelineStage]> {
        self.pipeline.as_deref()
    }

    pub fn is_pipeline(&self) -> bool {
        self.pipeline.as_ref().is_some_and(|p| !p.is_empty())
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

    pub fn variables(&self) -> &[RoleVariable] {
        &self.variables
    }

    pub fn apply_variables(&mut self, vars: &IndexMap<String, String>) {
        for (k, v) in vars {
            self.prompt = self.prompt.replace(&format!("{{{{{k}}}}}"), v);
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
}
