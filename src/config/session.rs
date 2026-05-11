use super::input::*;
use super::*;

use crate::client::{Message, MessageContent, MessageContentPart, MessageRole};
use crate::render::MarkdownRender;

use anyhow::{bail, Context, Result};
use fancy_regex::Regex;
use inquire::{validator::Validation, Confirm, Text};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs::{read_to_string, write};
use std::path::Path;
use std::sync::LazyLock;

static RE_AUTONAME_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{8}T\d{6}-").unwrap());

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Session {
    #[serde(rename(serialize = "model", deserialize = "model"))]
    model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    use_tools: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    save_session: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compress_threshold: Option<usize>,

    #[serde(skip_serializing_if = "Option::is_none")]
    role_name: Option<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    agent_variables: AgentVariables,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    agent_instructions: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    compressed_messages: Vec<Message>,
    messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    data_urls: HashMap<String, String>,

    #[serde(skip)]
    model: Model,
    #[serde(skip)]
    role_prompt: String,
    #[serde(skip)]
    name: String,
    #[serde(skip)]
    path: Option<String>,
    #[serde(skip)]
    dirty: bool,
    #[serde(skip)]
    save_session_this_time: bool,
    #[serde(skip)]
    compressing: bool,
    #[serde(skip)]
    autoname: Option<AutoName>,
    #[serde(skip)]
    tokens: usize,
    #[serde(skip)]
    output_schema: Option<serde_json::Value>,
    #[serde(skip)]
    input_schema: Option<serde_json::Value>,
    #[serde(skip)]
    pipe_to_override: Option<String>,
    #[serde(skip)]
    save_to_override: Option<String>,
}

impl Session {
    pub fn new(config: &Config, name: &str) -> Self {
        let role = config.extract_role();
        let mut session = Self {
            name: name.to_string(),
            save_session: config.save_session,
            ..Default::default()
        };
        session.set_role(role);
        session.dirty = false;
        session
    }

