use crate::{
    config::{Agent, Config, GlobalConfig, Input},
    utils::*,
};

use anyhow::{anyhow, bail, Context, Result};
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

/// Returns true if `tool_name` resolves to an MCP-sourced tool with a live
/// connection pool. Shared between `eval_tool_calls` (batch path) and
/// `ToolCall::eval` (single-call path used by `aichat --mcp`).
pub fn is_mcp_call(config: &GlobalConfig, tool_name: &str) -> bool {
    if !tool_name.contains(':') {
        return false;
    }
    let cfg = config.read();
    cfg.mcp_pool.is_some()
        && cfg
            .functions
            .find(tool_name)
            .map(|d| matches!(d.source, ToolSource::Mcp { .. }))
            .unwrap_or(false)
}

pub async fn eval_tool_calls(config: &GlobalConfig, mut calls: Vec<ToolCall>) -> Result<Vec<ToolResult>> {
    if calls.is_empty() {
        return Ok(vec![]);
    }
    calls = ToolCall::dedup(calls);
    if calls.is_empty() {
        bail!("The request was aborted because an infinite loop of function calls was detected.")
    }

    // Phase 8B: Determine MCP status for each call before concurrent execution.
    // Phase 31A: routing predicate moved to `is_mcp_call` so the single-call
    // path (`ToolCall::eval`) shares the same logic.
    let call_infos: Vec<(ToolCall, bool)> = calls
        .into_iter()
        .map(|call| {
            let is_mcp = is_mcp_call(config, &call.name);
            (call, is_mcp)
        })
        .collect();

    // Phase 8B: Run all tool calls concurrently using join_all.
    // Each call is independent — errors are per-tool (Phase 7 pattern).
    let futures: Vec<_> = call_infos
        .into_iter()
        .map(|(call, is_mcp)| {
            let config = config.clone();
            async move {
                let result = eval_single_tool(&config, &call, is_mcp).await;
                // Phase 7A: Null → structured null-result
                let result = if result.is_null() {
                    json!({"status": "ok", "output": null})
                } else {
                    result
                };
                ToolResult::new(call, result)
            }
        })
        .collect();

    let output = futures_util::future::join_all(futures).await;
    Ok(output)
}

/// Evaluate a single tool call, catching errors as ToolResult values.
async fn eval_single_tool(config: &GlobalConfig, call: &ToolCall, is_mcp: bool) -> Value {
    if is_mcp {
        match crate::mcp_client::eval_mcp_tool(config, &call.name, call.arguments.clone()).await {
            Ok(v) => v,
            Err(e) => {
                let error_msg = format_tool_error_for_llm(&call.name, &e);
                if *IS_STDOUT_TERMINAL {
                    eprintln!("{}", warning_text(&format!("tool '{}' failed: {}", call.name, e)));
                }
                json!(error_msg)
            }
        }
    } else {
        match call.eval(config).await {
            Ok(v) => v,
            Err(e) => {
                let error_msg = format_tool_error_for_llm(&call.name, &e);
                if *IS_STDOUT_TERMINAL {
                    eprintln!("{}", warning_text(&format!("tool '{}' failed: {}", call.name, e)));
                }
                json!(error_msg)
            }
        }
    }
}

