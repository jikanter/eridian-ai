use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use is_terminal::IsTerminal;
use std::io::{stdin, Read};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Raw JSON (validated)
    Json,
    /// One JSON object per line
    Jsonl,
    /// Tab-separated values
    Tsv,
    /// Comma-separated values
    Csv,
    /// Plain text (default behavior, explicit)
    Text,
    /// Compact output (minimal tokens, for agent consumption)
    Compact,
}

impl OutputFormat {
    pub fn system_prompt_suffix(&self) -> Option<&'static str> {
        match self {
            OutputFormat::Json => Some(
                "\n\nYou MUST respond with valid JSON only. No markdown, no code fences, no explanation — just the raw JSON object or array. Do not include any text outside the JSON."
            ),
            OutputFormat::Jsonl => Some(
                "\n\nYou MUST respond with JSON Lines (one valid JSON object per line). No markdown, no code fences, no explanation. Each line must be a complete, valid JSON object."
            ),
            OutputFormat::Tsv => Some(
                "\n\nYou MUST respond with tab-separated values only. No headers, no markdown, no code fences, no explanation. Each row on its own line, fields separated by tab characters."
            ),
            OutputFormat::Csv => Some(
                "\n\nYou MUST respond with comma-separated values only. No headers, no markdown, no code fences, no explanation. Each row on its own line. Quote fields that contain commas."
            ),
            OutputFormat::Text => None,
            OutputFormat::Compact => Some(
                "\n\nRespond with minimal tokens. Use short keys, abbreviations, and omit optional fields. No formatting, no explanations."
            ),
        }
    }

    pub fn is_structured(&self) -> bool {
        !matches!(self, OutputFormat::Text | OutputFormat::Compact)
    }

    pub fn clean_output(&self, output: &str) -> Result<String> {
        let cleaned = strip_code_fences(output);
        match self {
            OutputFormat::Json => {
                // Validate it's parseable JSON
                serde_json::from_str::<serde_json::Value>(&cleaned)
                    .context("Model output is not valid JSON")?;
                Ok(cleaned)
            }
            OutputFormat::Jsonl => {
                // Validate each non-empty line is valid JSON
                for (i, line) in cleaned.lines().enumerate() {
                    if !line.trim().is_empty() {
                        serde_json::from_str::<serde_json::Value>(line)
                            .with_context(|| format!("Line {} is not valid JSON", i + 1))?;
                    }
                }
                Ok(cleaned)
            }
            OutputFormat::Tsv | OutputFormat::Csv | OutputFormat::Text | OutputFormat::Compact => {
                Ok(cleaned)
            }
        }
    }
}

