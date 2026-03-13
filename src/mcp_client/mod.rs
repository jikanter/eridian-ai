use crate::cli::{Cli, OutputFormat};
use crate::config::{GlobalConfig, McpServerConfig};
use crate::function::{FunctionDeclaration, ToolSource};

use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexMap;
use rmcp::model::{CallToolRequestParams, CallToolResult, RawContent, Tool};
use rmcp::service::RunningService;
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;

mod streamable_http;

// ---------------------------------------------------------------------------
// McpConnection — a single live connection to one MCP server
// ---------------------------------------------------------------------------

pub struct McpConnection {
    client: RunningService<RoleClient, ()>,
    tools: Vec<Tool>,
}

impl McpConnection {
    /// Spawn an MCP server process and perform the initialize handshake.
    pub async fn connect(
        command: &str,
        extra_args: &[String],
        envs: HashMap<String, String>,
    ) -> Result<Self> {
        let parts = shell_words::split(command)
            .with_context(|| format!("Invalid MCP server command: {command}"))?;
        let (program, args) = parts
            .split_first()
            .ok_or_else(|| anyhow!("Empty MCP server command"))?;

        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
        cmd.args(extra_args);
        for (k, v) in &envs {
            cmd.env(k, v);
        }

        let (transport, _stderr) = TokioChildProcess::builder(cmd)
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| {
                anyhow!(
                    "Could not start MCP server \"{command}\": {e}\n\
                     hint: ensure the server binary is installed and on PATH"
                )
            })?;

        let client = ().serve(transport).await.map_err(|e| {
            anyhow!(
                "MCP server \"{command}\" did not complete initialization: {e}\n\
                 hint: the server may require additional configuration"
            )
        })?;

        let tools = client.list_all_tools().await.map_err(|e| {
            anyhow!("Failed to list tools from MCP server \"{command}\": {e}")
        })?;

        Ok(Self { client, tools })
    }

    /// Connect to a remote MCP server over HTTP/SSE (Streamable HTTP transport).
    pub async fn connect_remote(
        endpoint: &str,
        headers: HashMap<String, String>,
    ) -> Result<Self> {
        let transport = streamable_http::build_transport(endpoint, &headers)?;

        let client = ().serve(transport).await.map_err(|e| {
            anyhow!(
                "Remote MCP server \"{endpoint}\" did not complete initialization: {e}\n\
                 hint: check the endpoint URL and authentication"
            )
        })?;

        let tools = client.list_all_tools().await.map_err(|e| {
            anyhow!("Failed to list tools from remote MCP server \"{endpoint}\": {e}")
        })?;

        Ok(Self { client, tools })
    }

    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult> {
        let name_owned: std::borrow::Cow<'static, str> = tool_name.to_string().into();
        let params = CallToolRequestParams {
            meta: None,
            name: name_owned,
            arguments,
            task: None,
        };
        self.client
            .call_tool(params)
            .await
            .map_err(|e| anyhow!("MCP tool '{tool_name}' failed: {e}"))
    }

    pub async fn shutdown(self) -> Result<()> {
        let _ = self.client.cancel().await;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// McpConnectionPool — manages named connections from config
// ---------------------------------------------------------------------------

pub struct McpConnectionPool {
    connections: RwLock<HashMap<String, McpConnection>>,
    configs: IndexMap<String, McpServerConfig>,
}

impl std::fmt::Debug for McpConnectionPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpConnectionPool")
            .field("servers", &self.configs.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl McpConnectionPool {
    pub fn new(configs: IndexMap<String, McpServerConfig>) -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            configs,
        }
    }

    /// Get or lazily create a connection to a named MCP server.
    async fn get_or_connect(&self, name: &str) -> Result<()> {
        {
            let conns = self.connections.read().await;
            if conns.contains_key(name) {
                return Ok(());
            }
        }

        let server_config = self
            .configs
            .get(name)
            .ok_or_else(|| anyhow!("No MCP server configured with name '{name}'"))?;

        let conn = if let Some(ref endpoint) = server_config.endpoint {
            McpConnection::connect_remote(endpoint, server_config.headers.clone()).await?
        } else {
            let envs = resolve_env_vars(&server_config.env);
            McpConnection::connect(&server_config.command, &server_config.args, envs).await?
        };

        let mut conns = self.connections.write().await;
        conns.insert(name.to_string(), conn);
        Ok(())
    }

    /// Return tool declarations from all configured servers, connecting lazily.
    pub async fn all_tool_declarations(&self) -> Result<Vec<FunctionDeclaration>> {
        let names: Vec<String> = self.configs.keys().cloned().collect();
        let mut all_decls = Vec::new();
        for name in &names {
            self.get_or_connect(name).await?;
            let conns = self.connections.read().await;
            if let Some(conn) = conns.get(name) {
                for tool in conn.tools() {
                    all_decls.push(mcp_tool_to_declaration(tool, name));
                }
            }
        }
        Ok(all_decls)
    }

    /// Call a tool on a named server. `tool_name` should be without the server prefix.
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value> {
        self.get_or_connect(server_name).await?;
        let conns = self.connections.read().await;
        let conn = conns
            .get(server_name)
            .ok_or_else(|| anyhow!("MCP server '{server_name}' not connected"))?;

        let args = match arguments {
            Value::Object(map) => Some(map),
            _ => None,
        };
        let result = conn.call_tool(tool_name, args).await?;

        let text = result
            .content
            .into_iter()
            .filter_map(|c| match c.raw {
                RawContent::Text(t) => Some(t.text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        if result.is_error.unwrap_or(false) {
            bail!("MCP tool '{server_name}:{tool_name}' returned error: {text}");
        }
        Ok(json!({"output": text}))
    }

    #[allow(dead_code)]
    pub async fn shutdown(self) {
        let mut conns = self.connections.into_inner();
        for (_, conn) in conns.drain() {
            let _ = conn.shutdown().await;
        }
    }
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

/// Convert an rmcp Tool to our FunctionDeclaration, namespaced by server name.
/// Preserves the full JSON Schema from the MCP tool without lossy conversion.
pub fn mcp_tool_to_declaration(tool: &Tool, server_name: &str) -> FunctionDeclaration {
    let name = format!("{}:{}", server_name, tool.name);
    let description = tool.description.clone().unwrap_or_default().to_string();
    let parameters = Value::Object(tool.input_schema.as_ref().clone());

    FunctionDeclaration {
        name,
        description,
        parameters,
        agent: false,
        source: ToolSource::Mcp {
            server: server_name.to_string(),
        },
        examples: None,
    }
}

/// Convert rmcp Tools to a JSON array for `--list-tools` output.
fn tools_to_json(tools: &[Tool]) -> Value {
    let items: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name.to_string(),
                "description": t.description.as_ref().map(|d| d.to_string()).unwrap_or_default(),
                "parameters": Value::Object(t.input_schema.as_ref().clone()),
            })
        })
        .collect();
    Value::Array(items)
}