    pub fn load(config: &Config, name: &str, path: &Path) -> Result<Self> {
        let content = read_to_string(path)
            .with_context(|| format!("Failed to load session {} at {}", name, path.display()))?;
        let mut session: Self =
            serde_yaml::from_str(&content).with_context(|| format!("Invalid session {name}"))?;

        session.model = Model::retrieve_model(config, &session.model_id, ModelType::Chat)?;

        if let Some(autoname) = name.strip_prefix("_/") {
            session.name = TEMP_SESSION_NAME.to_string();
            session.path = None;
            if let Ok(true) = RE_AUTONAME_PREFIX.is_match(autoname) {
                session.autoname = Some(AutoName::new(autoname[16..].to_string()));
            }
        } else {
            session.name = name.to_string();
            session.path = Some(path.display().to_string());
        }

        if let Some(role_name) = &session.role_name {
            if let Ok(role) = config.retrieve_role(role_name) {
                session.role_prompt = role.prompt().to_string();
            }
        }

        session.update_tokens();

        Ok(session)
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.compressed_messages.is_empty()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn role_name(&self) -> Option<&str> {
        self.role_name.as_deref()
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn save_session(&self) -> Option<bool> {
        self.save_session
    }

    pub fn tokens(&self) -> usize {
        self.tokens
    }

    pub fn update_tokens(&mut self) {
        self.tokens = self.model().total_tokens(&self.messages);
    }

    pub fn has_user_messages(&self) -> bool {
        self.messages.iter().any(|v| v.role.is_user())
    }

    pub fn user_messages_len(&self) -> usize {
        self.messages.iter().filter(|v| v.role.is_user()).count()
    }

    pub fn export(&self) -> Result<String> {
        let mut data = json!({
            "path": self.path,
            "model": self.model().id(),
        });
        if let Some(temperature) = self.temperature() {
            data["temperature"] = temperature.into();
        }
        if let Some(top_p) = self.top_p() {
            data["top_p"] = top_p.into();
        }
        if let Some(use_tools) = self.use_tools() {
            data["use_tools"] = use_tools.into();
        }
        if let Some(save_session) = self.save_session() {
            data["save_session"] = save_session.into();
        }
        let (tokens, percent) = self.tokens_usage();
        data["total_tokens"] = tokens.into();
        if let Some(max_input_tokens) = self.model().max_input_tokens() {
            data["max_input_tokens"] = max_input_tokens.into();
        }
        if percent != 0.0 {
            data["total/max"] = format!("{percent}%").into();
        }
        data["messages"] = json!(self.messages);

        let output = serde_yaml::to_string(&data)
            .with_context(|| format!("Unable to show info about session '{}'", &self.name))?;
        Ok(output)
    }

    /// Export this session in pi's v3 JSONL tree format so an existing
    /// aichat conversation can be resumed in the pi REPL.
    ///
    /// Layout: one `SessionHeader` line, then one `SessionMessageEntry` per
    /// non-system message. The aichat message list is linear, so each entry
    /// is the child of the previous one. Assistant messages carrying tool
    /// calls split into one assistant entry (text + `toolCall` content
    /// blocks) followed by one `toolResult` entry per call — matching the
    /// shape pi's `buildSessionContext()` consumes.
    ///
    /// System-role messages are dropped: pi resolves the system prompt from
    /// the model + extension config at session start, so reseeding it as a
    /// user-visible turn would double up.
    ///
    /// `cwd` is recorded in the session header so pi can group the file
    /// alongside other sessions for that working directory.
    pub fn export_to_pi_jsonl(
        &self,
        cwd: &std::path::Path,
        out: &mut dyn std::io::Write,
    ) -> Result<()> {
        use std::io::Write;

        // Prefer the serialized `model_id` field over `self.model().id()`:
        // the latter is the runtime resolved `Model` which may not be
        // populated yet (e.g. during conversion outside of a chat).
        let model_id = if !self.model_id.is_empty() {
            self.model_id.clone()
        } else {
            self.model().id()
        };
        let (provider, model_short) = match model_id.split_once(':') {
            Some((p, m)) => (p.to_string(), m.to_string()),
            None => ("unknown".to_string(), model_id.clone()),
        };
        let base = chrono::Utc::now();
        let session_uuid = uuid::Uuid::new_v4().to_string();
        let cwd_str = cwd.to_string_lossy().to_string();

        // Header. `version: 3` is the current pi format; older sessions get
        // migrated by pi on load, so we ship the latest to avoid the loader
        // doing work it doesn't need to do.
        let header = json!({
            "type": "session",
            "version": 3,
            "id": session_uuid,
            "timestamp": iso_ms(&base),
            "cwd": cwd_str,
        });
        writeln!(out, "{header}")?;

        // Per-message synthetic timestamps and ids. Pi requires entry ids
        // to be globally unique within the file and parentId to chain
        // backwards; we increment timestamps by 1ms per emitted entry so
        // ordering is stable across re-imports.
        let mut prev_id: Option<String> = None;
        let mut step: i64 = 0;

        // For the user-facing timestamp inside each AgentMessage body, pi
        // expects Unix ms (a number); reuse our advancing clock.
        let body_ts = |step: i64| (base.timestamp_millis()) + step;

        let walk = |msgs: &[Message],
                    prev_id: &mut Option<String>,
                    step: &mut i64,
                    out: &mut dyn std::io::Write|
         -> Result<()> {
            for m in msgs {
                if matches!(m.role, MessageRole::System) {
                    continue;
                }
                match (&m.role, &m.content) {
                    (MessageRole::User, content) => {
                        *step += 1;
                        let id = short_hex_id();
                        let body = json!({
                            "role": "user",
                            "content": user_content_to_pi(content),
                            "timestamp": body_ts(*step),
                        });
                        let entry = json!({
                            "type": "message",
                            "id": id,
                            "parentId": prev_id.clone().map_or(Value::Null, Value::String),
                            "timestamp": iso_ms(&(base + chrono::Duration::milliseconds(*step))),
                            "message": body,
                        });
                        writeln!(out, "{entry}")?;
                        *prev_id = Some(id);
                    }
                    (MessageRole::Assistant, MessageContent::ToolCalls(calls)) => {
                        *step += 1;
                        let id = short_hex_id();
                        // Assistant content: optional preface text + one
                        // toolCall block per call. Pi expects all toolCall
                        // ids referenced by later toolResult entries to
                        // appear here, so we mint a fresh id when aichat's
                        // ToolCall.id was None.
                        let mut content_blocks: Vec<Value> = Vec::new();
                        if !calls.text.is_empty() {
                            content_blocks
                                .push(json!({"type": "text", "text": calls.text}));
                        }
                        let mut call_ids: Vec<String> = Vec::with_capacity(calls.tool_results.len());
                        for tr in &calls.tool_results {
                            let call_id = tr
                                .call
                                .id
                                .clone()
                                .unwrap_or_else(|| format!("call_{}", short_hex_id()));
                            content_blocks.push(json!({
                                "type": "toolCall",
                                "id": call_id,
                                "name": tr.call.name,
                                "arguments": tr.call.arguments,
                            }));
                            call_ids.push(call_id);
                        }
                        let body = json!({
                            "role": "assistant",
                            "content": content_blocks,
                            "provider": provider,
                            "model": model_short,
                            "usage": pi_zero_usage(),
                            "stopReason": "toolUse",
                            "timestamp": body_ts(*step),
                        });
                        let entry = json!({
                            "type": "message",
                            "id": id,
                            "parentId": prev_id.clone().map_or(Value::Null, Value::String),
                            "timestamp": iso_ms(&(base + chrono::Duration::milliseconds(*step))),
                            "message": body,
                        });
                        writeln!(out, "{entry}")?;
                        *prev_id = Some(id);

                        // Emit each tool result as its own entry. Pi treats
                        // each toolResult as one tree node — even when the
                        // assistant fired tool calls in parallel.
                        for (call_id, tr) in call_ids.iter().zip(calls.tool_results.iter()) {
                            *step += 1;
                            let id = short_hex_id();
                            let output_text = match tr.output.as_str() {
                                Some(s) => s.to_string(),
                                None => tr.output.to_string(),
                            };
                            let body = json!({
                                "role": "toolResult",
                                "toolCallId": call_id,
                                "toolName": tr.call.name,
                                "content": [{"type": "text", "text": output_text}],
                                "isError": false,
                                "timestamp": body_ts(*step),
                            });
                            let entry = json!({
                                "type": "message",
                                "id": id,
                                "parentId": prev_id.clone().map_or(Value::Null, Value::String),
                                "timestamp": iso_ms(&(base + chrono::Duration::milliseconds(*step))),
                                "message": body,
                            });
                            writeln!(out, "{entry}")?;
                            *prev_id = Some(id);
                        }
                    }
                    (MessageRole::Assistant, content) => {
                        *step += 1;
                        let id = short_hex_id();
                        let text = content.to_text();
                        let body = json!({
                            "role": "assistant",
                            "content": [{"type": "text", "text": text}],
                            "provider": provider,
                            "model": model_short,
                            "usage": pi_zero_usage(),
                            "stopReason": "stop",
                            "timestamp": body_ts(*step),
                        });
                        let entry = json!({
                            "type": "message",
                            "id": id,
                            "parentId": prev_id.clone().map_or(Value::Null, Value::String),
                            "timestamp": iso_ms(&(base + chrono::Duration::milliseconds(*step))),
                            "message": body,
                        });
                        writeln!(out, "{entry}")?;
                        *prev_id = Some(id);
                    }
                    (MessageRole::Tool, content) => {
                        // aichat sometimes carries a bare Tool role with
                        // text payload (older sessions); the corresponding
                        // call should have appeared in the prior assistant
                        // turn. Emit a best-effort toolResult; pi will
                        // reject it if the id doesn't match, so we tag it
                        // with a synthetic id.
                        *step += 1;
                        let id = short_hex_id();
                        let body = json!({
                            "role": "toolResult",
                            "toolCallId": format!("orphan_{}", short_hex_id()),
                            "toolName": "unknown",
                            "content": [{"type": "text", "text": content.to_text()}],
                            "isError": false,
                            "timestamp": body_ts(*step),
                        });
                        let entry = json!({
                            "type": "message",
                            "id": id,
                            "parentId": prev_id.clone().map_or(Value::Null, Value::String),
                            "timestamp": iso_ms(&(base + chrono::Duration::milliseconds(*step))),
                            "message": body,
                        });
                        writeln!(out, "{entry}")?;
                        *prev_id = Some(id);
                    }
                    (MessageRole::System, _) => unreachable!("filtered above"),
                }
            }
            Ok(())
        };

        // Compressed history is meaningful conversation just like live
        // messages — pi has no native way to inline an aichat-summarized
        // tail, so we flatten the two lists. A future pass could emit a
        // CompactionEntry between them to preserve the summarization, but
        // for round-tripping a conversation the flat order is faithful.
        walk(&self.compressed_messages, &mut prev_id, &mut step, out)?;
        walk(&self.messages, &mut prev_id, &mut step, out)?;

        Ok(())
    }

    pub fn render(
        &self,
        render: &mut MarkdownRender,
        agent_info: &Option<(String, Vec<String>)>,
    ) -> Result<String> {
        let mut items = vec![];

        if let Some(path) = &self.path {
            items.push(("path", path.to_string()));
        }

        if let Some(autoname) = self.autoname() {
            items.push(("autoname", autoname.to_string()));
        }

        items.push(("model", self.model().id()));

        if let Some(temperature) = self.temperature() {
            items.push(("temperature", temperature.to_string()));
        }
        if let Some(top_p) = self.top_p() {
            items.push(("top_p", top_p.to_string()));
        }

        if let Some(use_tools) = self.use_tools() {
            items.push(("use_tools", use_tools));
        }

        if let Some(save_session) = self.save_session() {
            items.push(("save_session", save_session.to_string()));
        }

        if let Some(compress_threshold) = self.compress_threshold {
            items.push(("compress_threshold", compress_threshold.to_string()));
        }

        if let Some(max_input_tokens) = self.model().max_input_tokens() {
            items.push(("max_input_tokens", max_input_tokens.to_string()));
        }

        let mut lines: Vec<String> = items
            .iter()
            .map(|(name, value)| format!("{name:<20}{value}"))
            .collect();

        lines.push(String::new());

        if !self.is_empty() {
            let resolve_url_fn = |url: &str| resolve_data_url(&self.data_urls, url.to_string());

            for message in &self.messages {
                match message.role {
                    MessageRole::System => {
                        lines.push(
                            render
                                .render(&message.content.render_input(resolve_url_fn, agent_info)),
                        );
                    }
                    MessageRole::Assistant => {
                        if let MessageContent::Text(text) = &message.content {
                            lines.push(render.render(text));
                        }
                        lines.push("".into());
                    }
                    MessageRole::User => {
                        lines.push(format!(
                            ">> {}",
                            message.content.render_input(resolve_url_fn, agent_info)
                        ));
                    }
                    MessageRole::Tool => {
                        lines.push(message.content.render_input(resolve_url_fn, agent_info));
                    }
                }
            }
        }

        Ok(lines.join("\n"))
    }

    pub fn tokens_usage(&self) -> (usize, f32) {
        let tokens = self.tokens();
        let max_input_tokens = self.model().max_input_tokens().unwrap_or_default();
        let percent = if max_input_tokens == 0 {
            0.0
        } else {
            let percent = tokens as f32 / max_input_tokens as f32 * 100.0;
            (percent * 100.0).round() / 100.0
        };
        (tokens, percent)
    }

    pub fn set_role(&mut self, role: Role) {
        self.model_id = role.model().id();
        self.temperature = role.temperature();
        self.top_p = role.top_p();
        self.use_tools = role.use_tools();
        self.model = role.model().clone();
        self.role_name = convert_option_string(role.name());
        self.role_prompt = role.prompt().to_string();
        self.dirty = true;
        self.update_tokens();
    }

    pub fn clear_role(&mut self) {
        self.role_name = None;
        self.role_prompt.clear();
    }

    pub fn sync_agent(&mut self, agent: &Agent) {
        self.role_name = None;
        self.role_prompt = agent.interpolated_instructions();
        self.agent_variables = agent.variables().clone();
        self.agent_instructions = self.role_prompt.clone();
    }

    pub fn agent_variables(&self) -> &AgentVariables {
        &self.agent_variables
    }

    pub fn agent_instructions(&self) -> &str {
        &self.agent_instructions
    }

    pub fn set_save_session(&mut self, value: Option<bool>) {
        if self.save_session != value {
            self.save_session = value;
            self.dirty = true;
        }
    }

    pub fn set_save_session_this_time(&mut self) {
        self.save_session_this_time = true;
    }

    pub fn set_compress_threshold(&mut self, value: Option<usize>) {
        if self.compress_threshold != value {
            self.compress_threshold = value;
            self.dirty = true;
        }
    }

    pub fn set_output_schema(&mut self, value: Option<serde_json::Value>) {
        self.output_schema = value;
    }

    pub fn set_input_schema(&mut self, value: Option<serde_json::Value>) {
        self.input_schema = value;
    }

    pub fn set_pipe_to(&mut self, value: Option<String>) {
        self.pipe_to_override = value;
    }

    pub fn set_save_to(&mut self, value: Option<String>) {
        self.save_to_override = value;
    }

    pub fn need_compress(&self, global_compress_threshold: usize) -> bool {
        if self.compressing {
            return false;
        }
        let threshold = self.compress_threshold.unwrap_or(global_compress_threshold);
        if threshold < 1 {
            return false;
        }
        self.tokens() > threshold
    }

    pub fn compressing(&self) -> bool {
        self.compressing
    }

    pub fn set_compressing(&mut self, compressing: bool) {
        self.compressing = compressing;
    }

    pub fn compress(&mut self, mut prompt: String) {
        if let Some(system_prompt) = self.messages.first().and_then(|v| {
            if MessageRole::System == v.role {
                let content = v.content.to_text();
                if !content.is_empty() {
                    return Some(content);
                }
            }
            None
        }) {
            prompt = format!("{system_prompt}\n\n{prompt}",);
        }
        self.compressed_messages.append(&mut self.messages);
        self.messages.push(Message::new(
            MessageRole::System,
            MessageContent::Text(prompt),
        ));
        self.dirty = true;
        self.update_tokens();
    }

    pub fn need_autoname(&self) -> bool {
        self.autoname.as_ref().map(|v| v.need()).unwrap_or_default()
    }

    pub fn set_autonaming(&mut self, naming: bool) {
        if let Some(v) = self.autoname.as_mut() {
            v.naming = naming;
        }
    }

    pub fn chat_history_for_autonaming(&self) -> Option<String> {
        self.autoname.as_ref().and_then(|v| v.chat_history.clone())
    }

    pub fn autoname(&self) -> Option<&str> {
        self.autoname.as_ref().and_then(|v| v.name.as_deref())
    }

    pub fn set_autoname(&mut self, value: &str) {
        let name = value
            .chars()
            .map(|v| if v.is_alphanumeric() { v } else { '-' })
            .collect();
        self.autoname = Some(AutoName::new(name));
    }

    pub fn exit(&mut self, session_dir: &Path, is_repl: bool) -> Result<()> {
        let mut save_session = self.save_session();
        if self.save_session_this_time {
            save_session = Some(true);
        }
        if self.dirty && save_session != Some(false) {
            let mut session_dir = session_dir.to_path_buf();
            let mut session_name = self.name().to_string();
            if save_session.is_none() {
                if !is_repl {
                    return Ok(());
                }
                let ans = Confirm::new("Save session?").with_default(false).prompt()?;
                if !ans {
                    return Ok(());
                }
                if session_name == TEMP_SESSION_NAME {
                    session_name = Text::new("Session name:")
                        .with_validator(|input: &str| {
                            let input = input.trim();
                            if input.is_empty() {
                                Ok(Validation::Invalid("This name is required".into()))
                            } else if input == TEMP_SESSION_NAME {
                                Ok(Validation::Invalid("This name is reserved".into()))
                            } else {
                                Ok(Validation::Valid)
                            }
                        })
                        .prompt()?;
                }
            } else if save_session == Some(true) && session_name == TEMP_SESSION_NAME {
                session_dir = session_dir.join("_");
                ensure_parent_exists(&session_dir).with_context(|| {
                    format!("Failed to create directory '{}'", session_dir.display())
                })?;

                let now = chrono::Local::now();
                session_name = now.format("%Y%m%dT%H%M%S").to_string();
                if let Some(autoname) = self.autoname() {
                    session_name = format!("{session_name}-{autoname}")
                }
            }
            let session_path = session_dir.join(format!("{session_name}.yaml"));
            self.save(&session_name, &session_path, is_repl)?;
        }
        Ok(())
    }

    pub fn save(&mut self, session_name: &str, session_path: &Path, is_repl: bool) -> Result<()> {
        ensure_parent_exists(session_path)?;

        self.path = Some(session_path.display().to_string());

        let content = serde_yaml::to_string(&self)
            .with_context(|| format!("Failed to serde session '{}'", self.name))?;
        write(session_path, content).with_context(|| {
            format!(
                "Failed to write session '{}' to '{}'",
                self.name,
                session_path.display()
            )
        })?;

        if is_repl {
            println!("✓ Saved the session to '{}'.", session_path.display());
        }

        if self.name() != session_name {
            self.name = session_name.to_string()
        }

        self.dirty = false;

        Ok(())
    }

    pub fn guard_empty(&self) -> Result<()> {
        if !self.is_empty() {
            bail!("Cannot perform this operation because the session has messages, please `.empty session` first.");
        }
        Ok(())
    }

    pub fn add_message(&mut self, input: &Input, output: &str) -> Result<()> {
        if input.continue_output().is_some() {
            if let Some(message) = self.messages.last_mut() {
                if let MessageContent::Text(text) = &mut message.content {
                    *text = format!("{text}{output}");
                }
            }
        } else if input.regenerate() {
            if let Some(message) = self.messages.last_mut() {
                if let MessageContent::Text(text) = &mut message.content {
                    *text = output.to_string();
                }
            }
        } else {
            if self.messages.is_empty() {
                if self.name == TEMP_SESSION_NAME && self.save_session == Some(true) {
                    let raw_input = input.raw();
                    let chat_history = format!("USER: {raw_input}\nASSISTANT: {output}\n");
                    self.autoname = Some(AutoName::new_from_chat_history(chat_history));
                }
                self.messages.extend(input.role().build_messages(input));
            } else {
                self.messages
                    .push(Message::new(MessageRole::User, input.message_content()));
            }
            self.data_urls.extend(input.data_urls());
            if let Some(tool_calls) = input.tool_calls() {
                self.messages.push(Message::new(
                    MessageRole::Tool,
                    MessageContent::ToolCalls(tool_calls.clone()),
                ))
            }
            self.messages.push(Message::new(
                MessageRole::Assistant,
                MessageContent::Text(output.to_string()),
            ));
        }
        self.dirty = true;
        self.update_tokens();
        Ok(())
    }

    pub fn clear_messages(&mut self) {
        self.messages.clear();
        self.compressed_messages.clear();
        self.data_urls.clear();
        self.autoname = None;
        self.dirty = true;
        self.update_tokens();
    }

    pub fn echo_messages(&self, input: &Input) -> String {
        let messages = self.build_messages(input);
        serde_yaml::to_string(&messages).unwrap_or_else(|_| "Unable to echo message".into())
    }

    pub fn build_messages(&self, input: &Input) -> Vec<Message> {
        let mut messages = self.messages.clone();
        if input.continue_output().is_some() {
            return messages;
        } else if input.regenerate() {
            while let Some(last) = messages.last() {
                if !last.role.is_user() {
                    messages.pop();
                } else {
                    break;
                }
            }
            return messages;
        }
        let mut need_add_msg = true;
        let len = messages.len();
        if len == 0 {
            messages = input.role().build_messages(input);
            need_add_msg = false;
        } else if len == 1 && self.compressed_messages.len() >= 2 {
            if let Some(index) = self
                .compressed_messages
                .iter()
                .rposition(|v| v.role == MessageRole::User)
            {
                messages.extend(self.compressed_messages[index..].to_vec());
            }
        }
        if need_add_msg {
            messages.push(Message::new(MessageRole::User, input.message_content()));
        }
        messages
    }
}

impl RoleLike for Session {
    fn to_role(&self) -> Role {
        let role_name = self.role_name.as_deref().unwrap_or_default();
        let mut role = Role::new(role_name, &self.role_prompt);
        role.sync(self);
        if self.output_schema.is_some() {
            role.set_output_schema(self.output_schema.clone());
        }
        if self.input_schema.is_some() {
            role.set_input_schema(self.input_schema.clone());
        }
        if self.pipe_to_override.is_some() {
            role.set_pipe_to(self.pipe_to_override.clone());
        }
        if self.save_to_override.is_some() {
            role.set_save_to(self.save_to_override.clone());
        }
        role
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
        if self.model().id() != model.id() {
            self.model_id = model.id();
            self.model = model;
            self.dirty = true;
            self.update_tokens();
        }
    }

    fn set_temperature(&mut self, value: Option<f64>) {
        if self.temperature != value {
            self.temperature = value;
            self.dirty = true;
        }
    }

    fn set_top_p(&mut self, value: Option<f64>) {
        if self.top_p != value {
            self.top_p = value;
            self.dirty = true;
        }
    }

    fn set_use_tools(&mut self, value: Option<String>) {
        if self.use_tools != value {
            self.use_tools = value;
            self.dirty = true;
        }
    }
}

#[derive(Debug, Clone, Default)]
struct AutoName {
    naming: bool,
    chat_history: Option<String>,
    name: Option<String>,
}

impl AutoName {
    pub fn new(name: String) -> Self {
        Self {
            name: Some(name),
            ..Default::default()
        }
    }
    pub fn new_from_chat_history(chat_history: String) -> Self {
        Self {
            chat_history: Some(chat_history),
            ..Default::default()
        }
    }
    pub fn need(&self) -> bool {
        !self.naming && self.chat_history.is_some() && self.name.is_none()
    }
}

#[cfg(test)]
mod pi_export_tests {
    use super::*;
    use crate::client::Message;
    use crate::function::{ToolCall, ToolResult};
    use serde_json::Value;
    use std::path::PathBuf;

    /// Build a session, push messages, and export to a Vec<u8>. Returns
    /// the JSONL lines parsed as JSON values so each test can assert on
    /// shape without re-parsing.
    fn export(messages: Vec<Message>, compressed: Vec<Message>) -> Vec<Value> {
        let mut session = Session::default();
        session.model_id = "openai:gpt-4o-mini".to_string();
        // The exported `model` short name comes from the configured
        // model's `id()` (Model::id concatenates client:model). For tests
        // we bypass Model::retrieve_model — set the inner model id via
        // the underlying default model_id string parsing in export.
        session.messages = messages;
        session.compressed_messages = compressed;

        let mut buf: Vec<u8> = Vec::new();
        session
            .export_to_pi_jsonl(&PathBuf::from("/tmp/aichat-test"), &mut buf)
            .expect("export should not fail");
        let text = String::from_utf8(buf).unwrap();
        text.lines()
            .map(|l| serde_json::from_str::<Value>(l).expect("each line must be valid JSON"))
            .collect()
    }

    fn user(text: &str) -> Message {
        Message::new(MessageRole::User, MessageContent::Text(text.to_string()))
    }

    fn assistant(text: &str) -> Message {
        Message::new(
            MessageRole::Assistant,
            MessageContent::Text(text.to_string()),
        )
    }

    fn system(text: &str) -> Message {
        Message::new(MessageRole::System, MessageContent::Text(text.to_string()))
    }

    #[test]
    fn header_is_pi_v3_with_cwd() {
        let lines = export(vec![user("hello")], vec![]);
        let header = &lines[0];
        assert_eq!(header["type"], "session");
        assert_eq!(header["version"], 3);
        assert_eq!(header["cwd"], "/tmp/aichat-test");
        assert!(header["id"].as_str().unwrap().len() >= 32);
        // ISO 8601 with milliseconds and Z suffix.
        let ts = header["timestamp"].as_str().unwrap();
        assert!(ts.ends_with('Z'), "timestamp must be UTC: {ts}");
        assert!(ts.contains('.'), "timestamp must have ms: {ts}");
    }

    #[test]
    fn linear_user_assistant_pair_chains_via_parent_id() {
        let lines = export(vec![user("hi"), assistant("hello")], vec![]);
        // [header, user_entry, assistant_entry]
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1]["type"], "message");
        assert_eq!(lines[1]["parentId"], Value::Null);
        assert_eq!(lines[1]["message"]["role"], "user");
        assert_eq!(lines[1]["message"]["content"], "hi");

        assert_eq!(lines[2]["type"], "message");
        assert_eq!(
            lines[2]["parentId"].as_str().unwrap(),
            lines[1]["id"].as_str().unwrap(),
            "second entry must point to first",
        );
        assert_eq!(lines[2]["message"]["role"], "assistant");
        assert_eq!(
            lines[2]["message"]["content"][0]["text"],
            "hello",
            "assistant text is wrapped in a content block",
        );
        assert_eq!(lines[2]["message"]["provider"], "openai");
        assert_eq!(lines[2]["message"]["model"], "gpt-4o-mini");
        assert_eq!(lines[2]["message"]["stopReason"], "stop");
    }

