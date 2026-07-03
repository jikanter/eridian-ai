use super::*;

use crate::{
    client::Model,
    function::{run_llm_function, Functions},
};

use anyhow::{Context, Result};
use inquire::{validator::Validation, Text};
use std::{fs::read_to_string, path::Path};

use serde::{Deserialize, Serialize};

const DEFAULT_AGENT_NAME: &str = "rag";

pub type AgentVariables = IndexMap<String, String>;

#[derive(Debug, Clone)]
pub struct Agent {
    name: String,
    config: AgentConfig,
    definition: AgentDefinition,
    shared_variables: AgentVariables,
    session_variables: Option<AgentVariables>,
    shared_dynamic_instructions: Option<String>,
    session_dynamic_instructions: Option<String>,
    functions: Functions,
    rag: Option<Arc<Rag>>,
    model: Model,
}

impl Agent {
    pub async fn init(
        config: &GlobalConfig,
        name: &str,
        abort_signal: AbortSignal,
    ) -> Result<Self> {
        let functions_dir = Config::agent_functions_dir(name);
        let definition_file_path = functions_dir.join("index.yaml");
        if !definition_file_path.exists() {
            bail!(crate::utils::did_you_mean(
                &format!("Unknown agent `{name}`"),
                name,
                &list_agents()
            ));
        }
        let functions_file_path = functions_dir.join("functions.json");
        let rag_path = Config::agent_rag_file(name, DEFAULT_AGENT_NAME);
        let config_path = Config::agent_config_file(name);
        let mut agent_config = if config_path.exists() {
            AgentConfig::load(&config_path)?
        } else {
            AgentConfig::new(&config.read())
        };
        let mut definition = AgentDefinition::load(&definition_file_path)?;
        let functions = if functions_file_path.exists() {
            Functions::init(&functions_file_path)?
        } else {
            Functions::default()
        };
        definition.replace_tools_placeholder(&functions);

        agent_config.load_envs(&definition.name);

        // Phase 19D: bind agent-declared MCP servers into use_tools
        // using the same expansion the role path uses (Phase 6C).
        if !agent_config.mcp_servers.is_empty() {
            let cfg = config.read();
            let available: std::collections::HashSet<&str> =
                cfg.mcp_servers.keys().map(|s| s.as_str()).collect();
            let new_use_tools = super::resolver::expand_mcp_servers_into_use_tools(
                "Agent",
                &definition.name,
                &agent_config.mcp_servers,
                agent_config.use_tools.as_deref(),
                &available,
            );
            drop(cfg);
            agent_config.use_tools = new_use_tools;
        }

        let model = {
            let config = config.read();
            match agent_config.model_id.as_ref() {
                Some(model_id) => Model::retrieve_model(&config, model_id, ModelType::Chat)?,
                None => {
                    if agent_config.temperature.is_none() {
                        agent_config.temperature = config.temperature;
                    }
                    if agent_config.top_p.is_none() {
                        agent_config.top_p = config.top_p;
                    }
                    config.current_model().clone()
                }
            }
        };

        let rag = if rag_path.exists() {
            Some(Arc::new(Rag::load(config, DEFAULT_AGENT_NAME, &rag_path)?))
        } else if !definition.documents.is_empty() && !config.read().info_flag {
            let mut ans = false;
            if *IS_STDOUT_TERMINAL {
                ans = Confirm::new("The agent has the documents, init RAG?")
                    .with_default(true)
                    .prompt()?;
            }
            if ans {
                let mut document_paths = vec![];
                for path in &definition.documents {
                    if is_url(path) {
                        document_paths.push(path.to_string());
                    } else {
                        let new_path = safe_join_path(&functions_dir, path)
                            .ok_or_else(|| anyhow!("Invalid document path: '{path}'"))?;
                        document_paths.push(new_path.display().to_string())
                    }
                }
                let rag =
                    Rag::init(config, "rag", &rag_path, &document_paths, abort_signal).await?;
                Some(Arc::new(rag))
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            name: name.to_string(),
            config: agent_config,
            definition,
            shared_variables: Default::default(),
            session_variables: None,
            shared_dynamic_instructions: None,
            session_dynamic_instructions: None,
            functions,
            rag,
            model,
        })
    }