/// Convert a single rmcp Tool to JSON for `--tool-info` output.
fn tool_to_json(tool: &Tool) -> Value {
    json!({
        "name": tool.name.to_string(),
        "description": tool.description.as_ref().map(|d| d.to_string()).unwrap_or_default(),
        "parameters": Value::Object(tool.input_schema.as_ref().clone()),
    })
}

// ---------------------------------------------------------------------------
// Schema cache
// ---------------------------------------------------------------------------

fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".cache"))
        .join("aichat")
        .join("mcp")
}

fn cache_key(command: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(command.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    tools_json: Value,
    fetched_at: String,
}

fn read_cache(command: &str) -> Option<Value> {
    let path = cache_dir().join(format!("{}.json", cache_key(command)));
    let content = std::fs::read_to_string(&path).ok()?;
    let entry: CacheEntry = serde_json::from_str(&content).ok()?;

    // Check TTL (1 hour)
    let fetched = chrono::DateTime::parse_from_rfc3339(&entry.fetched_at).ok()?;
    let now = chrono::Utc::now();
    if now.signed_duration_since(fetched).num_seconds() > 3600 {
        return None;
    }
    Some(entry.tools_json)
}

fn write_cache(command: &str, tools_json: &Value) -> Result<()> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", cache_key(command)));
    let entry = CacheEntry {
        tools_json: tools_json.clone(),
        fetched_at: chrono::Utc::now().to_rfc3339(),
    };
    let content = serde_json::to_string_pretty(&entry)?;
    std::fs::write(path, content)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Environment variable resolution for config-based servers
// ---------------------------------------------------------------------------

fn resolve_env_vars(env_map: &HashMap<String, String>) -> HashMap<String, String> {
    env_map
        .iter()
        .map(|(k, v)| {
            let resolved = if v.starts_with("${") && v.ends_with('}') {
                // ${VAR} → read from parent env
                let var_name = &v[2..v.len() - 1];
                std::env::var(var_name).unwrap_or_default()
            } else {
                v.clone()
            };
            (k.clone(), resolved)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// CLI entry point: aichat --mcp-server <CMD> [--list-tools | --call | --tool-info]
// ---------------------------------------------------------------------------

pub async fn run_mcp_client_command(cli: &Cli, server_cmd: &str) -> Result<()> {
    if cli.list_tools {
        return cmd_list_tools(cli, server_cmd).await;
    }
    if let Some(ref tool_name) = cli.tool_info {
        return cmd_tool_info(server_cmd, tool_name).await;
    }
    if let Some(ref tool_name) = cli.call {
        return cmd_call_tool(cli, server_cmd, tool_name).await;
    }

    bail!(
        "--mcp-server requires one of: --list-tools, --tool-info <TOOL>, or --call <TOOL>\n\
         Example: aichat --mcp-server \"{server_cmd}\" --list-tools"
    );
}

/// Detect whether a --mcp-server argument is a remote HTTP endpoint or a local command.
fn is_remote_endpoint(server_cmd: &str) -> bool {
    server_cmd.starts_with("http://") || server_cmd.starts_with("https://")
}

async fn connect_for_cli(server_cmd: &str) -> Result<McpConnection> {
    if is_remote_endpoint(server_cmd) {
        McpConnection::connect_remote(server_cmd, Default::default()).await
    } else {
        McpConnection::connect(server_cmd, &[], Default::default()).await
    }
}

async fn cmd_list_tools(cli: &Cli, server_cmd: &str) -> Result<()> {
    // Check cache first
    if let Some(cached) = read_cache(server_cmd) {
        let output = format_tools_output(&cached, cli.output_format);
        println!("{output}");
        return Ok(());
    }

    let conn = connect_for_cli(server_cmd).await?;
    let tools_json = tools_to_json(conn.tools());

    // Cache the result
    let _ = write_cache(server_cmd, &tools_json);

    let output = format_tools_output(&tools_json, cli.output_format);
    println!("{output}");

    conn.shutdown().await?;
    Ok(())
}

fn format_tools_output(tools_json: &Value, output_format: Option<OutputFormat>) -> String {
    match output_format {
        Some(OutputFormat::Json) => serde_json::to_string_pretty(tools_json).unwrap_or_default(),
        _ => {
            // Default human-readable: one tool per line with description
            if let Some(arr) = tools_json.as_array() {
                arr.iter()
                    .map(|t| {
                        let name = t["name"].as_str().unwrap_or("?");
                        let desc = t["description"].as_str().unwrap_or("");
                        if desc.is_empty() {
                            name.to_string()
                        } else {
                            format!("{name} - {desc}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                String::new()
            }
        }
    }
}

async fn cmd_tool_info(server_cmd: &str, tool_name: &str) -> Result<()> {
    let conn = connect_for_cli(server_cmd).await?;
    let tool = conn
        .tools()
        .iter()
        .find(|t| t.name.as_ref() == tool_name)
        .ok_or_else(|| anyhow!("Tool '{tool_name}' not found on server"))?;

    let json = serde_json::to_string_pretty(&tool_to_json(tool))?;
    println!("{json}");

    conn.shutdown().await?;
    Ok(())
}

async fn cmd_call_tool(cli: &Cli, server_cmd: &str, tool_name: &str) -> Result<()> {
    let arguments = parse_call_arguments(cli)?;

    let conn = connect_for_cli(server_cmd).await?;
    let result = conn.call_tool(tool_name, arguments).await?;

    if result.is_error.unwrap_or(false) {
        let text = extract_text_content(&result);
        conn.shutdown().await?;
        bail!("MCP tool '{tool_name}' returned error: {text}");
    }

    let text = extract_text_content(&result);
    if !text.is_empty() {
        println!("{text}");
    }

    conn.shutdown().await?;
    Ok(())
}

fn extract_text_content(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| match &c.raw {
            RawContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_call_arguments(cli: &Cli) -> Result<Option<Map<String, Value>>> {
    match &cli.call_json {
        Some(json_str) => {
            let val: Value = serde_json::from_str(json_str)
                .with_context(|| format!("Invalid JSON in --json: {json_str}"))?;
            match val {
                Value::Object(map) => Ok(Some(map)),
                _ => bail!("--json must be a JSON object, got: {val}"),
            }
        }
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Async tool evaluation — called from the tool-calling loop for MCP-sourced tools
// ---------------------------------------------------------------------------

/// Execute an MCP tool call via the connection pool.
/// `namespaced_name` is e.g. "github:create-issue".
pub async fn eval_mcp_tool(config: &GlobalConfig, call_name: &str, arguments: Value) -> Result<Value> {
    let (server_name, tool_name) = call_name
        .split_once(':')
        .ok_or_else(|| anyhow!("Invalid MCP tool name (missing server prefix): {call_name}"))?;

    let pool = {
        let cfg = config.read();
        cfg.mcp_pool
            .clone()
            .ok_or_else(|| anyhow!("No MCP connection pool configured"))?
    };

    pool.call_tool(server_name, tool_name, arguments).await
}