    #[test]
    fn system_messages_are_dropped() {
        let lines = export(
            vec![system("you are X"), user("hi"), assistant("hello")],
            vec![],
        );
        // Header + user + assistant only.
        assert_eq!(lines.len(), 3);
        for entry in &lines[1..] {
            assert_ne!(entry["message"]["role"], "system");
        }
    }

    #[test]
    fn compressed_messages_emitted_before_live_messages() {
        let lines = export(
            vec![user("live-q")],
            vec![user("old-q"), assistant("old-a")],
        );
        // Header + old-q + old-a + live-q.
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[1]["message"]["content"], "old-q");
        assert_eq!(lines[2]["message"]["content"][0]["text"], "old-a");
        assert_eq!(lines[3]["message"]["content"], "live-q");
        // The live message must chain off the compressed tail.
        assert_eq!(
            lines[3]["parentId"].as_str().unwrap(),
            lines[2]["id"].as_str().unwrap(),
        );
    }

    #[test]
    fn tool_call_assistant_splits_into_assistant_plus_tool_result() {
        // Build an assistant message with a single tool call + result.
        let call = ToolCall {
            name: "bash".into(),
            arguments: json!({"command": "ls"}),
            id: Some("call_abc".into()),
        };
        let tool_results = vec![ToolResult::new(call, json!("file1\nfile2"))];
        let asst = Message::new(
            MessageRole::Assistant,
            MessageContent::ToolCalls(crate::client::MessageContentToolCalls::new(
                tool_results,
                "running ls".into(),
            )),
        );
        let lines = export(vec![user("list files"), asst], vec![]);
        // Header + user + assistant(with toolCall) + toolResult.
        assert_eq!(lines.len(), 4);

        let assistant_entry = &lines[2]["message"];
        assert_eq!(assistant_entry["role"], "assistant");
        assert_eq!(assistant_entry["stopReason"], "toolUse");
        let blocks = assistant_entry["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2, "text + one toolCall block");
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "running ls");
        assert_eq!(blocks[1]["type"], "toolCall");
        assert_eq!(blocks[1]["id"], "call_abc");
        assert_eq!(blocks[1]["name"], "bash");
        assert_eq!(blocks[1]["arguments"]["command"], "ls");

        let result_entry = &lines[3]["message"];
        assert_eq!(result_entry["role"], "toolResult");
        assert_eq!(result_entry["toolCallId"], "call_abc");
        assert_eq!(result_entry["toolName"], "bash");
        assert_eq!(result_entry["content"][0]["text"], "file1\nfile2");
        assert_eq!(result_entry["isError"], false);
    }

    #[test]
    fn tool_call_without_id_mints_synthetic_id_that_matches_result() {
        let call = ToolCall {
            name: "bash".into(),
            arguments: json!({"command": "ls"}),
            id: None,
        };
        let tool_results = vec![ToolResult::new(call, json!("ok"))];
        let asst = Message::new(
            MessageRole::Assistant,
            MessageContent::ToolCalls(crate::client::MessageContentToolCalls::new(
                tool_results,
                String::new(),
            )),
        );
        let lines = export(vec![asst], vec![]);
        // Header + assistant + toolResult.
        let asst_call_id = lines[1]["message"]["content"][0]["id"].as_str().unwrap();
        let result_id = lines[2]["message"]["toolCallId"].as_str().unwrap();
        assert!(asst_call_id.starts_with("call_"));
        assert_eq!(asst_call_id, result_id);
    }

    #[test]
    fn all_entry_ids_are_unique() {
        // A bigger session to make sure synthetic ids don't collide.
        let mut msgs = vec![];
        for i in 0..20 {
            msgs.push(user(&format!("q{i}")));
            msgs.push(assistant(&format!("a{i}")));
        }
        let lines = export(msgs, vec![]);
        let mut seen = std::collections::HashSet::new();
        for entry in &lines[1..] {
            let id = entry["id"].as_str().unwrap().to_string();
            assert!(seen.insert(id.clone()), "id collision: {id}");
        }
    }
}

