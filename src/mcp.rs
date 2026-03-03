use crate::config::GlobalConfig;
use crate::function::ToolCall;

use anyhow::Result;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Map, Value};
use std::future::Future;
use std::sync::Arc;

pub struct AichatMcpServer {
    config: GlobalConfig,
    tools: Vec<Tool>,
}

impl AichatMcpServer {
    pub fn new(config: GlobalConfig) -> Self {
        let tools = build_tools(&config);
        Self { config, tools }
    }
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
            Some(Tool::new(
                decl.name.clone(),
                decl.description.clone(),
                Arc::new(input_schema),
            ))
        })
        .collect()
}

impl ServerHandler for AichatMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "aichat".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            instructions: Some(
                "aichat MCP server — exposes llm-functions tools via the Model Context Protocol"
                    .into(),
            ),
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(self.tools.clone())))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, ErrorData>> + Send + '_ {
        let config = self.config.clone();
        async move {
            let tool_name = request.name.to_string();
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

pub async fn run(config: GlobalConfig) -> Result<()> {
    let server = AichatMcpServer::new(config);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
