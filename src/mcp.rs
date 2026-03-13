use crate::config::GlobalConfig;
use crate::function::ToolCall;

use anyhow::Result;
use parking_lot::Mutex;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const DISCOVER_ROLES_NAME: &str = "discover_roles";

/// Threshold: if there are fewer than this many tools, skip lazy discovery
/// and serve all tools eagerly (the overhead isn't worth it).
const LAZY_DISCOVERY_THRESHOLD: usize = 8;

// ---------------------------------------------------------------------------
// AichatMcpServer
// ---------------------------------------------------------------------------

pub struct AichatMcpServer {
    config: GlobalConfig,
    /// All tools that could be served (full schemas).
    all_tools: Vec<Tool>,
    /// Currently advertised tools. Starts with just `discover_roles` for lazy mode,
    /// or all tools when the client doesn't support `list_changed` / tool count is small.
    advertised_tools: Mutex<Vec<Tool>>,
    /// Names of tools whose schemas have been injected (lazy mode tracking).
    expanded_tools: Mutex<HashSet<String>>,
    /// Whether we're operating in lazy discovery mode.
    lazy_mode: bool,
}

impl AichatMcpServer {
    pub fn new(config: GlobalConfig) -> Self {
        let all_tools = build_tools(&config);
        let lazy_mode = all_tools.len() >= LAZY_DISCOVERY_THRESHOLD;

        let advertised_tools = if lazy_mode {
            // Start with only the discover_roles meta-tool
            vec![build_discover_roles_tool()]
        } else {
            // Small tool set — serve everything eagerly
            all_tools.clone()
        };

        Self {
            config,
            all_tools,
            advertised_tools: Mutex::new(advertised_tools),
            expanded_tools: Mutex::new(HashSet::new()),
            lazy_mode,
        }
    }

    /// Handle the discover_roles meta-tool call. Returns a compact index of
    /// available tools grouped by category.
    fn handle_discover_roles(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Available tools ({} total):\n",
            self.all_tools.len()
        ));
        for tool in &self.all_tools {
            let name = tool.name.as_ref();
            let desc = tool
                .description
                .as_ref()
                .map(|d| d.to_string())
                .unwrap_or_default();
            // Truncate description to first line for compact index
            let short_desc = desc.lines().next().unwrap_or("");
            lines.push(format!("- {name}: {short_desc}"));
        }
        lines.push(String::new());
        lines.push(
            "Call any tool by name. Schemas will be provided automatically.".to_string(),
        );
        lines.join("\n")
    }

    /// Expand a tool: add its full schema to the advertised set if not already present.
    /// Returns true if the advertised set was modified (i.e. list_changed should fire).
    fn expand_tool(&self, tool_name: &str) -> bool {
        if !self.lazy_mode {
            return false;
        }

        let mut expanded = self.expanded_tools.lock();
        if expanded.contains(tool_name) {
            return false;
        }

        // Find the full tool schema
        let tool = self
            .all_tools
            .iter()
            .find(|t| t.name.as_ref() == tool_name);
        let Some(tool) = tool else {
            return false;
        };

        expanded.insert(tool_name.to_string());
        let mut advertised = self.advertised_tools.lock();
        advertised.push(tool.clone());
        true
    }
}

// ---------------------------------------------------------------------------
// ServerHandler implementation
// ---------------------------------------------------------------------------

impl ServerHandler for AichatMcpServer {
    fn get_info(&self) -> ServerInfo {
        let capabilities = if self.lazy_mode {
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build()
        } else {
            ServerCapabilities::builder().enable_tools().build()
        };

        ServerInfo {
            protocol_version: Default::default(),
            capabilities,
            server_info: Implementation {
                name: "aichat".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            instructions: Some(
                "aichat MCP server — exposes llm-functions tools via the Model Context Protocol. \
                 Call discover_roles to see available tools."
                    .into(),
            ),
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        let tools = self.advertised_tools.lock().clone();
        std::future::ready(Ok(ListToolsResult::with_all_items(tools)))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, ErrorData>> + Send + '_ {
        let config = self.config.clone();
        async move {
            let tool_name = request.name.to_string();

            // Handle discover_roles meta-tool
            if tool_name == DISCOVER_ROLES_NAME {
                let index = self.handle_discover_roles();
                return Ok(CallToolResult::success(vec![Content::text(index)]));
            }

            // Lazy expansion: inject the tool's schema and notify list_changed
            if self.expand_tool(&tool_name) {
                if let Err(e) = context.peer.notify_tool_list_changed().await {
                    // Non-fatal: client may not support list_changed
                    log::debug!("Failed to send tools/list_changed notification: {e}");
                }
            }

            // Execute the actual tool
            let arguments: Value = match request.arguments {
                Some(map) => Value::Object(map),
                None => json!({}),
            };

            let call = ToolCall::new(tool_name.clone(), arguments, None);

            let result = tokio::task::spawn_blocking(move || call.eval(&config))
                .await
                .map_err(|e| {
                    ErrorData::internal_error(format!("Task join error: {e}"), None)
                })?
                .map_err(|e| {
                    ErrorData::internal_error(
                        format!("Tool '{tool_name}' failed: {e}"),
                        None,
                    )
                })?;

            let text = match result {
                Value::Null => "DONE".to_string(),
                Value::String(s) => s,
                other => serde_json::to_string_pretty(&other).unwrap_or_default(),
            };

            Ok(CallToolResult::success(vec![Content::text(text)]))
        }
    }
}

// ---------------------------------------------------------------------------
// Tool builders
// ---------------------------------------------------------------------------

fn build_discover_roles_tool() -> Tool {
    let schema: Map<String, Value> = serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Optional keyword to filter tools by name or description."
            }
        }
    }))
    .unwrap_or_default();

    Tool::new(
        DISCOVER_ROLES_NAME.to_string(),
        "List all available tools with their descriptions. Call this first to discover \
         what tools are available, then call tools by name."
            .to_string(),
        Arc::new(schema),
    )
}

fn build_tools(config: &GlobalConfig) -> Vec<Tool> {
    let config = config.read();
    config
        .functions
        .declarations()
        .iter()
        .filter_map(|decl| {
            let schema = serde_json::to_value(&decl.parameters).ok()?;
            let input_schema = match schema {
                Value::Object(map) => map,
                _ => Map::new(),
            };
            let mut description = decl.description.clone();
            // Append examples if present
            if let Some(examples) = &decl.examples {
                if !examples.is_empty() {
                    description.push_str("\n\nExamples:");
                    for ex in examples {
                        description.push_str(&format!("\n- \"{}\"", ex.input));
                        if let Some(args) = &ex.args {
                            description.push_str(&format!(
                                " -> {}",
                                serde_json::to_string(args).unwrap_or_default()
                            ));
                        }
                    }
                }
            }
            Some(Tool::new(decl.name.clone(), description, Arc::new(input_schema)))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn run(config: GlobalConfig) -> Result<()> {
    let server = AichatMcpServer::new(config);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