/// Format a tool error into a concise message for the LLM.
/// Plain text with [TOOL_ERROR] prefix so system prompts can reference it.
/// Target: under 300 tokens.
fn format_tool_error_for_llm(tool_name: &str, err: &anyhow::Error) -> String {
    if let Some(exec_err) =
        err.downcast_ref::<crate::utils::exit_code::AichatError>()
    {
        match exec_err {
            crate::utils::exit_code::AichatError::ToolExecutionError {
                exit_code,
                stderr,
                hint,
                ..
            } => {
                let mut msg =
                    format!("[TOOL_ERROR] {tool_name} failed (exit {exit_code}).");
                if let Some(stderr) = stderr {
                    if !stderr.is_empty() {
                        msg.push_str(&format!("\nStderr: {stderr}"));
                    }
                }
                if let Some(hint) = hint {
                    msg.push_str(&format!("\nHint: {hint}"));
                }
                msg
            }
            crate::utils::exit_code::AichatError::ToolSpawnError {
                message, hint, ..
            } => {
                let mut msg =
                    format!("[TOOL_ERROR] {tool_name} could not be started: {message}.");
                if let Some(hint) = hint {
                    msg.push_str(&format!("\nHint: {hint}"));
                }
                msg
            }
            crate::utils::exit_code::AichatError::ToolTimeout {
                timeout_secs, ..
            } => {
                format!(
                    "[TOOL_ERROR] {tool_name} timed out after {timeout_secs}s.\n\
                     Hint: increase timeout with tool_timeout in config or per-tool \"timeout\" in functions.json."
                )
            }
            other => {
                format!("[TOOL_ERROR] {tool_name} failed: {other}")
            }
        }
    } else {
        format!("[TOOL_ERROR] {tool_name} failed: {err}")
    }
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
    pub parameters: Value,
    #[serde(skip_serializing, default)]
    pub agent: bool,
    #[serde(skip, default)]
    pub source: ToolSource,
    #[serde(skip_serializing, default)]
    pub examples: Option<Vec<RoleExample>>,
    /// Per-tool timeout in seconds. Overrides global `tool_timeout`. 0 = use global.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub timeout: Option<u64>,
}

impl FunctionDeclaration {
    pub fn is_empty_parameters(&self) -> bool {
        match self.parameters.get("properties") {
            Some(Value::Object(map)) => map.is_empty(),
            Some(_) => false,
            None => true,
        }
    }
}


pub const TOOL_SEARCH_NAME: &str = "tool_search";
pub const SEARCH_KNOWLEDGE_NAME: &str = "search_knowledge";

/// Phase 28B: name of the synthetic `finish` tool. The model calls it to end
/// the react loop cleanly with an explicit final answer; `call_react` detects
/// the call by this name and terminates.
pub const FINISH_NAME: &str = "finish";

/// Phase 28A: true when `depth` has reached the delegation cap and a further
/// agent-as-tool call must be refused.
pub fn agent_depth_exceeded(depth: usize, max_depth: usize) -> bool {
    depth >= max_depth
}

/// Phase 28A: decide whether `name` should dispatch as an agent-as-tool call.
/// A real function always wins a name collision — agent-as-tool must never
/// hijack an existing tool — so the name must be a known agent AND not a known
/// function.
pub fn is_agent_tool(
    name: &str,
    agents: &HashSet<String>,
    functions: &HashSet<String>,
) -> bool {
    agents.contains(name) && !functions.contains(name)
}

/// Phase 28B: append the synthetic `finish` tool when the role configured a
/// bounded react loop (`react_max_steps`), giving the model an explicit clean
/// exit. No-op when the cap is unset, no tools are active, or `finish` is
/// already present — so default tool turns are unchanged (token-conscious).
pub fn maybe_inject_finish(
    functions: &mut Vec<FunctionDeclaration>,
    react_max_steps: Option<usize>,
) {
    if react_max_steps.is_some()
        && !functions.is_empty()
        && !functions.iter().any(|f| f.name == FINISH_NAME)
    {
        functions.push(FunctionDeclaration::finish());
    }
}