    pub fn init_agent_variables(
        agent_variables: &[AgentVariable],
        variables: &AgentVariables,
        no_interaction: bool,
    ) -> Result<AgentVariables> {
        let mut output = IndexMap::new();
        if agent_variables.is_empty() {
            return Ok(output);
        }
        let mut printed = false;
        let mut unset_variables = vec![];
        for agent_variable in agent_variables {
            let key = agent_variable.name.clone();
            match variables.get(&key) {
                Some(value) => {
                    output.insert(key, value.clone());
                }
                None => {
                    if let Some(value) = agent_variable.default.clone() {
                        output.insert(key, value);
                        continue;
                    }
                    if no_interaction {
                        continue;
                    }
                    if *IS_STDOUT_TERMINAL {
                        if !printed {
                            println!("⚙ Init agent variables...");
                            printed = true;
                        }
                        let value = Text::new(&format!(
                            "{} ({}):",
                            agent_variable.name, agent_variable.description
                        ))
                        .with_validator(|input: &str| {
                            if input.trim().is_empty() {
                                Ok(Validation::Invalid("This field is required".into()))
                            } else {
                                Ok(Validation::Valid)
                            }
                        })
                        .prompt()?;
                        output.insert(key, value);
                    } else {
                        unset_variables.push(agent_variable)
                    }
                }
            }
        }
        if !unset_variables.is_empty() {
            bail!(
                "The following agent variables are required:\n{}",
                unset_variables
                    .iter()
                    .map(|v| format!("  - {}: {}", v.name, v.description))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
        Ok(output)
    }

    pub fn export(&self) -> Result<String> {
        let mut value = json!({});
        value["name"] = json!(self.name());
        let variables = self.variables();
        if !variables.is_empty() {
            value["variables"] = serde_json::to_value(variables)?;
        }
        value["config"] = json!(self.config);
        let mut definition = self.definition.clone();
        definition.instructions = self.interpolated_instructions();
        value["definition"] = json!(definition);
        value["functions_dir"] = Config::agent_functions_dir(&self.name)
            .display()
            .to_string()
            .into();
        value["data_dir"] = Config::agent_data_dir(&self.name)
            .display()
            .to_string()
            .into();
        value["config_file"] = Config::agent_config_file(&self.name)
            .display()
            .to_string()
            .into();
        let data = serde_norway::to_string(&value)?;
        Ok(data)
    }

    pub fn banner(&self) -> String {
        self.definition.banner()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn functions(&self) -> &Functions {
        &self.functions
    }

    pub fn rag(&self) -> Option<Arc<Rag>> {
        self.rag.clone()
    }

    pub fn conversation_staters(&self) -> &[String] {
        &self.definition.conversation_starters
    }

    pub fn interpolated_instructions(&self) -> String {
        let mut output = self
            .session_dynamic_instructions
            .clone()
            .or_else(|| self.shared_dynamic_instructions.clone())
            .or_else(|| self.config.instructions.clone())
            .unwrap_or_else(|| self.definition.instructions.clone());
        for (k, v) in self.variables() {
            output = output.replace(&format!("{{{{{k}}}}}"), v)
        }
        interpolate_variables_with_model(&mut output, Some(&self.model));
        output
    }

    pub fn agent_prelude(&self) -> Option<&str> {
        self.config.agent_prelude.as_deref()
    }

    pub fn variables(&self) -> &AgentVariables {
        match &self.session_variables {
            Some(variables) => variables,
            None => &self.shared_variables,
        }
    }

    pub fn variable_envs(&self) -> HashMap<String, String> {
        self.variables()
            .iter()
            .map(|(k, v)| {
                (
                    format!("LLM_AGENT_VAR_{}", normalize_env_name(k)),
                    v.clone(),
                )
            })
            .collect()
    }

    pub fn config_variables(&self) -> &AgentVariables {
        &self.config.variables
    }

    pub fn shared_variables(&self) -> &AgentVariables {
        &self.shared_variables
    }

    pub fn set_shared_variables(&mut self, shared_variables: AgentVariables) {
        self.shared_variables = shared_variables;
    }

    pub fn set_session_variables(&mut self, session_variables: AgentVariables) {
        self.session_variables = Some(session_variables);
    }

    pub fn defined_variables(&self) -> &[AgentVariable] {
        &self.definition.variables
    }

    pub fn exit_session(&mut self) {
        self.session_variables = None;
        self.session_dynamic_instructions = None;
    }

    pub fn is_dynamic_instructions(&self) -> bool {
        self.definition.dynamic_instructions
    }

    pub fn update_shared_dynamic_instructions(&mut self, force: bool) -> Result<()> {
        if self.is_dynamic_instructions() && (force || self.shared_dynamic_instructions.is_none()) {
            self.shared_dynamic_instructions = Some(self.run_instructions_fn()?);
        }
        Ok(())
    }

    pub fn update_session_dynamic_instructions(&mut self, value: Option<String>) -> Result<()> {
        if self.is_dynamic_instructions() {
            let value = match value {
                Some(v) => v,
                None => self.run_instructions_fn()?,
            };
            self.session_dynamic_instructions = Some(value);
        }
        Ok(())
    }

    pub fn set_output_schema(&mut self, value: Option<serde_json::Value>) {
        self.config.output_schema = value;
    }

    pub fn set_input_schema(&mut self, value: Option<serde_json::Value>) {
        self.config.input_schema = value;
    }

    pub fn set_pipe_to(&mut self, value: Option<String>) {
        self.config.pipe_to = value;
    }

    pub fn set_save_to(&mut self, value: Option<String>) {
        self.config.save_to = value;
    }

    fn run_instructions_fn(&self) -> Result<String> {
        // run_llm_function is async; bridge from sync context via block_in_place
        let name = self.name().to_string();
        let envs = self.variable_envs();
        let value = tokio::task::block_in_place(move || {
            tokio::runtime::Handle::current().block_on(async {
                run_llm_function(name, vec!["_instructions".into(), "{}".into()], envs, 0, None)
                    .await
            })
        })?;
        match value {
            Some(v) => Ok(v),
            _ => bail!("No return value from '_instructions' function"),
        }
    }
}

impl Entity for Agent {
    fn to_role(&self) -> Role {
        let prompt = self.interpolated_instructions();
        let mut role = Role::new("", &prompt);
        role.sync(self);
        if self.config.output_schema.is_some() {
            role.set_output_schema(self.config.output_schema.clone());
        }
        if self.config.input_schema.is_some() {
            role.set_input_schema(self.config.input_schema.clone());
        }
        if self.config.pipe_to.is_some() {
            role.set_pipe_to(self.config.pipe_to.clone());
        }
        if self.config.save_to.is_some() {
            role.set_save_to(self.config.save_to.clone());
        }
        role
    }

    fn model(&self) -> &Model {
        &self.model
    }

    fn temperature(&self) -> Option<f64> {
        self.config.temperature
    }

    fn top_p(&self) -> Option<f64> {
        self.config.top_p
    }

    fn use_tools(&self) -> Option<String> {
        self.config.use_tools.clone()
    }

    fn set_model(&mut self, model: Model) {
        self.config.model_id = Some(model.id());
        self.model = model;
    }

    fn set_temperature(&mut self, value: Option<f64>) {
        self.config.temperature = value;
    }

    fn set_top_p(&mut self, value: Option<f64>) {
        self.config.top_p = value;
    }

    fn set_use_tools(&mut self, value: Option<String>) {
        self.config.use_tools = value;
    }

    fn facets(&self) -> FacetSet {
        use FacetOwnership::{Owned, Referenced};
        let mut facets = FacetSet::new();
        // An agent is directory-backed: it can *own* executable/stateful facets
        // its files provide (a tools toolset, a RAG corpus, dynamic
        // instructions) while still *referencing* external ones (MCP servers).
        if self.rag.is_some() || !self.definition.documents.is_empty() {
            facets.insert(Facet::Know, Owned);
        }
        if !self.functions.is_empty() {
            facets.insert(Facet::Act, Owned);
        }
        if self.config.use_tools.is_some() || !self.config.mcp_servers.is_empty() {
            facets.insert(Facet::Act, Referenced);
        }
        if self.config.input_schema.is_some()
            || self.config.output_schema.is_some()
            || self.config.pipe_to.is_some()
            || self.config.save_to.is_some()
            || !self.config.variables.is_empty()
            || !self.definition.variables.is_empty()
        {
            facets.insert(Facet::Shape, Owned);
        }
        if self.definition.dynamic_instructions
            || self.shared_dynamic_instructions.is_some()
            || self.session_dynamic_instructions.is_some()
        {
            facets.insert(Facet::Govern, Owned);
        }
        facets
    }

    fn backing(&self) -> Backing {
        // An agent is a directory: it may own executable/stateful facets.
        Backing::Directory
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentConfig {
    #[serde(rename(serialize = "model", deserialize = "model"))]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "tools")]
    pub use_tools: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_prelude: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub variables: AgentVariables,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipe_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save_to: Option<String>,
    /// Phase 19D: MCP servers this agent should auto-bind into `use_tools`
    /// at load time. Mirrors `mcp_servers:` on a Role (Phase 6C).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<String>,
}

impl AgentConfig {
    pub fn new(config: &Config) -> Self {
        Self {
            use_tools: config.use_tools.clone(),
            agent_prelude: config.agent_prelude.clone(),
            ..Default::default()
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let contents = read_to_string(path)
            .with_context(|| format!("Failed to read agent config file at '{}'", path.display()))?;
        let config: Self = serde_norway::from_str(&contents)
            .with_context(|| format!("Failed to load agent config at '{}'", path.display()))?;
        Ok(config)
    }

    fn load_envs(&mut self, name: &str) {
        let with_prefix = |v: &str| normalize_env_name(&format!("{name}_{v}"));

        if let Some(v) = read_env_value::<String>(&with_prefix("model")) {
            self.model_id = v;
        }
        if let Some(v) = read_env_value::<f64>(&with_prefix("temperature")) {
            self.temperature = v;
        }
        if let Some(v) = read_env_value::<f64>(&with_prefix("top_p")) {
            self.top_p = v;
        }
        if let Some(v) = read_env_value::<String>(&with_prefix("use_tools")) {
            self.use_tools = v;
        }
        if let Some(v) = read_env_value::<String>(&with_prefix("agent_prelude")) {
            self.agent_prelude = v;
        }
        if let Some(v) = read_env_value::<String>(&with_prefix("instructions")) {
            self.instructions = v;
        }
        if let Ok(v) = env::var(with_prefix("variables")) {
            if let Ok(v) = serde_json::from_str(&v) {
                self.variables = v;
            }
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentDefinition {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub dynamic_instructions: bool,
    #[serde(default)]
    pub variables: Vec<AgentVariable>,
    #[serde(default)]
    pub conversation_starters: Vec<String>,
    #[serde(default)]
    pub documents: Vec<String>,
}

impl AgentDefinition {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = read_to_string(path)
            .with_context(|| format!("Failed to read agent index file at '{}'", path.display()))?;
        let definition: Self = serde_norway::from_str(&contents)
            .with_context(|| format!("Failed to load agent index at '{}'", path.display()))?;
        Ok(definition)
    }

    fn banner(&self) -> String {
        let AgentDefinition {
            name,
            description,
            version,
            conversation_starters,
            ..
        } = self;
        let starters = if conversation_starters.is_empty() {
            String::new()
        } else {
            let starters = conversation_starters
                .iter()
                .map(|v| format!("- {v}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                r#"

## Conversation Starters
{starters}"#
            )
        };
        format!(
            r#"# {name} {version}
{description}{starters}"#
        )
    }

    fn replace_tools_placeholder(&mut self, functions: &Functions) {
        let tools_placeholder: &str = "{{__tools__}}";
        if self.instructions.contains(tools_placeholder) {
            let tools = functions
                .declarations()
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let description = match v.description.split_once('\n') {
                        Some((v, _)) => v,
                        None => &v.description,
                    };
                    format!("{}. {}: {description}", i + 1, v.name)
                })
                .collect::<Vec<String>>()
                .join("\n");
            self.instructions = self.instructions.replace(tools_placeholder, &tools);
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentVariable {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(skip_deserializing, default)]
    pub value: String,
}

pub fn list_agents() -> Vec<String> {
    let agents_file = Config::functions_dir().join("agents.txt");
    let contents = match read_to_string(agents_file) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    contents
        .split('\n')
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                None
            } else {
                Some(line.to_string())
            }
        })
        .collect()
}

pub fn complete_agent_variables(agent_name: &str) -> Vec<(String, Option<String>)> {
    let index_path = Config::agent_functions_dir(agent_name).join("index.yaml");
    if !index_path.exists() {
        return vec![];
    }
    let Ok(definition) = AgentDefinition::load(&index_path) else {
        return vec![];
    };
    definition
        .variables
        .iter()
        .map(|v| {
            let description = match &v.default {
                Some(default) => format!("{} [default: {default}]", v.description),
                None => v.description.clone(),
            };
            (format!("{}=", v.name), Some(description))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_config_default_has_empty_mcp_servers() {
        let cfg = AgentConfig::default();
        assert!(cfg.mcp_servers.is_empty());
    }

    #[test]
    fn agent_config_parses_mcp_servers_field() {
        let yaml = r#"
mcp_servers:
  - github
  - filesystem
"#;
        let cfg: AgentConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(cfg.mcp_servers, vec!["github".to_string(), "filesystem".to_string()]);
    }

    #[test]
    fn agent_config_omits_empty_mcp_servers_on_serialize() {
        let cfg = AgentConfig::default();
        let out = serde_norway::to_string(&cfg).unwrap();
        assert!(
            !out.contains("mcp_servers"),
            "empty mcp_servers should not be serialized: {out}"
        );
    }

    #[test]
    fn agent_config_round_trips_mcp_servers() {
        let cfg = AgentConfig {
            mcp_servers: vec!["github".into(), "fs".into()],
            ..Default::default()
        };
        let yaml = serde_norway::to_string(&cfg).unwrap();
        let parsed: AgentConfig = serde_norway::from_str(&yaml).unwrap();
        assert_eq!(parsed.mcp_servers, cfg.mcp_servers);
    }

    #[test]
    fn agent_config_coexists_with_existing_fields() {
        // Phase 19D field must not collide with the Phase 7.5C cluster
        // (input_schema, output_schema, pipe_to, save_to) or earlier fields.
        let yaml = r#"
model: claude:claude-sonnet-4-6
temperature: 0.2
use_tools: local_lookup
mcp_servers:
  - github
input_schema:
  type: object
output_schema:
  type: string
pipe_to: jq .
save_to: out.txt
"#;
        let cfg: AgentConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(cfg.model_id.as_deref(), Some("claude:claude-sonnet-4-6"));
        assert_eq!(cfg.temperature, Some(0.2));
        assert_eq!(cfg.use_tools.as_deref(), Some("local_lookup"));
        assert_eq!(cfg.mcp_servers, vec!["github".to_string()]);
        assert!(cfg.input_schema.is_some());
        assert!(cfg.output_schema.is_some());
        assert_eq!(cfg.pipe_to.as_deref(), Some("jq ."));
        assert_eq!(cfg.save_to.as_deref(), Some("out.txt"));
    }

    // Phase 52A: Entity::facets() — agent (directory) backing owns
    // executable/stateful facets and references external ones.

    fn bare_agent() -> Agent {
        Agent {
            name: "t".into(),
            config: AgentConfig::default(),
            definition: AgentDefinition::default(),
            shared_variables: AgentVariables::default(),
            session_variables: None,
            shared_dynamic_instructions: None,
            session_dynamic_instructions: None,
            functions: Functions::default(),
            rag: None,
            model: Model::default(),
        }
    }

    #[test]
    fn agent_facets_bare_has_none() {
        assert!(bare_agent().facets().is_empty());
    }

    #[test]
    fn agent_owns_act_via_functions() {
        let mut a = bare_agent();
        a.functions.add_declarations(vec![serde_json::from_value(
            serde_json::json!({"name": "f", "description": "d", "parameters": {}}),
        )
        .unwrap()]);
        assert!(a.facets().is_owned(Facet::Act));
        assert!(!a.facets().is_referenced(Facet::Act));
    }

    #[test]
    fn agent_references_act_via_mcp_servers() {
        let mut a = bare_agent();
        a.config.mcp_servers = vec!["github".into()];
        assert!(a.facets().is_referenced(Facet::Act));
        assert!(!a.facets().is_owned(Facet::Act));
    }

    #[test]
    fn agent_act_is_both_owned_and_referenced() {
        let mut a = bare_agent();
        a.functions.add_declarations(vec![serde_json::from_value(
            serde_json::json!({"name": "f", "description": "d", "parameters": {}}),
        )
        .unwrap()]);
        a.config.mcp_servers = vec!["github".into()];
        let f = a.facets();
        assert!(f.is_owned(Facet::Act));
        assert!(f.is_referenced(Facet::Act));
    }

    #[test]
    fn agent_owns_know_via_documents() {
        let mut a = bare_agent();
        a.definition.documents = vec!["handbook.md".into()];
        assert!(a.facets().is_owned(Facet::Know));
    }

    #[test]
    fn agent_owns_govern_via_dynamic_instructions() {
        let mut a = bare_agent();
        a.definition.dynamic_instructions = true;
        assert!(a.facets().is_owned(Facet::Govern));
    }
}