// --- pi JSONL conversion helpers -------------------------------------------

/// Format a UTC instant as the ISO 8601 string pi expects in entry
/// `timestamp` fields: millisecond precision, trailing `Z`.
fn iso_ms(t: &chrono::DateTime<chrono::Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Mint an 8-char lowercase-hex entry id. Pi accepts anything unique in
/// the file; truncating a v4 UUID gives 32 bits of entropy which is more
/// than enough for the few hundred entries a converted session contains.
fn short_hex_id() -> String {
    let s = uuid::Uuid::new_v4().simple().to_string();
    s[..8].to_string()
}

/// Zeroed `Usage` block. Converted sessions don't carry token/cost data
/// because aichat never recorded it per-message. Pi's loader tolerates
/// the zeros and just shows "0 tokens" in its session stats.
fn pi_zero_usage() -> Value {
    json!({
        "input": 0,
        "output": 0,
        "cacheRead": 0,
        "cacheWrite": 0,
        "totalTokens": 0,
        "cost": {
            "input": 0.0,
            "output": 0.0,
            "cacheRead": 0.0,
            "cacheWrite": 0.0,
            "total": 0.0,
        },
    })
}

/// Convert an aichat `MessageContent` for a user-role message into pi's
/// `UserMessage.content` shape (string when plain text, array of content
/// blocks when multimodal). Tool-call content can't appear on a user
/// message in either format; it falls back to a flattened text rendering.
#[allow(dead_code)] // Called from inside a closure in export_to_pi_jsonl;
                    // rustc's dead-code lint misses captures occasionally.
fn user_content_to_pi(content: &MessageContent) -> Value {
    match content {
        MessageContent::Text(text) => json!(text),
        MessageContent::Array(parts) => {
            let blocks: Vec<Value> = parts
                .iter()
                .map(|part| match part {
                    MessageContentPart::Text { text } => json!({"type": "text", "text": text}),
                    MessageContentPart::ImageUrl { image_url } => {
                        // Pi uses an embedded-data shape, not a URL ref.
                        // aichat already stores base64-encoded data URLs
                        // for local images, but external URLs would need
                        // fetch-and-encode work we don't do here. Emit as
                        // text so the message round-trips without lying
                        // about its provenance.
                        if let Some(rest) = image_url.url.strip_prefix("data:") {
                            if let Some((meta, b64)) = rest.split_once(',') {
                                let mime = meta.split(';').next().unwrap_or("image/png");
                                return json!({
                                    "type": "image",
                                    "data": b64,
                                    "mimeType": mime,
                                });
                            }
                        }
                        json!({"type": "text", "text": format!("[image: {}]", image_url.url)})
                    }
                })
                .collect();
            json!(blocks)
        }
        MessageContent::ToolCalls(_) => json!(content.to_text()),
    }
}