fn strip_code_fences(text: &str) -> String {
    let trimmed = text.trim();
    // Strip ```json ... ``` or ``` ... ``` wrapping
    if let Some(rest) = trimmed.strip_prefix("```") {
        // Skip the optional language tag on the first line
        let rest = match rest.find('\n') {
            Some(pos) => &rest[pos + 1..],
            None => return String::new(),
        };
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    trimmed.to_string()
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Select a LLM model
    #[clap(short, long)]
    pub model: Option<String>,
    /// Use the system prompt
    #[clap(long)]
    pub prompt: Option<String>,
    /// Select a role
    #[clap(short, long)]
    pub role: Option<String>,
    /// Start or join a session
    #[clap(short = 's', long)]
    pub session: Option<Option<String>>,
    /// Ensure the session is empty
    #[clap(long)]
    pub empty_session: bool,
    /// Ensure the new conversation is saved to the session
    #[clap(long)]
    pub save_session: bool,
    /// Start a agent
    #[clap(short = 'a', long)]
    pub agent: Option<String>,
    /// Set agent variables
    #[clap(long, value_names = ["NAME", "VALUE"], num_args = 2)]
    pub agent_variable: Vec<String>,
    /// Set role variable (key=value)
    #[clap(short = 'v', long = "variable", value_name = "KEY=VALUE")]
    pub variable: Vec<String>,
    /// Start a RAG
    #[clap(long)]
    pub rag: Option<String>,
    /// Rebuild the RAG to sync document changes
    #[clap(long)]
    pub rebuild_rag: bool,
    /// Execute a macro
    #[clap(long = "macro", value_name = "MACRO")]
    pub macro_name: Option<String>,
    /// Serve the LLM API and WebAPP
    #[clap(long, value_name = "ADDRESS")]
    pub serve: Option<Option<String>>,
    /// Run as an MCP stdio server
    #[clap(long)]
    pub mcp: bool,
    /// Connect to an external MCP server (stdio transport)
    #[clap(long = "mcp-server", value_name = "COMMAND")]
    pub mcp_server: Option<String>,
    /// List tools from a connected MCP server
    #[clap(long = "list-tools")]
    pub list_tools: bool,
    /// Show schema for a specific MCP tool
    #[clap(long = "tool-info", value_name = "TOOL")]
    pub tool_info: Option<String>,
    /// Call an MCP tool directly
    #[clap(long, value_name = "TOOL")]
    pub call: Option<String>,
    /// JSON arguments for --call
    #[clap(long = "json", value_name = "JSON", requires = "call")]
    pub call_json: Option<String>,
    /// Tool call arguments as KEY=VALUE pairs (repeatable, use with --call)
    #[clap(long = "arg", value_name = "KEY=VALUE", requires = "call")]
    pub call_args: Vec<String>,
    /// Bypass MCP schema cache (force re-fetch from server)
    #[clap(long)]
    pub refresh: bool,
    /// Run a multi-stage pipeline
    #[clap(long)]
    pub pipe: bool,
    /// Pipeline stages (role or role@model)
    #[clap(long = "stage", value_name = "ROLE[@MODEL]", requires = "pipe")]
    pub stages: Vec<String>,
    /// Pipeline definition file
    #[clap(long = "pipe-def", value_name = "FILE", requires = "pipe")]
    pub pipe_def: Option<String>,
    /// Output format (json, jsonl, tsv, csv, text)
    #[clap(short = 'o', long = "output", value_name = "FORMAT")]
    pub output_format: Option<OutputFormat>,
    /// Execute commands in natural language
    #[clap(short = 'e', long)]
    pub execute: bool,
    /// Output code only
    #[clap(short = 'c', long)]
    pub code: bool,
    /// Include files, directories, or URLs
    #[clap(short = 'f', long, value_name = "FILE")]
    pub file: Vec<String>,
    /// Strip <think>...</think> blocks from piped input
    #[clap(long)]
    pub strip_thinking: bool,
    /// Turn off stream mode
    #[clap(short = 'S', long)]
    pub no_stream: bool,
    /// Display cost summary on stderr
    #[clap(long)]
    pub cost: bool,
    /// Display interaction trace on stderr
    #[clap(long)]
    pub trace: bool,
    /// Process stdin line-by-line, one invocation per record
    #[clap(long)]
    pub each: bool,
    /// Number of parallel workers for --each
    #[clap(long, default_value = "1", requires = "each")]
    pub parallel: usize,
    /// Display the message without sending it
    #[clap(long)]
    pub dry_run: bool,
    /// Display information
    #[clap(long)]
    pub info: bool,
    /// Sync models updates
    #[clap(long)]
    pub sync_models: bool,
    /// List all available chat models
    #[clap(long)]
    pub list_models: bool,
    /// List all roles
    #[clap(long)]
    pub list_roles: bool,
    /// List all prompts
    #[clap(long)]
    pub list_prompts: bool,
    /// List all sessions
    #[clap(long)]
    pub list_sessions: bool,
    /// List all agents
    #[clap(long)]
    pub list_agents: bool,
    /// List all RAGs
    #[clap(long)]
    pub list_rags: bool,
    /// List all macros
    #[clap(long)]
    pub list_macros: bool,
    /// Input text
    #[clap(trailing_var_arg = true)]
    text: Vec<String>,
}

impl Cli {
    pub fn text(&self) -> Result<Option<String>> {
        let mut stdin_text = String::new();
        if !self.each && !stdin().is_terminal() {
            let _ = stdin()
                .read_to_string(&mut stdin_text)
                .context("Invalid stdin pipe")?;
        };
        match self.text.is_empty() {
            true => {
                if stdin_text.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(stdin_text))
                }
            }
            false => {
                if self.macro_name.is_some() {
                    let text = self
                        .text
                        .iter()
                        .map(|v| shell_words::quote(v))
                        .collect::<Vec<_>>()
                        .join(" ");
                    if stdin_text.is_empty() {
                        Ok(Some(text))
                    } else {
                        Ok(Some(format!("{text} -- {stdin_text}")))
                    }
                } else {
                    let text = self.text.join(" ");
                    if stdin_text.is_empty() {
                        Ok(Some(text))
                    } else {
                        Ok(Some(format!("{text}\n{stdin_text}")))
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_code_fences_json() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_code_fences(input), r#"{"key": "value"}"#);
    }

    #[test]
    fn test_strip_code_fences_bare() {
        let input = "```\nsome text\n```";
        assert_eq!(strip_code_fences(input), "some text");
    }

    #[test]
    fn test_strip_code_fences_no_fences() {
        let input = r#"{"key": "value"}"#;
        assert_eq!(strip_code_fences(input), input);
    }

    #[test]
    fn test_strip_code_fences_with_whitespace() {
        let input = "  ```json\n{\"a\": 1}\n```  ";
        assert_eq!(strip_code_fences(input), r#"{"a": 1}"#);
    }

    #[test]
    fn test_clean_output_valid_json() {
        let fmt = OutputFormat::Json;
        let result = fmt.clean_output("```json\n{\"a\": 1}\n```");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), r#"{"a": 1}"#);
    }

    #[test]
    fn test_clean_output_invalid_json() {
        let fmt = OutputFormat::Json;
        let result = fmt.clean_output("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_clean_output_jsonl() {
        let fmt = OutputFormat::Jsonl;
        let result = fmt.clean_output("{\"a\":1}\n{\"b\":2}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_clean_output_jsonl_invalid_line() {
        let fmt = OutputFormat::Jsonl;
        let result = fmt.clean_output("{\"a\":1}\nnot json");
        assert!(result.is_err());
    }

    #[test]
    fn test_clean_output_tsv_passthrough() {
        let fmt = OutputFormat::Tsv;
        let result = fmt.clean_output("a\tb\nc\td");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "a\tb\nc\td");
    }

    #[test]
    fn test_is_structured() {
        assert!(OutputFormat::Json.is_structured());
        assert!(OutputFormat::Jsonl.is_structured());
        assert!(OutputFormat::Tsv.is_structured());
        assert!(OutputFormat::Csv.is_structured());
        assert!(!OutputFormat::Text.is_structured());
    }

    #[test]
    fn test_system_prompt_suffix() {
        assert!(OutputFormat::Json.system_prompt_suffix().is_some());
        assert!(OutputFormat::Jsonl.system_prompt_suffix().is_some());
        assert!(OutputFormat::Tsv.system_prompt_suffix().is_some());
        assert!(OutputFormat::Csv.system_prompt_suffix().is_some());
        assert!(OutputFormat::Text.system_prompt_suffix().is_none());
    }
}
