use crate::{
    config::{Agent, Config, GlobalConfig},
    utils::*,
};

use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[cfg(windows)]
const PATH_SEP: &str = ";";
#[cfg(not(windows))]
const PATH_SEP: &str = ":";

pub async fn eval_tool_calls(config: &GlobalConfig, mut calls: Vec<ToolCall>) -> Result<Vec<ToolResult>> {
    let mut output = vec![];
    if calls.is_empty() {
        return Ok(output);
    }
    calls = ToolCall::dedup(calls);
    if calls.is_empty() {
        bail!("The request was aborted because an infinite loop of function calls was detected.")
    }
    let mut is_all_null = true;
    for call in calls {
        let is_mcp = call.name.contains(':') && {
            let cfg = config.read();
            cfg.mcp_pool.is_some()
                && cfg
                    .functions
                    .find(&call.name)
                    .map(|d| matches!(d.source, ToolSource::Mcp { .. }))
                    .unwrap_or(false)
        };
        let mut result = if is_mcp {
            crate::mcp_client::eval_mcp_tool(config, &call.name, call.arguments.clone()).await?
        } else {
            call.eval(config)?
        };
        if result.is_null() {
            result = json!("DONE");
        } else {
            is_all_null = false;
        }
        output.push(ToolResult::new(call, result));
    }
    if is_all_null {
        output = vec![];
    }
    Ok(output)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolResult {
    pub call: ToolCall,
    pub output: Value,
}

impl ToolResult {
    pub fn new(call: ToolCall, output: Value) -> Self {
        Self { call, output }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Functions {
    declarations: Vec<FunctionDeclaration>,
}

impl Functions {
    pub fn init(declarations_path: &Path) -> Result<Self> {
        let declarations: Vec<FunctionDeclaration> = if declarations_path.exists() {
            let ctx = || {
                format!(
                    "Failed to load functions at {}",
                    declarations_path.display()
                )
            };
            let content = fs::read_to_string(declarations_path).with_context(ctx)?;
            serde_json::from_str(&content).with_context(ctx)?
        } else {
            vec![]
        };

        Ok(Self { declarations })
    }

    pub fn find(&self, name: &str) -> Option<&FunctionDeclaration> {
        self.declarations.iter().find(|v| v.name == name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.declarations.iter().any(|v| v.name == name)
    }

    pub fn declarations(&self) -> &[FunctionDeclaration] {
        &self.declarations
    }

    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }

    pub fn add_declarations(&mut self, decls: Vec<FunctionDeclaration>) {
        self.declarations.extend(decls);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ToolSource {
    #[default]
    Local,
    Mcp {
        server: String,
    },
}

use crate::config::RoleExample;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: JsonSchema,
    #[serde(skip_serializing, default)]
    pub agent: bool,
    #[serde(skip, default)]
    pub source: ToolSource,
    #[serde(skip_serializing, default)]
    pub examples: Option<Vec<RoleExample>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JsonSchema {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<IndexMap<String, JsonSchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<JsonSchema>>,
    #[serde(rename = "anyOf", skip_serializing_if = "Option::is_none")]
    pub any_of: Option<Vec<JsonSchema>>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_value: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

impl JsonSchema {
    pub fn is_empty_properties(&self) -> bool {
        match &self.properties {
            Some(v) => v.is_empty(),
            None => true,
        }
    }
}

pub const TOOL_SEARCH_NAME: &str = "tool_search";

impl FunctionDeclaration {
    /// Creates the tool_search meta-function for deferred tool loading.
    pub fn tool_search() -> Self {
        let mut properties = IndexMap::new();
        properties.insert(
            "query".to_string(),
            JsonSchema {
                type_value: Some("string".to_string()),
                description: Some(
                    "Keyword to search for relevant tools. Use descriptive terms like 'file', 'web', 'database'.".to_string(),
                ),
                ..Default::default()
            },
        );
        Self {
            name: TOOL_SEARCH_NAME.to_string(),
            description: "Search for available tools by keyword. You MUST call this before using any other tool.".to_string(),
            parameters: JsonSchema {
                type_value: Some("object".to_string()),
                properties: Some(properties),
                required: Some(vec!["query".to_string()]),
                ..Default::default()
            },
            agent: false,
            source: ToolSource::default(),
            examples: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
    pub id: Option<String>,
}

type CallConfig = (String, String, Vec<String>, HashMap<String, String>);

impl ToolCall {
    pub fn dedup(calls: Vec<Self>) -> Vec<Self> {
        let mut new_calls = vec![];
        let mut seen_ids = HashSet::new();

        for call in calls.into_iter().rev() {
            if let Some(id) = &call.id {
                if !seen_ids.contains(id) {
                    seen_ids.insert(id.clone());
                    new_calls.push(call);
                }
            } else {
                new_calls.push(call);
            }
        }

        new_calls.reverse();
        new_calls
    }

    pub fn new(name: String, arguments: Value, id: Option<String>) -> Self {
        Self {
            name,
            arguments,
            id,
        }
    }

    pub fn eval(&self, config: &GlobalConfig) -> Result<Value> {
        // Phase 1C: Handle tool_search meta-function
        if self.name == TOOL_SEARCH_NAME {
            return self.eval_tool_search(config);
        }

        // Phase 2A: Handle pipeline-role tool calls
        if let Some(pipeline_stages) = self.check_pipeline_role(config) {
            return self.eval_pipeline_role(config, &pipeline_stages);
        }

        let (call_name, cmd_name, mut cmd_args, envs) = match &config.read().agent {
            Some(agent) => self.extract_call_config_from_agent(config, agent)?,
            None => self.extract_call_config_from_config(config)?,
        };

        let json_data = if self.arguments.is_object() {
            self.arguments.clone()
        } else if let Some(arguments) = self.arguments.as_str() {
            let arguments: Value = serde_json::from_str(arguments).map_err(|_| {
                anyhow!("The call '{call_name}' has invalid arguments: {arguments}")
            })?;
            arguments
        } else {
            bail!(
                "The call '{call_name}' has invalid arguments: {}",
                self.arguments
            );
        };

        cmd_args.push(json_data.to_string());

        let output = match run_llm_function(cmd_name, cmd_args, envs)? {
            Some(contents) => serde_json::from_str(&contents)
                .ok()
                .unwrap_or_else(|| json!({"output": contents})),
            None => Value::Null,
        };

        Ok(output)
    }

    /// Check if this tool call targets a pipeline role.
    fn check_pipeline_role(
        &self,
        config: &GlobalConfig,
    ) -> Option<Vec<crate::config::RolePipelineStage>> {
        // Don't check if it's already a known function
        if config.read().functions.contains(&self.name) {
            return None;
        }
        // Try to resolve as a role with pipeline
        if let Ok(role) = config.read().retrieve_role(&self.name) {
            if role.is_pipeline() {
                return role.pipeline().map(|p| p.to_vec());
            }
        }
        None
    }

    /// Execute a pipeline role as a tool call.
    fn eval_pipeline_role(
        &self,
        config: &GlobalConfig,
        stages: &[crate::config::RolePipelineStage],
    ) -> Result<Value> {
        let input_text = self
            .arguments
            .get("input")
            .and_then(|v| v.as_str())
            .or_else(|| {
                // Fallback: use the entire arguments as input if it's a string
                self.arguments.as_str()
            })
            .unwrap_or("")
            .to_string();

        if *IS_STDOUT_TERMINAL {
            println!("{}", dimmed_text(&format!("Call pipeline {}", self.name)));
        }

        // Use block_in_place to run async pipeline from sync context
        let config = config.clone();
        let stages = stages.to_vec();
        let result = tokio::task::block_in_place(move || {
            tokio::runtime::Handle::current().block_on(async {
                crate::pipe::run_pipeline_role(&config, &stages, &input_text).await
            })
        })?;

        Ok(json!({"output": result}))
    }

    /// Handles the tool_search meta-function for deferred tool loading.
    fn eval_tool_search(&self, config: &GlobalConfig) -> Result<Value> {
        let query = self
            .arguments
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        let deferred = config.read().deferred_tools.clone();
        let all_tools = match deferred {
            Some(ref state) => state.all_tools.clone(),
            None => {
                // Fallback: read all functions
                config.read().functions.declarations().to_vec()
            }
        };

        // Match tools by keyword against name and description
        let matched: Vec<&FunctionDeclaration> = all_tools
            .iter()
            .filter(|f| {
                query.is_empty()
                    || f.name.to_lowercase().contains(&query)
                    || f.description.to_lowercase().contains(&query)
            })
            .collect();

        // Build compact index
        let mut result = format!("Found {} tools matching \"{}\":\n", matched.len(), query);
        let mut active_names = Vec::new();
        for (i, f) in matched.iter().enumerate() {
            // Include parameter hints
            let params = f
                .parameters
                .properties
                .as_ref()
                .map(|props| {
                    props
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            result.push_str(&format!(
                "{}. {} - {} ({})\n",
                i + 1,
                f.name,
                f.description.lines().next().unwrap_or(""),
                params
            ));
            active_names.push(f.name.clone());
        }

        // Also check mapping_tools for group matches
        let mapping_tools = &config.read().mapping_tools;
        for (group, tools) in mapping_tools.iter() {
            if group.to_lowercase().contains(&query) {
                for tool_name in tools.split(',').map(|s| s.trim()) {
                    if !active_names.contains(&tool_name.to_string()) {
                        if let Some(f) = all_tools.iter().find(|f| f.name == tool_name) {
                            active_names.push(f.name.clone());
                        }
                    }
                }
            }
        }

        result.push_str("\nCall the tool by name with its parameters.");

        // Set active tools so next select_functions iteration returns them
        config.write().deferred_tools = Some(crate::config::DeferredToolState {
            all_tools: all_tools.clone(),
            active_tools: Some(active_names),
        });

        Ok(json!({"output": result}))
    }

    fn extract_call_config_from_agent(
        &self,
        config: &GlobalConfig,
        agent: &Agent,
    ) -> Result<CallConfig> {
        let function_name = self.name.clone();
        match agent.functions().find(&function_name) {
            Some(function) => {
                let agent_name = agent.name().to_string();
                if function.agent {
                    Ok((
                        format!("{agent_name}-{function_name}"),
                        agent_name,
                        vec![function_name],
                        agent.variable_envs(),
                    ))
                } else {
                    Ok((
                        function_name.clone(),
                        function_name,
                        vec![],
                        Default::default(),
                    ))
                }
            }
            None => self.extract_call_config_from_config(config),
        }
    }

    fn extract_call_config_from_config(&self, config: &GlobalConfig) -> Result<CallConfig> {
        let function_name = self.name.clone();
        match config.read().functions.contains(&function_name) {
            true => Ok((
                function_name.clone(),
                function_name,
                vec![],
                Default::default(),
            )),
            false => bail!("Unexpected call: {function_name} {}", self.arguments),
        }
    }
}

pub fn run_llm_function(
    cmd_name: String,
    cmd_args: Vec<String>,
    mut envs: HashMap<String, String>,
) -> Result<Option<String>> {
    let prompt = format!("Call {cmd_name} {}", cmd_args.join(" "));

    let mut bin_dirs: Vec<PathBuf> = vec![];
    if cmd_args.len() > 1 {
        let dir = Config::agent_functions_dir(&cmd_name).join("bin");
        if dir.exists() {
            bin_dirs.push(dir);
        }
    }
    bin_dirs.push(Config::functions_bin_dir());
    let current_path = std::env::var("PATH").context("No PATH environment variable")?;
    let prepend_path = bin_dirs
        .iter()
        .map(|v| format!("{}{PATH_SEP}", v.display()))
        .collect::<Vec<_>>()
        .join("");
    envs.insert("PATH".into(), format!("{prepend_path}{current_path}"));

    let temp_file = temp_file("-eval-", "");
    envs.insert("LLM_OUTPUT".into(), temp_file.display().to_string());

    #[cfg(windows)]
    let cmd_name = polyfill_cmd_name(&cmd_name, &bin_dirs);
    if *IS_STDOUT_TERMINAL {
        println!("{}", dimmed_text(&prompt));
    }
    let exit_code = run_command(&cmd_name, &cmd_args, Some(envs))
        .map_err(|err| anyhow!("Unable to run {cmd_name}, {err}"))?;
    if exit_code != 0 {
        bail!("Tool call exit with {exit_code}");
    }
    let mut output = None;
    if temp_file.exists() {
        let contents =
            fs::read_to_string(temp_file).context("Failed to retrieve tool call output")?;
        if !contents.is_empty() {
            output = Some(contents);
        }
    };
    Ok(output)
}

#[cfg(windows)]
fn polyfill_cmd_name<T: AsRef<Path>>(cmd_name: &str, bin_dir: &[T]) -> String {
    let cmd_name = cmd_name.to_string();
    if let Ok(exts) = std::env::var("PATHEXT") {
        for name in exts.split(';').map(|ext| format!("{cmd_name}{ext}")) {
            for dir in bin_dir {
                let path = dir.as_ref().join(&name);
                if path.exists() {
                    return name.to_string();
                }
            }
        }
    }
    cmd_name
}