impl FunctionDeclaration {
    /// Creates the tool_search meta-function for deferred tool loading.
    pub fn tool_search() -> Self {
        Self {
            name: TOOL_SEARCH_NAME.to_string(),
            description: "Search for available tools by keyword. You MUST call this before using any other tool.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keyword to search for relevant tools. Use descriptive terms like 'file', 'web', 'database'."
                    }
                },
                "required": ["query"]
            }),
            agent: false,
            source: ToolSource::default(),
            examples: None,
            timeout: None,
        }
    }

    /// Phase 26E: synthetic `search_knowledge` tool. Injected when the active
    /// role sets `knowledge_mode: tool` — in that mode facts are NOT
    /// auto-attached to the user message; instead the LLM decides when to
    /// search by calling this tool.
    pub fn search_knowledge() -> Self {
        Self {
            name: SEARCH_KNOWLEDGE_NAME.to_string(),
            description: "Search the configured knowledge base(s) for atomic facts relevant to a query. Returns entity-description pairs with provenance. Pass an optional `tags` array of `namespace:value` predicates to narrow the candidate set.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural-language query. BM25 ranks facts against this text."
                    },
                    "tags": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Optional AND-joined tag predicates in `namespace:value` form."
                    }
                },
                "required": ["query"]
            }),
            agent: false,
            source: ToolSource::default(),
            examples: None,
            timeout: None,
        }
    }

    /// Phase 28A: declaration exposing an agent as a callable tool. The model
    /// delegates a task by calling it with a single `input` string; the
    /// sub-agent runs in its own context window (token isolation) and returns
    /// its final output. `agent` is set so the agent path's tool merge treats
    /// it correctly.
    pub fn agent_as_tool(name: &str, description: &str) -> Self {
        let description = if description.is_empty() {
            format!("Delegate a task to the `{name}` agent. It runs autonomously in its own context and returns a final answer.")
        } else {
            format!("{description} (Delegate a task to the `{name}` agent; it runs in its own context and returns a final answer.)")
        };
        Self {
            name: name.to_string(),
            description,
            parameters: json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "The task or question to delegate to the agent. Be self-contained: the agent does not see this conversation."
                    }
                },
                "required": ["input"]
            }),
            agent: true,
            source: ToolSource::default(),
            examples: None,
            timeout: None,
        }
    }

    /// Phase 28B: synthetic `finish` tool. Injected when the role sets
    /// `react_max_steps`. The model calls it to terminate the react loop
    /// explicitly, passing its final answer as `summary`.
    pub fn finish() -> Self {
        Self {
            name: FINISH_NAME.to_string(),
            description: "Finish the task and return the final answer. Call this when you have completed all work and have nothing more to do. Pass your complete final answer as `summary`.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "The complete final answer to return to the user."
                    }
                },
                "required": ["summary"]
            }),
            agent: false,
            source: ToolSource::default(),
            examples: None,
            timeout: None,
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

    pub async fn eval(&self, config: &GlobalConfig) -> Result<Value> {
        // Phase 1C: Handle tool_search meta-function
        if self.name == TOOL_SEARCH_NAME {
            return self.eval_tool_search(config);
        }

        // Phase 26E: Handle search_knowledge synthetic tool.
        if self.name == SEARCH_KNOWLEDGE_NAME {
            return self.eval_search_knowledge(config);
        }

        // Phase 28B: synthetic `finish` tool — explicit clean termination of
        // the react loop. Echoes the `summary` argument as output;
        // `call_react` detects the call by name and stops the loop.
        if self.name == FINISH_NAME {
            let summary = self
                .arguments
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            return Ok(json!({ "output": summary }));
        }

        // Phase 31A: route MCP-sourced tools through the connection pool.
        // Without this branch the `aichat --mcp` server (which dispatches via
        // this single-call path) falls through to the llm-functions binary
        // lookup and fails with "binary not found" for any namespaced call
        // like `git:git_status`. Mirrors `eval_tool_calls`/`eval_single_tool`.
        if is_mcp_call(config, &self.name) {
            return crate::mcp_client::eval_mcp_tool(
                config,
                &self.name,
                self.arguments.clone(),
            )
            .await;
        }

        // Phase 2A: Handle pipeline-role tool calls
        if let Some(pipeline_stages) = self.check_pipeline_role(config) {
            return self.eval_pipeline_role(config, &pipeline_stages).await;
        }

        // Phase 28A: agent-as-tool. When the tool name matches a known agent
        // (and not a real function), run it as a delegated sub-agent with its
        // own context window and bounded recursion depth.
        if let Some(agent_name) = self.check_agent(config) {
            return self.eval_agent(config, &agent_name).await;
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

        // Phase 8A: Resolve timeout — per-tool overrides global
        let timeout_secs = resolve_tool_timeout(config, &call_name);

        // Phase 36B: per-stage working directory (pipeline `config_override.
        // working_directory`). `None` outside an isolated stage — tools spawn
        // in aichat's cwd as before.
        let working_directory = config.read().working_directory.clone();

        let output = match run_llm_function(
            cmd_name,
            cmd_args,
            envs,
            timeout_secs,
            working_directory,
        )
        .await?
        {
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
    ) -> Option<Vec<crate::config::PipelineNode>> {
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
    async fn eval_pipeline_role(
        &self,
        config: &GlobalConfig,
        nodes: &[crate::config::PipelineNode],
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

        // Phase 21D: detect self-references and transitive pipeline cycles
        // before triggering the (potentially expensive) tool dispatch.
        crate::config::preflight::validate_pipeline_dag_cycles(
            &config.read(),
            &self.name,
            nodes,
        )?;

        let result =
            crate::pipe::run_pipeline_role(config, nodes, &input_text).await?;

        Ok(json!({"output": result}))
    }

    /// Phase 28A: resolve this tool call to a known agent, if any. Returns the
    /// agent name when the call should dispatch as an agent-as-tool delegation.
    /// A real function with the same name always wins (no hijacking).
    fn check_agent(&self, config: &GlobalConfig) -> Option<String> {
        let functions: HashSet<String> = config
            .read()
            .functions
            .declarations()
            .iter()
            .map(|f| f.name.clone())
            .collect();
        let agents: HashSet<String> = crate::config::list_agents().into_iter().collect();
        if is_agent_tool(&self.name, &agents, &functions) {
            Some(self.name.clone())
        } else {
            None
        }
    }

    /// Phase 28A: run a known agent as a delegated sub-agent (agent-as-tool).
    /// The sub-agent executes in a *cloned* config with its own context window —
    /// the parent's messages are never passed, only the `input` argument — and
    /// at an incremented delegation depth, bounded by `react_max_depth`.
    async fn eval_agent(&self, config: &GlobalConfig, agent_name: &str) -> Result<Value> {
        // Recursion guard: refuse to delegate past the configured depth.
        let depth = config.read().agent_depth;
        let max_depth = config
            .read()
            .react_max_depth
            .unwrap_or(crate::config::DEFAULT_REACT_MAX_DEPTH);
        if agent_depth_exceeded(depth, max_depth) {
            return Ok(json!({
                "output": format!(
                    "[TOOL_ERROR] Agent delegation depth exceeded (max {max_depth}). \
                     Complete the task without delegating to another agent."
                )
            }));
        }

        let input_text = self
            .arguments
            .get("input")
            .and_then(|v| v.as_str())
            .or_else(|| self.arguments.as_str())
            .unwrap_or("")
            .to_string();

        if *IS_STDOUT_TERMINAL {
            println!("{}", dimmed_text(&format!("Call agent {agent_name}")));
        }

        // Token isolation: clone the config, strip the parent's role/session/
        // agent/rag so `use_agent` can bind the sub-agent cleanly, and bump the
        // delegation depth. The sub-agent's only input is this call's `input`.
        let mut sub = config.read().clone();
        sub.role = None;
        sub.session = None;
        sub.agent = None;
        sub.rag = None;
        sub.agent_depth = depth + 1;
        let sub = std::sync::Arc::new(parking_lot::RwLock::new(sub));

        let abort_signal = create_abort_signal();
        Config::use_agent(&sub, agent_name, None, abort_signal.clone()).await?;

        let mut input = Input::from_str(&sub, &input_text, None);
        let client = input.create_client()?;
        let (output, _tool_results, _metrics) =
            crate::client::call_react(&mut input, client.as_ref(), abort_signal).await?;

        Ok(json!({ "output": output }))
    }

    /// Phase 26E: handle `search_knowledge` synthetic tool calls. Resolves
    /// the role's + CLI's knowledge bindings, runs the Phase 26A/B pipeline,
    /// and returns hits as a structured JSON payload for the LLM.
    fn eval_search_knowledge(&self, config: &GlobalConfig) -> Result<Value> {
        let query = self
            .arguments
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if query.is_empty() {
            return Ok(serde_json::json!({
                "results": [],
                "note": "search_knowledge called with empty query",
            }));
        }

        let extra_tag_strings: Vec<String> = self
            .arguments
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        // Merge role + CLI knowledge bindings; add tool-supplied tag
        // predicates to each binding's declared tags.
        let role = config.read().extract_role();
        let mut bindings: Vec<crate::config::KnowledgeBinding> =
            role.knowledge_bindings().to_vec();
        for name in config.read().cli_knowledge_bindings.clone() {
            if !bindings.iter().any(|b| b.name == name) {
                bindings.push(crate::config::KnowledgeBinding::simple(name));
            }
        }
        if bindings.is_empty() {
            return Ok(serde_json::json!({
                "results": [],
                "note": "no knowledge bindings are active — declare `knowledge:` in the role or pass `--knowledge <name>`",
            }));
        }
        for b in bindings.iter_mut() {
            for t in &extra_tag_strings {
                if !b.tags.contains(t) {
                    b.tags.push(t.clone());
                }
            }
        }

        let hits = crate::knowledge::retrieve::retrieve_from_bindings(
            &bindings,
            &query,
            &crate::knowledge::retrieve::RetrievalOptions {
                top_k: None,
                token_budget: None,
                graph_expand: true,
                include_deprecated: false,
            },
        )?;

        Ok(serde_json::json!({
            "results": crate::knowledge::query::hits_to_json(&hits),
        }))
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
                .get("properties")
                .and_then(|v| v.as_object())
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

        // Also check mapping_tools for group matches.
        // Clone to drop the read guard before acquiring the write lock below;
        // binding `&config.read().mapping_tools` would extend the read guard's
        // lifetime to end of scope and self-deadlock at `config.write()`
        // (parking_lot RwLock is not re-entrant).
        let mapping_tools = config.read().mapping_tools.clone();
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

pub async fn run_llm_function(
    cmd_name: String,
    cmd_args: Vec<String>,
    mut envs: HashMap<String, String>,
    timeout_secs: u64,
    working_directory: Option<PathBuf>,
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

    // Phase 7B: Pre-flight checks before spawning
    preflight_check(&cmd_name, &bin_dirs)?;

    if *IS_STDOUT_TERMINAL {
        println!("{}", dimmed_text(&prompt));
    }

    // Phase 8A: Async execution with timeout support
    let (exit_code, stderr) =
        run_command_with_stderr_timeout(
            &cmd_name,
            &cmd_args,
            envs,
            timeout_secs,
            working_directory.as_deref(),
        )
        .await
            .map_err(|err| {
                // Check if it's already a typed error (e.g., ToolTimeout)
                if err.downcast_ref::<crate::utils::exit_code::AichatError>().is_some() {
                    return err;
                }
                let hint = spawn_error_hint(&err);
                anyhow::Error::new(crate::utils::exit_code::AichatError::ToolSpawnError {
                    tool_name: cmd_name.clone(),
                    message: err.to_string(),
                    hint,
                })
            })?;

    // Log stderr at debug level even on success (tool warnings)
    if !stderr.is_empty() && exit_code == 0 {
        debug!("Tool '{cmd_name}' stderr: {stderr}");
    }

    if exit_code != 0 {
        let stderr_display = truncate_stderr(&stderr, 15);
        let hint = generate_tool_hint(exit_code, &stderr);
        return Err(anyhow::Error::new(
            crate::utils::exit_code::AichatError::ToolExecutionError {
                tool_name: cmd_name,
                exit_code,
                stderr: if stderr_display.is_empty() {
                    None
                } else {
                    Some(stderr_display)
                },
                hint: Some(hint),
            },
        ));
    }

    let mut output = None;
    if temp_file.exists() {
        let contents =
            fs::read_to_string(&temp_file).context("Failed to retrieve tool call output")?;
        if !contents.is_empty() {
            output = Some(contents);
        }
        // Clean up temp file on success
        let _ = fs::remove_file(&temp_file);
    };
    Ok(output)
}

/// Resolve the effective timeout for a tool call.
/// Per-tool timeout overrides global. 0 = disabled.
fn resolve_tool_timeout(config: &GlobalConfig, tool_name: &str) -> u64 {
    let cfg = config.read();
    // Check per-tool timeout first
    if let Some(decl) = cfg.functions.find(tool_name) {
        if let Some(timeout) = decl.timeout {
            if timeout > 0 {
                return timeout;
            }
        }
    }
    // Fall back to global config
    cfg.tool_timeout
}

/// Truncate stderr to the last N lines for display.
fn truncate_stderr(stderr: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = stderr.lines().collect();
    if lines.len() <= max_lines {
        stderr.trim().to_string()
    } else {
        let total = lines.len();
        let tail = &lines[total - max_lines..];
        format!(
            "[{} lines total, showing last {}]\n{}",
            total,
            max_lines,
            tail.join("\n")
        )
    }
}

/// Generate contextual hint based on exit code and stderr content.
fn generate_tool_hint(exit_code: i32, stderr: &str) -> String {
    let stderr_lower = stderr.to_lowercase();
    if exit_code == 127 {
        "the tool binary was not found on PATH.".to_string()
    } else if exit_code == 126 {
        "the tool binary exists but is not executable. Try: chmod +x <path>".to_string()
    } else if stderr_lower.contains("not found") || stderr_lower.contains("no such file") {
        "a dependency may be missing. Check the tool's requirements.".to_string()
    } else if stderr_lower.contains("permission denied") {
        "check file permissions on the tool binary.".to_string()
    } else if stderr_lower.contains("econnrefused") || stderr_lower.contains("connection refused")
    {
        "a network service the tool depends on may be down.".to_string()
    } else if stderr_lower.contains("rate limit") || stderr_lower.contains("429") {
        "the tool hit a rate limit. Wait and retry.".to_string()
    } else {
        "run the command manually to diagnose.".to_string()
    }
}

/// Generate hint for spawn failures.
fn spawn_error_hint(err: &anyhow::Error) -> Option<String> {
    let msg = err.to_string().to_lowercase();
    if msg.contains("not found") || msg.contains("no such file") {
        Some("ensure the tool binary is installed and on PATH.".to_string())
    } else if msg.contains("permission denied") {
        Some("check file permissions. Try: chmod +x <path>".to_string())
    } else {
        None
    }
}

/// Pre-flight checks before spawning a tool process.
fn preflight_check(cmd_name: &str, bin_dirs: &[PathBuf]) -> Result<()> {
    // Check if the binary can be found in bin_dirs or system PATH
    let found_in_bin_dirs = bin_dirs.iter().any(|dir| dir.join(cmd_name).exists());
    if !found_in_bin_dirs {
        // Check system PATH via which/where
        let in_system_path = std::process::Command::new("which")
            .arg(cmd_name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !in_system_path {
            let searched: Vec<_> = bin_dirs.iter().map(|d| d.display().to_string()).collect();
            return Err(anyhow::Error::new(
                crate::utils::exit_code::AichatError::ToolSpawnError {
                    tool_name: cmd_name.to_string(),
                    message: format!("binary not found"),
                    hint: Some(format!(
                        "searched: {}. Ensure the tool is installed.",
                        searched.join(", ")
                    )),
                },
            ));
        }
    } else {
        // Binary found in bin_dirs — check if executable (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for dir in bin_dirs {
                let path = dir.join(cmd_name);
                if path.exists() {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        if meta.permissions().mode() & 0o111 == 0 {
                            return Err(anyhow::Error::new(
                                crate::utils::exit_code::AichatError::ToolSpawnError {
                                    tool_name: cmd_name.to_string(),
                                    message: "binary is not executable".to_string(),
                                    hint: Some(format!("run: chmod +x {}", path.display())),
                                },
                            ));
                        }
                    }
                    break;
                }
            }
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Phase 28B: synthetic `finish` tool ----

    #[test]
    fn finish_declaration_has_name_and_summary_param() {
        let decl = FunctionDeclaration::finish();
        assert_eq!(decl.name, FINISH_NAME);
        assert!(!decl.agent);
        let props = &decl.parameters["properties"];
        assert!(props.get("summary").is_some(), "expected a `summary` param");
    }

    #[test]
    fn maybe_inject_finish_appends_when_react_cap_set() {
        let mut fns = vec![FunctionDeclaration::tool_search()];
        maybe_inject_finish(&mut fns, Some(5));
        assert!(fns.iter().any(|f| f.name == FINISH_NAME));
    }

    #[test]
    fn maybe_inject_finish_noop_when_cap_unset() {
        // Default turns (no react_max_steps) must be unchanged — token-conscious.
        let mut fns = vec![FunctionDeclaration::tool_search()];
        maybe_inject_finish(&mut fns, None);
        assert!(!fns.iter().any(|f| f.name == FINISH_NAME));
    }

    #[test]
    fn maybe_inject_finish_noop_when_no_tools() {
        // A bounded react loop with no tools has no loop to finish.
        let mut fns: Vec<FunctionDeclaration> = vec![];
        maybe_inject_finish(&mut fns, Some(3));
        assert!(fns.is_empty());
    }

    #[test]
    fn maybe_inject_finish_idempotent() {
        let mut fns = vec![FunctionDeclaration::finish()];
        maybe_inject_finish(&mut fns, Some(5));
        assert_eq!(fns.iter().filter(|f| f.name == FINISH_NAME).count(), 1);
    }

    // ---- Phase 28A: agent-as-tool ----

    #[test]
    fn agent_as_tool_declaration_has_input_param_and_agent_flag() {
        let decl = FunctionDeclaration::agent_as_tool("code-reviewer", "Reviews code");
        assert_eq!(decl.name, "code-reviewer");
        assert!(decl.agent, "agent-as-tool declarations are flagged agent=true");
        assert!(decl.description.contains("Reviews code"));
        let props = &decl.parameters["properties"];
        assert!(props.get("input").is_some(), "expected an `input` param");
        assert_eq!(decl.parameters["required"][0], "input");
    }

    #[test]
    fn agent_depth_exceeded_at_or_above_max() {
        assert!(!agent_depth_exceeded(0, 3));
        assert!(!agent_depth_exceeded(2, 3));
        assert!(agent_depth_exceeded(3, 3));
        assert!(agent_depth_exceeded(4, 3));
    }

    #[test]
    fn is_agent_tool_matches_known_agent_not_function() {
        let agents: HashSet<String> = ["reviewer".to_string()].into_iter().collect();
        let functions: HashSet<String> = ["fs_read".to_string()].into_iter().collect();
        assert!(is_agent_tool("reviewer", &agents, &functions));
        // A real function name with the same string is never shadowed.
        assert!(!is_agent_tool("fs_read", &agents, &functions));
        // Unknown name.
        assert!(!is_agent_tool("nope", &agents, &functions));
    }

    #[test]
    fn is_agent_tool_function_wins_over_agent_name_collision() {
        // If a name is both a function and an agent, the function takes
        // precedence (agent-as-tool must not hijack a real tool).
        let agents: HashSet<String> = ["dup".to_string()].into_iter().collect();
        let functions: HashSet<String> = ["dup".to_string()].into_iter().collect();
        assert!(!is_agent_tool("dup", &agents, &functions));
    }
}
