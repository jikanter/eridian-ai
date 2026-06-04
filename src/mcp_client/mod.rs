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
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

mod streamable_http;

// ---------------------------------------------------------------------------
// McpConnection — a single live connection to one MCP server
// ---------------------------------------------------------------------------

pub struct McpConnection {
    client: RunningService<RoleClient, ()>,
    tools: Vec<Tool>,
    last_used: std::sync::atomic::AtomicI64,
}

impl McpConnection {
    /// Spawn an MCP server process and perform the initialize handshake.
    /// `startup_timeout` is in seconds (0 = no timeout).
    pub async fn connect(
        command: &str,
        extra_args: &[String],
        envs: HashMap<String, String>,
        startup_timeout: u64,
    ) -> Result<Self> {
        let parts = shell_words::split(command)
            .with_context(|| format!("Invalid MCP server command: {command}"))?;
        let (program, args) = parts
            .split_first()
            .ok_or_else(|| anyhow!("Empty MCP server command"))?;

        info!("Spawning MCP server: {command}");
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
        cmd.args(extra_args);
        // Phase 31B: ensure children die when their TokioChildProcess transport
        // is dropped (e.g. on startup timeout). Without this, an orphan stdio
        // server can keep its pipes open and starve aichat's own stdio loop
        // when running as `aichat --mcp`.
        cmd.kill_on_drop(true);
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

        let handshake = ().serve(transport);
        let client = if startup_timeout > 0 {
            tokio::time::timeout(
                std::time::Duration::from_secs(startup_timeout),
                handshake,
            )
            .await
            .map_err(|_| {
                anyhow!(
                    "MCP server \"{command}\" startup timed out after {startup_timeout}s\n\
                     hint: increase mcp_startup_timeout or check the server"
                )
            })?
        } else {
            handshake.await
        }
        .map_err(|e| {
            anyhow!(
                "MCP server \"{command}\" did not complete initialization: {e}\n\
                 hint: the server may require additional configuration"
            )
        })?;

        let tools = client.list_all_tools().await.map_err(|e| {
            anyhow!("Failed to list tools from MCP server \"{command}\": {e}")
        })?;

        Ok(Self {
            client,
            tools,
            last_used: std::sync::atomic::AtomicI64::new(chrono::Utc::now().timestamp()),
        })
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

        Ok(Self {
            client,
            tools,
            last_used: std::sync::atomic::AtomicI64::new(chrono::Utc::now().timestamp()),
        })
    }

