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

#[derive(Embed)]
#[folder = "assets/roles/"]
struct RolesAsset;

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
pub struct RoleVariable {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
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

            if let Ok(value) = serde_yaml::from_str::<Value>(meta_str) {
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
                                        .filter_map(|v| serde_json::from_value::<RoleVariable>(v.clone()).ok())
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
    let content = RolesAsset::get(&format!("{name}.md"))
        .ok_or_else(|| anyhow!("Unknown role `{name}`"))?;
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
        let mut prompt_parts = include_prompts;
        if !parent.prompt.is_empty() {
            prompt_parts.push(parent.prompt);
        }
        if !parts.prompt.is_empty() {
            prompt_parts.push(parts.prompt);
        }
        parts.prompt = prompt_parts.join("\n\n");
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
            if let Ok(value) = serde_yaml::from_str::<Value>(metadata) {
                if let Some(value) = value.as_object() {
                    for (key, value) in value {
                        match key.as_str() {
                            "model" => role.model_id = value.as_str().map(|v| v.to_string()),
                            "temperature" => role.temperature = value.as_f64(),
                            "top_p" => role.top_p = value.as_f64(),
                            "use_tools" => role.use_tools = value.as_str().map(|v| v.to_string()),
                            "input_schema" => role.input_schema = Some(value.clone()),
                            "output_schema" => role.output_schema = Some(value.clone()),
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
                                        .filter_map(|v| {
                                            serde_json::from_value::<RoleVariable>(v.clone()).ok()
                                        })
                                        .collect();
                                }
                            }
                            _ => (),
                        }
                    }
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
                    "\n\nYou MUST respond with valid JSON conforming to this JSON Schema:\n```json\n{schema_str}\n```\nDo not include any text outside the JSON object."
                );
                if system.is_empty() {
                    suffix.trim_start().to_string()
                } else {
                    format!("{system}{suffix}")
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

pub fn validate_schema(context: &str, schema: &Value, text: &str) -> Result<()> {
    let data: Value = serde_json::from_str(text.trim())
        .with_context(|| format!("Schema {context} validation failed: not valid JSON"))?;
    let validator = jsonschema::validator_for(schema)
        .map_err(|e| anyhow!("Invalid {context} schema: {e}"))?;
    let errors: Vec<String> = validator
        .iter_errors(&data)
        .map(|e| format!("  - {e}"))
        .collect();
    if !errors.is_empty() {
        bail!("Schema {context} validation failed:\n{}", errors.join("\n"));
    }
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
        assert_eq!(
            merged.get("model"),
            Some(&serde_json::json!("gpt-4"))
        );
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

        assert_eq!(result, "Safety first.\n\nBe helpful.\n\nFocus on code review.");
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
        assert!(err.contains("A -> B -> A"), "Error should show chain: {err}");
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
        assert_eq!(parts.variables[1].default, Some("formal".to_string()));
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
        assert_eq!(role.variables()[0].default, Some("english".to_string()));
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
}