    fn touch(&self) {
        self.last_used.store(
            chrono::Utc::now().timestamp(),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    fn idle_seconds(&self) -> i64 {
        chrono::Utc::now().timestamp()
            - self.last_used.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult> {
        let mut params = CallToolRequestParams::new(tool_name.to_string());
        if let Some(arguments) = arguments {
            params = params.with_arguments(arguments);
        }
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
    pub startup_timeout: u64,
    pub max_connections: usize,
}

const IDLE_TIMEOUT_SECS: i64 = 300; // 5 minutes

impl std::fmt::Debug for McpConnectionPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpConnectionPool")
            .field("servers", &self.configs.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl McpConnectionPool {
    pub fn new(
        configs: IndexMap<String, McpServerConfig>,
        startup_timeout: u64,
        max_connections: usize,
    ) -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            configs,
            startup_timeout,
            max_connections,
        }
    }

    /// Get or lazily create a connection to a named MCP server.
    /// Evicts idle connections (>5 min) and enforces max_connections limit.
    async fn get_or_connect(&self, name: &str) -> Result<()> {
        // Check for existing non-stale connection
        {
            let conns = self.connections.read().await;
            if let Some(conn) = conns.get(name) {
                if conn.idle_seconds() < IDLE_TIMEOUT_SECS {
                    conn.touch();
                    return Ok(());
                }
            }
        }

        // Remove stale connection if present
        {
            let mut conns = self.connections.write().await;
            if let Some(conn) = conns.get(name) {
                if conn.idle_seconds() < IDLE_TIMEOUT_SECS {
                    conn.touch();
                    return Ok(());
                }
            }
            if let Some(old) = conns.remove(name) {
                let _ = old.shutdown().await;
            }
        }

        // Check max connections limit before creating new
        {
            let conns = self.connections.read().await;
            if conns.len() >= self.max_connections {
                bail!(
                    "MCP connection limit reached ({}/{}). Close unused servers or increase mcp_max_connections.",
                    conns.len(),
                    self.max_connections,
                );
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
            McpConnection::connect(&server_config.command, &server_config.args, envs, self.startup_timeout).await?
        };

        let mut conns = self.connections.write().await;
        conns.insert(name.to_string(), conn);
        Ok(())
    }

    /// Return tool declarations from all configured servers, connecting concurrently.
    ///
    /// Phase 31B: connect each server in its own task so a single hung/slow
    /// server cannot block the others, and per-server failures are isolated
    /// rather than fail-fast. A misbehaving stdio child (e.g. one that drops
    /// the initialize handshake or never responds to `tools/list`) used to
    /// abort the whole loop before reaching later entries — the reported
    /// "5-server subset returns 0 registered tools" symptom in
    /// `docs/architecture/integrated-architecture/bridge-retirement.md` was
    /// this fail-fast pattern, not a tokio runtime saturation issue.
    pub async fn all_tool_declarations(&self) -> Result<Vec<FunctionDeclaration>> {
        let names: Vec<String> = self.configs.keys().cloned().collect();

        let connect_futures = names.iter().map(|name| {
            let name = name.clone();
            async move {
                let result = self.get_or_connect(&name).await;
                (name, result)
            }
        });
        let results = futures_util::future::join_all(connect_futures).await;

        let mut all_decls = Vec::new();
        let conns = self.connections.read().await;
        for (name, result) in results {
            match result {
                Ok(()) => {
                    if let Some(conn) = conns.get(&name) {
                        for tool in conn.tools() {
                            all_decls.push(mcp_tool_to_declaration(tool, &name));
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "MCP server '{name}' failed to register: {e}\n\
                         hint: this server's tools will not be available; \
                         remove it from mcp_servers or fix the config to silence this warning"
                    );
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
        conn.touch();

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
        timeout: None,
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
    hex::encode(hasher.finalize())
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
// Portable mcp.json loader (Phase 31C)
//
// Reads a Claude-Code-compatible declarations file (`{ "mcpServers": {...} }`)
// from disk, normalizes each entry into McpServerConfig, and merges with the
// inline `mcp_servers:` block from config.yaml. Inline entries win on key
// conflict.
//
// Spec: docs/architecture/integrated-architecture/SPEC-mcp-json-artifact.md
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct PortableMcpFile {
    #[serde(rename = "mcpServers", default)]
    mcp_servers: IndexMap<String, PortableMcpEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableMcpEntry {
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
    /// Hint for ambiguous transports; not used today (we infer from
    /// `command` vs `url`). Accepted to keep the file schema-compatible.
    #[serde(default, rename = "type")]
    _transport_type: Option<String>,
    /// Aichat-specific extensions namespace (`namespace`, `lazyDiscover`).
    /// Reserved for follow-on; ignored today.
    #[serde(default, rename = "x-aichat")]
    _x_aichat: Option<Value>,
}

impl PortableMcpEntry {
    fn into_server_config(self, name: &str) -> Result<McpServerConfig> {
        match (self.command.as_deref(), self.url.as_deref()) {
            (Some(_), Some(_)) => bail!(
                "mcp.json entry '{name}' sets both `command` and `url`; \
                 choose one (stdio vs http/sse)"
            ),
            (None, None) => bail!(
                "mcp.json entry '{name}' must set either `command` (stdio) \
                 or `url` (http/sse)"
            ),
            _ => {}
        }
        Ok(McpServerConfig {
            command: self.command.unwrap_or_default(),
            args: self.args.unwrap_or_default(),
            env: self.env.unwrap_or_default(),
            endpoint: self.url,
            headers: self.headers.unwrap_or_default(),
        })
    }
}

/// Resolve the path of the portable `mcp.json`. First hit wins.
///
/// 1. `explicit` (from `mcp_servers_file:` in config.yaml; expanded for `~`).
/// 2. `./mcp.json` in the current working directory.
/// 3. `$XDG_CONFIG_HOME/mcp/mcp.json` (or `~/.config/mcp/mcp.json` if unset).
///
/// Returns `Some(path)` only if the resolved file exists. An explicit path
/// that does not exist is **not** silently ignored — the loader bails so a
/// typo in `config.yaml` is caught at startup.
pub fn resolve_mcp_servers_file(explicit: Option<&str>) -> Result<Option<PathBuf>> {
    if let Some(raw) = explicit {
        let path = expand_tilde(raw);
        if !path.exists() {
            bail!(
                "mcp_servers_file points to '{}' but no file exists there",
                path.display()
            );
        }
        return Ok(Some(path));
    }

    let cwd_candidate = PathBuf::from("./mcp.json");
    if cwd_candidate.exists() {
        return Ok(Some(cwd_candidate));
    }

    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")));
    if let Some(base) = xdg {
        let candidate = base.join("mcp").join("mcp.json");
        if candidate.exists() {
            return Ok(Some(candidate));
        }
    }

    Ok(None)
}

fn expand_tilde(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(raw)
}

/// Parse a portable `mcp.json` file at `path` and return server entries
/// keyed by name. Each entry is converted to the internal `McpServerConfig`
/// representation; `${VAR}` interpolation in `env`/`headers` happens at
/// connection time (see `resolve_env_vars`).
pub fn load_mcp_servers_file(path: &Path) -> Result<IndexMap<String, McpServerConfig>> {
    let content = std::fs::read_to_string(path).with_context(|| {
        format!("failed to read mcp servers file at {}", path.display())
    })?;
    let parsed: PortableMcpFile = serde_json::from_str(&content).with_context(|| {
        format!(
            "failed to parse mcp servers file at {} as JSON \
             (expected `{{ \"mcpServers\": {{ ... }} }}`)",
            path.display()
        )
    })?;

    let mut out = IndexMap::with_capacity(parsed.mcp_servers.len());
    for (name, entry) in parsed.mcp_servers {
        let server = entry.into_server_config(&name)?;
        out.insert(name, server);
    }
    Ok(out)
}

/// Merge file-loaded entries into the inline `mcp_servers:` map. Inline wins
/// on key conflict — the spec treats `config.yaml` as a test/override
/// surface, not the canonical home.
pub fn merge_mcp_servers(
    inline: &mut IndexMap<String, McpServerConfig>,
    file_loaded: IndexMap<String, McpServerConfig>,
) {
    for (name, cfg) in file_loaded {
        if !inline.contains_key(&name) {
            inline.insert(name, cfg);
        }
    }
}

/// Phase 31E: implementation of `aichat --validate-mcp-config [PATH]`.
/// Returns the process exit code (0 = valid, non-zero with diagnostic).
///
/// `path_arg` is `Some("path")` for an explicit path, `Some("")` is treated
/// the same as `None` (uses search order), and `None` means the flag was
/// not supplied (caller shouldn't reach here).
pub fn run_validate_mcp_config(
    path_arg: Option<&str>,
    output_format: Option<OutputFormat>,
) -> i32 {
    let json_output = matches!(output_format, Some(OutputFormat::Json));
    let explicit = path_arg.filter(|s| !s.is_empty());

    let resolved = match resolve_mcp_servers_file(explicit) {
        Ok(Some(p)) => p,
        Ok(None) => {
            emit_validation_error(
                json_output,
                None,
                "no mcp.json found",
                "set mcp_servers_file:, place ./mcp.json in CWD, \
                 or create ~/.config/mcp/mcp.json",
            );
            return 2;
        }
        Err(e) => {
            emit_validation_error(json_output, None, &e.to_string(), "");
            return 2;
        }
    };

    let entries = match load_mcp_servers_file(&resolved) {
        Ok(entries) => entries,
        Err(e) => {
            emit_validation_error(
                json_output,
                Some(&resolved),
                &e.to_string(),
                "",
            );
            return 1;
        }
    };

    let (stdio, http) = entries.values().fold((0u32, 0u32), |(s, h), cfg| {
        if cfg.endpoint.is_some() {
            (s, h + 1)
        } else {
            (s + 1, h)
        }
    });

    if json_output {
        let payload = json!({
            "valid": true,
            "path": resolved.to_string_lossy(),
            "servers": entries.len(),
            "stdio": stdio,
            "http": http,
            "names": entries.keys().collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    } else {
        println!("ok: {}", resolved.display());
        println!(
            "  {} servers ({} stdio, {} http/sse)",
            entries.len(),
            stdio,
            http,
        );
        for (name, cfg) in &entries {
            let kind = if cfg.endpoint.is_some() { "http" } else { "stdio" };
            println!("    [{kind}] {name}");
        }
    }
    0
}

fn emit_validation_error(
    json: bool,
    path: Option<&Path>,
    message: &str,
    hint: &str,
) {
    if json {
        let payload = json!({
            "valid": false,
            "path": path.map(|p| p.to_string_lossy().into_owned()),
            "error": message,
            "hint": if hint.is_empty() { Value::Null } else { Value::String(hint.to_string()) },
        });
        eprintln!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    } else {
        if let Some(p) = path {
            eprintln!("error: {}: {}", p.display(), message);
        } else {
            eprintln!("error: {message}");
        }
        if !hint.is_empty() {
            eprintln!("hint: {hint}");
        }
    }
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
        McpConnection::connect(server_cmd, &[], Default::default(), 30).await
    }
}

async fn cmd_list_tools(cli: &Cli, server_cmd: &str) -> Result<()> {
    // Check cache first (skip if --refresh)
    if !cli.refresh {
        if let Some(cached) = read_cache(server_cmd) {
            let output = format_tools_output(&cached, cli.output_format);
            println!("{output}");
            return Ok(());
        }
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
    // Start from --json if provided, or empty object
    let mut map: Map<String, Value> = match &cli.call_json {
        Some(json_str) => {
            let val: Value = serde_json::from_str(json_str)
                .with_context(|| format!("Invalid JSON in --json: {json_str}"))?;
            match val {
                Value::Object(m) => m,
                _ => bail!("--json must be a JSON object, got: {val}"),
            }
        }
        None => Map::new(),
    };

    // Merge --arg KEY=VALUE pairs (overrides --json per Rule 5)
    let mut array_keys: HashMap<String, Vec<Value>> = HashMap::new();
    for arg in &cli.call_args {
        let (key, val_str) = arg
            .split_once('=')
            .ok_or_else(|| anyhow!("Invalid --arg format '{arg}': expected KEY=VALUE"))?;
        let value = parse_scalar_value(val_str);
        array_keys.entry(key.to_string()).or_default().push(value);
    }

    for (key, values) in array_keys {
        if values.len() == 1 {
            map.insert(key, values.into_iter().next().unwrap());
        } else {
            // Repeated key -> JSON array (Rule 2)
            map.insert(key, Value::Array(values));
        }
    }

    if map.is_empty() {
        Ok(None)
    } else {
        Ok(Some(map))
    }
}

/// Parse a string value into a JSON scalar, attempting number/bool detection.
fn parse_scalar_value(s: &str) -> Value {
    if s == "true" {
        return Value::Bool(true);
    }
    if s == "false" {
        return Value::Bool(false);
    }
    if s == "null" {
        return Value::Null;
    }
    if let Ok(n) = s.parse::<i64>() {
        return Value::Number(n.into());
    }
    if let Ok(n) = s.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(n) {
            return Value::Number(n);
        }
    }
    Value::String(s.to_string())
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serde_json::json;

    // ---- Feature 2: parse_scalar_value ----

    #[test]
    fn test_parse_scalar_value_string() {
        assert_eq!(parse_scalar_value("hello"), Value::String("hello".into()));
    }

    #[test]
    fn test_parse_scalar_value_integer() {
        assert_eq!(parse_scalar_value("42"), json!(42));
    }

    #[test]
    fn test_parse_scalar_value_negative_integer() {
        assert_eq!(parse_scalar_value("-7"), json!(-7));
    }

    #[test]
    fn test_parse_scalar_value_float() {
        assert_eq!(parse_scalar_value("3.14"), json!(3.14));
    }

    #[test]
    fn test_parse_scalar_value_bool_true() {
        assert_eq!(parse_scalar_value("true"), Value::Bool(true));
    }

    #[test]
    fn test_parse_scalar_value_bool_false() {
        assert_eq!(parse_scalar_value("false"), Value::Bool(false));
    }

    #[test]
    fn test_parse_scalar_value_null() {
        assert_eq!(parse_scalar_value("null"), Value::Null);
    }

    #[test]
    fn test_parse_scalar_value_numeric_string() {
        // A string that looks like a number but has extra chars stays a string
        assert_eq!(
            parse_scalar_value("42abc"),
            Value::String("42abc".into())
        );
    }

    // ---- Feature 2: parse_call_arguments with --arg ----

    fn make_cli_with_args(call_json: Option<&str>, call_args: Vec<&str>) -> Cli {
        let mut cli = Cli::parse_from(["aichat", "--mcp-server", "test", "--call", "tool"]);
        cli.call_json = call_json.map(String::from);
        cli.call_args = call_args.into_iter().map(String::from).collect();
        cli
    }

    #[test]
    fn test_parse_args_single_kv() {
        let cli = make_cli_with_args(None, vec!["path=/tmp/file.txt"]);
        let result = parse_call_arguments(&cli).unwrap();
        assert_eq!(result, Some(serde_json::from_str::<Map<String, Value>>(
            r#"{"path": "/tmp/file.txt"}"#
        ).unwrap().into()));
    }

    #[test]
    fn test_parse_args_multiple_kv() {
        let cli = make_cli_with_args(None, vec!["title=Bug", "priority=3"]);
        let result = parse_call_arguments(&cli).unwrap().unwrap();
        assert_eq!(result["title"], json!("Bug"));
        assert_eq!(result["priority"], json!(3));
    }

    #[test]
    fn test_parse_args_repeated_key_becomes_array() {
        let cli = make_cli_with_args(None, vec!["label=bug", "label=urgent"]);
        let result = parse_call_arguments(&cli).unwrap().unwrap();
        assert_eq!(result["label"], json!(["bug", "urgent"]));
    }

    #[test]
    fn test_parse_args_json_only() {
        let cli = make_cli_with_args(Some(r#"{"title": "Bug"}"#), vec![]);
        let result = parse_call_arguments(&cli).unwrap().unwrap();
        assert_eq!(result["title"], json!("Bug"));
    }

    #[test]
    fn test_parse_args_hybrid_merge() {
        let cli = make_cli_with_args(Some(r#"{"body": "Details"}"#), vec!["title=Bug"]);
        let result = parse_call_arguments(&cli).unwrap().unwrap();
        assert_eq!(result["title"], json!("Bug"));
        assert_eq!(result["body"], json!("Details"));
    }

    #[test]
    fn test_parse_args_override_json_with_arg() {
        // Rule 5: --arg overrides --json
        let cli = make_cli_with_args(Some(r#"{"title": "Old"}"#), vec!["title=New"]);
        let result = parse_call_arguments(&cli).unwrap().unwrap();
        assert_eq!(result["title"], json!("New"));
    }

    #[test]
    fn test_parse_args_invalid_format() {
        let cli = make_cli_with_args(None, vec!["noequals"]);
        assert!(parse_call_arguments(&cli).is_err());
    }

    #[test]
    fn test_parse_args_empty() {
        let cli = make_cli_with_args(None, vec![]);
        let result = parse_call_arguments(&cli).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_args_value_with_equals() {
        // KEY=VALUE where VALUE itself contains =
        let cli = make_cli_with_args(None, vec!["query=a=b"]);
        let result = parse_call_arguments(&cli).unwrap().unwrap();
        assert_eq!(result["query"], json!("a=b"));
    }

    // ---- Feature 1: --refresh flag existence ----

    #[test]
    fn test_cli_refresh_flag_defaults_false() {
        let cli = Cli::parse_from(["aichat", "--mcp-server", "test", "--list-tools"]);
        assert!(!cli.refresh);
    }

    #[test]
    fn test_cli_refresh_flag_set() {
        let cli = Cli::parse_from([
            "aichat",
            "--mcp-server",
            "test",
            "--list-tools",
            "--refresh",
        ]);
        assert!(cli.refresh);
    }

    // ---- Feature 2: --arg flag existence ----

    #[test]
    fn test_cli_arg_flag() {
        let cli = Cli::parse_from([
            "aichat",
            "--mcp-server",
            "test",
            "--call",
            "tool",
            "--arg",
            "key=val",
        ]);
        assert_eq!(cli.call_args, vec!["key=val"]);
    }

    #[test]
    fn test_cli_arg_multiple() {
        let cli = Cli::parse_from([
            "aichat",
            "--mcp-server",
            "test",
            "--call",
            "tool",
            "--arg",
            "a=1",
            "--arg",
            "b=2",
        ]);
        assert_eq!(cli.call_args, vec!["a=1", "b=2"]);
    }

    // ---- Feature 3: Config fields ----

    #[test]
    fn test_config_mcp_defaults() {
        let config = crate::config::Config::default();
        assert_eq!(config.mcp_cache_ttl, 3600);
        assert_eq!(config.mcp_startup_timeout, 30);
        assert_eq!(config.mcp_call_timeout, 120);
        assert_eq!(config.mcp_max_connections, 10);
    }

    // ---- Feature 4: McpConnection idle tracking ----

    #[test]
    fn test_mcp_connection_pool_new_with_params() {
        let pool = McpConnectionPool::new(
            IndexMap::new(),
            30,  // startup_timeout
            10,  // max_connections
        );
        assert_eq!(pool.startup_timeout, 30);
        assert_eq!(pool.max_connections, 10);
    }

    // ---- Feature 5: Max connections ----

    #[test]
    fn test_pool_max_connections_stored() {
        let pool = McpConnectionPool::new(IndexMap::new(), 30, 5);
        assert_eq!(pool.max_connections, 5);
    }

    // ---- Existing: resolve_env_vars ----

    #[test]
    fn test_resolve_env_vars_literal() {
        let mut env = HashMap::new();
        env.insert("KEY".into(), "literal_value".into());
        let resolved = resolve_env_vars(&env);
        assert_eq!(resolved["KEY"], "literal_value");
    }

    #[test]
    fn test_resolve_env_vars_expansion() {
        std::env::set_var("TEST_MCP_TOKEN_12345", "secret");
        let mut env = HashMap::new();
        env.insert("TOKEN".into(), "${TEST_MCP_TOKEN_12345}".into());
        let resolved = resolve_env_vars(&env);
        assert_eq!(resolved["TOKEN"], "secret");
        std::env::remove_var("TEST_MCP_TOKEN_12345");
    }

    // ---- Schema cache helpers ----

    #[test]
    fn test_cache_key_deterministic() {
        let k1 = cache_key("npx server-filesystem /tmp");
        let k2 = cache_key("npx server-filesystem /tmp");
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_cache_key_differs_for_different_commands() {
        let k1 = cache_key("npx server-a");
        let k2 = cache_key("npx server-b");
        assert_ne!(k1, k2);
    }

    // ---- Phase 31C: portable mcp.json loader ----

    fn write_tmp_json(content: &str) -> PathBuf {
        // Phase 22E: key the temp dir on (pid, monotonic counter), not the
        // wall clock. A timestamp collides when two parallel test threads run
        // in the same clock tick; an atomic counter is unique by construction,
        // so no two calls — in any thread of this process — ever share a dir.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "aichat-mcp-test-{}-{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mcp.json");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_load_mcp_servers_file_stdio_entry() {
        let path = write_tmp_json(
            r#"{
                "mcpServers": {
                    "git": {
                        "command": "/usr/local/bin/uvx",
                        "args": ["mcp-server-git"]
                    }
                }
            }"#,
        );
        let entries = load_mcp_servers_file(&path).unwrap();
        assert_eq!(entries.len(), 1);
        let git = &entries["git"];
        assert_eq!(git.command, "/usr/local/bin/uvx");
        assert_eq!(git.args, vec!["mcp-server-git"]);
        assert!(git.endpoint.is_none());
    }

    #[test]
    fn test_load_mcp_servers_file_remote_entry() {
        let path = write_tmp_json(
            r#"{
                "mcpServers": {
                    "remote": {
                        "url": "https://mcp.example.com/sse",
                        "headers": { "Authorization": "Bearer ${API_TOKEN}" }
                    }
                }
            }"#,
        );
        let entries = load_mcp_servers_file(&path).unwrap();
        let remote = &entries["remote"];
        assert_eq!(
            remote.endpoint.as_deref(),
            Some("https://mcp.example.com/sse")
        );
        assert_eq!(
            remote.headers.get("Authorization").map(String::as_str),
            Some("Bearer ${API_TOKEN}")
        );
    }

    #[test]
    fn test_load_mcp_servers_file_rejects_both_command_and_url() {
        let path = write_tmp_json(
            r#"{ "mcpServers": { "x": { "command": "a", "url": "b" } } }"#,
        );
        let err = load_mcp_servers_file(&path).unwrap_err();
        assert!(format!("{err}").contains("both `command` and `url`"));
    }

    #[test]
    fn test_load_mcp_servers_file_rejects_neither_command_nor_url() {
        let path = write_tmp_json(r#"{ "mcpServers": { "x": {} } }"#);
        let err = load_mcp_servers_file(&path).unwrap_err();
        assert!(format!("{err}").contains("either `command`"));
    }

    #[test]
    fn test_load_mcp_servers_file_ignores_x_aichat_extension() {
        let path = write_tmp_json(
            r#"{
                "mcpServers": {
                    "git": {
                        "command": "uvx",
                        "args": ["mcp-server-git"],
                        "x-aichat": { "namespace": "g", "lazyDiscover": true }
                    }
                }
            }"#,
        );
        let entries = load_mcp_servers_file(&path).unwrap();
        assert_eq!(entries["git"].command, "uvx");
    }

    #[test]
    fn test_load_mcp_servers_file_invalid_json() {
        let path = write_tmp_json("{ not json");
        assert!(load_mcp_servers_file(&path).is_err());
    }

    // Phase 22E: `write_tmp_json` must hand back a unique path on every call,
    // even under heavy parallel contention. The original timestamp-keyed dir
    // name collided when two test threads landed in the same clock tick — one
    // clobbered the other's `mcp.json`, so a test that wrote `{}` could read a
    // sibling's `{ "command": .. , "url": .. }` and assert against the wrong
    // error string. That is the root cause of the historically flaky
    // `test_load_mcp_servers_file_rejects_neither_command_nor_url`.
    #[test]
    fn write_tmp_json_is_collision_free_across_threads() {
        use std::collections::HashSet;
        use std::sync::{Arc, Mutex};
        let seen = Arc::new(Mutex::new(HashSet::new()));
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let seen = Arc::clone(&seen);
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        let p = write_tmp_json("{}");
                        let mut s = seen.lock().unwrap();
                        assert!(
                            s.insert(p.clone()),
                            "duplicate tmp path handed out: {}",
                            p.display()
                        );
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_merge_mcp_servers_inline_wins() {
        let mut inline = IndexMap::new();
        inline.insert(
            "git".to_string(),
            McpServerConfig {
                command: "inline-cmd".to_string(),
                ..Default::default()
            },
        );

        let mut from_file = IndexMap::new();
        from_file.insert(
            "git".to_string(),
            McpServerConfig {
                command: "file-cmd".to_string(),
                ..Default::default()
            },
        );
        from_file.insert(
            "fetch".to_string(),
            McpServerConfig {
                command: "uvx".to_string(),
                ..Default::default()
            },
        );

        merge_mcp_servers(&mut inline, from_file);
        assert_eq!(inline.len(), 2);
        assert_eq!(inline["git"].command, "inline-cmd");
        assert_eq!(inline["fetch"].command, "uvx");
    }

    #[test]
    fn test_resolve_explicit_path_missing_bails() {
        let err = resolve_mcp_servers_file(Some("/no/such/file.json"))
            .unwrap_err();
        assert!(format!("{err}").contains("no file exists"));
    }

    #[test]
    fn test_resolve_explicit_path_present() {
        let path = write_tmp_json(r#"{ "mcpServers": {} }"#);
        let resolved =
            resolve_mcp_servers_file(Some(path.to_str().unwrap())).unwrap();
        assert_eq!(resolved.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn test_expand_tilde_with_home() {
        if let Some(home) = dirs::home_dir() {
            let expanded = expand_tilde("~/foo/bar.json");
            assert_eq!(expanded, home.join("foo/bar.json"));
        }
    }

    #[test]
    fn test_expand_tilde_passthrough() {
        let expanded = expand_tilde("/abs/path");
        assert_eq!(expanded, PathBuf::from("/abs/path"));
    }
}
