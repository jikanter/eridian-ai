use super::*;

use crate::client::{
    init_client, patch_messages, ChatCompletionsData, Client, ImageUrl, Message, MessageContent,
    MessageContentPart, MessageContentToolCalls, MessageRole, Model,
};
use crate::function::ToolResult;
use crate::utils::{base64_encode, is_loader_protocol, sha256, AbortSignal};

use anyhow::{bail, Context, Result};
use indexmap::IndexSet;
use std::{collections::HashMap, fs::File, io::Read};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const IMAGE_EXTS: [&str; 5] = ["png", "jpeg", "jpg", "webp", "gif"];
const SUMMARY_MAX_WIDTH: usize = 80;

#[derive(Debug, Clone)]
pub struct Input {
    config: GlobalConfig,
    text: String,
    raw: (String, Vec<String>),
    patched_text: Option<String>,
    last_reply: Option<String>,
    continue_output: Option<String>,
    regenerate: bool,
    medias: Vec<String>,
    data_urls: HashMap<String, String>,
    tool_calls: Option<MessageContentToolCalls>,
    role: Role,
    rag_name: Option<String>,
    with_session: bool,
    with_agent: bool,
    /// Phase 9C: retry feedback — assistant's previous failed output and a
    /// corrective user message to append before resending. When set,
    /// `build_messages` appends an Assistant message with `failed_output`
    /// followed by a User message with `retry_prompt`.
    retry_feedback: Option<(String, String)>,
}

impl Input {
    pub fn from_str(config: &GlobalConfig, text: &str, role: Option<Role>) -> Self {
        let (role, with_session, with_agent) = resolve_role(&config.read(), role);
        Self {
            config: config.clone(),
            text: text.to_string(),
            raw: (text.to_string(), vec![]),
            patched_text: None,
            last_reply: None,
            continue_output: None,
            regenerate: false,
            medias: Default::default(),
            data_urls: Default::default(),
            tool_calls: None,
            role,
            rag_name: None,
            with_session,
            with_agent,
            retry_feedback: None,
        }
    }

    pub async fn from_files(
        config: &GlobalConfig,
        raw_text: &str,
        paths: Vec<String>,
        role: Option<Role>,
    ) -> Result<Self> {
        let loaders = config.read().document_loaders.clone();
        let (raw_paths, local_paths, remote_urls, external_cmds, protocol_paths, with_last_reply) =
            resolve_paths(&loaders, paths)?;
        let mut last_reply = None;
        let (documents, medias, data_urls) = load_documents(
            &loaders,
            local_paths,
            remote_urls,
            external_cmds,
            protocol_paths,
        )
        .await
        .context("Failed to load files")?;
        let mut texts = vec![];
        if !raw_text.is_empty() {
            texts.push(raw_text.to_string());
        };
        if with_last_reply {
            if let Some(LastMessage { input, output, .. }) = config.read().last_message.as_ref() {
                if !output.is_empty() {
                    last_reply = Some(output.clone())
                } else if let Some(v) = input.last_reply.as_ref() {
                    last_reply = Some(v.clone());
                }
                if let Some(v) = last_reply.clone() {
                    texts.push(format!("\n{v}"));
                }
            }
            if last_reply.is_none() && documents.is_empty() && medias.is_empty() {
                bail!("No last reply found");
            }
        }
        let (role, with_session, with_agent) = resolve_role(&config.read(), role);

        // Phase 11A/B: when multiple files were loaded and the user supplied a
        // query, rank files by BM25 relevance and greedily pack the top-scoring
        // subset into the model's input budget. Single-file or no-query cases
        // fall through to the uniform concatenation path below — same behavior
        // as before this phase.
        let documents = maybe_select_by_budget(documents, raw_text, &role);

        let documents_len = documents.len();
        for (kind, path, contents) in documents {
            if documents_len == 1 && raw_text.is_empty() {
                texts.push(format!("\n{contents}"));
            } else {
                texts.push(format!(
                    "\n============ {kind}: {path} ============\n{contents}"
                ));
            }
        }
        Ok(Self {
            config: config.clone(),
            text: texts.join("\n"),
            raw: (raw_text.to_string(), raw_paths),
            patched_text: None,
            last_reply,
            continue_output: None,
            regenerate: false,
            medias,
            data_urls,
            tool_calls: Default::default(),
            role,
            rag_name: None,
            with_session,
            with_agent,
            retry_feedback: None,
        })
    }

    pub async fn from_files_with_spinner(
        config: &GlobalConfig,
        raw_text: &str,
        paths: Vec<String>,
        role: Option<Role>,
        abort_signal: AbortSignal,
    ) -> Result<Self> {
        abortable_run_with_spinner(
            Input::from_files(config, raw_text, paths, role),
            "Loading files",
            abort_signal,
        )
        .await
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.medias.is_empty()
    }

    pub fn has_images(&self) -> bool {
        !self.medias.is_empty()
    }

    /// Phase 9D: Pre-flight capability check. Resolves the model+functions and validates
    /// they are compatible with the role and input. Cheap; safe to call before `dry_run`
    /// short-circuits so misconfigs surface even when no LLM call would be made.
    pub fn preflight(&self, model: &Model) -> Result<()> {
        let functions = self.config.read().select_functions(self.role());
        super::preflight::validate_model_capabilities(
            model,
            self.role(),
            functions.as_deref(),
            self.has_images(),
        )
    }

    pub fn data_urls(&self) -> HashMap<String, String> {
        self.data_urls.clone()
    }

    pub fn tool_calls(&self) -> &Option<MessageContentToolCalls> {
        &self.tool_calls
    }

    pub fn text(&self) -> String {
        match self.patched_text.clone() {
            Some(text) => text,
            None => self.text.clone(),
        }
    }

    pub fn clear_patch(&mut self) {
        self.patched_text = None;
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
    }

    pub fn has_structured_output_format(&self) -> bool {
        self.config
            .read()
            .output_format
            .map(|f| f.is_structured())
            .unwrap_or(false)
    }

    pub fn stream(&self) -> bool {
        if self.role().has_output_schema() || self.has_structured_output_format() {
            return false;
        }
        let cfg = self.config.read();
        if cfg.strip_thinking {
            return false;
        }
        cfg.stream && !self.role().model().no_stream()
    }

    pub fn continue_output(&self) -> Option<&str> {
        self.continue_output.as_deref()
    }

    pub fn set_continue_output(&mut self, output: &str) {
        let output = match &self.continue_output {
            Some(v) => format!("{v}{output}"),
            None => output.to_string(),
        };
        self.continue_output = Some(output);
    }

    /// Phase 9C: Attach an assistant's failed output + corrective user prompt
    /// to be replayed on the next call. Consumed by `build_messages`.
    pub fn with_retry_prompt(mut self, failed_output: &str, retry_prompt: &str) -> Self {
        self.retry_feedback = Some((failed_output.to_string(), retry_prompt.to_string()));
        self
    }

    pub fn retry_feedback(&self) -> Option<(&str, &str)> {
        self.retry_feedback
            .as_ref()
            .map(|(a, b)| (a.as_str(), b.as_str()))
    }

    pub fn regenerate(&self) -> bool {
        self.regenerate
    }

    pub fn set_regenerate(&mut self) {
        let role = self.config.read().extract_role();
        if role.name() == self.role().name() {
            self.role = role;
        }
        self.regenerate = true;
        self.tool_calls = None;
    }

    pub async fn use_embeddings(&mut self, abort_signal: AbortSignal) -> Result<()> {
        if self.text.is_empty() {
            return Ok(());
        }
        let rag = self.config.read().rag.clone();
        if let Some(rag) = rag {
            let result = Config::search_rag(&self.config, &rag, &self.text, abort_signal).await?;
            self.patched_text = Some(result);
            self.rag_name = Some(rag.name().to_string());
        }
        Ok(())
    }

    /// Phase 26D: retrieve from the role's + CLI's knowledge bindings and
    /// append the formatted hits to the user message. No-op when:
    /// - the role is in `knowledge_mode: tool` (26E handles it via a tool),
    /// - there are no bindings,
    /// - the prompt text is empty.
    ///
    /// Phase 27C: when `--trace` / `AICHAT_TRACE` is active, per-binding
    /// retrieval events are emitted on the TraceEmitter. The events are
    /// also cached on the Config for `.sources knowledge` in the REPL.
    pub fn use_knowledge(&mut self) -> Result<()> {
        if self.text.is_empty() {
            return Ok(());
        }
        if self.role.knowledge_mode() == Some("tool") {
            return Ok(());
        }

        // Merge role-declared + CLI-supplied bindings; dedupe by name.
        let mut bindings: Vec<KnowledgeBinding> = self.role.knowledge_bindings().to_vec();
        for name in self.config.read().cli_knowledge_bindings.clone() {
            if !bindings.iter().any(|b| b.name == name) {
                bindings.push(KnowledgeBinding::simple(name));
            }
        }
        if bindings.is_empty() {
            return Ok(());
        }

        let model_max = self.role.model().max_input_tokens();
        let budget = crate::knowledge::query::default_budget_for(model_max);
        let trace_emitter = self
            .config
            .read()
            .trace_config
            .clone()
            .map(crate::utils::trace::TraceEmitter::new);
        let (hits, events) = crate::knowledge::retrieve::retrieve_from_bindings_traced(
            &bindings,
            &self.text,
            &crate::knowledge::retrieve::RetrievalOptions::new_for_injection(budget),
            trace_emitter.as_ref(),
        )?;
        // Stash last retrieval events + hits for REPL `.sources knowledge`.
        self.config.write().last_knowledge_events = events;
        self.config.write().last_knowledge_hits = hits.clone();
        if hits.is_empty() {
            return Ok(());
        }
        let attributed = self.role.attributed_output();
        let context = if attributed {
            crate::knowledge::query::format_hits_for_attributed_injection(&hits)
        } else {
            crate::knowledge::query::format_hits_for_injection(&hits)
        };
        let base = self.patched_text.clone().unwrap_or_else(|| self.text.clone());
        self.patched_text = Some(format!("{base}{context}"));
        Ok(())
    }

    pub fn rag_name(&self) -> Option<&str> {
        self.rag_name.as_deref()
    }

    pub fn merge_tool_results(mut self, output: String, tool_results: Vec<ToolResult>) -> Self {
        match self.tool_calls.as_mut() {
            Some(exist_tool_results) => {
                exist_tool_results.merge(tool_results, output);
            }
            None => self.tool_calls = Some(MessageContentToolCalls::new(tool_results, output)),
        }
        self
    }

    pub fn create_client(&self) -> Result<Box<dyn Client>> {
        init_client(&self.config, Some(self.role().model().clone()))
    }

    pub async fn fetch_chat_text(&self) -> Result<String> {
        let client = self.create_client()?;
        let text = client.chat_completions(self.clone()).await?.text;
        let text = strip_think_tag(&text).to_string();
        Ok(text)
    }

    pub fn prepare_completion_data(
        &self,
        model: &Model,
        stream: bool,
    ) -> Result<ChatCompletionsData> {
        let mut messages = self.build_messages()?;
        // Phase 9B: when the provider will enforce the schema natively
        // (Claude tool-use-as-schema, OpenAI `response_format`), strip the
        // prompt-injected schema suffix so we don't pay for it twice.
        if self.role().has_output_schema()
            && model.data().supports_response_format_json_schema
        {
            strip_output_schema_suffix(&mut messages);
        }
        patch_messages(&mut messages, model);
        model.guard_max_input_tokens(&messages)?;
        let (temperature, top_p) = (self.role().temperature(), self.role().top_p());
        let functions = self.config.read().select_functions(self.role());

        // Phase 1C: Initialize deferred tool state when tool_search was returned
        if let Some(ref fns) = functions {
            use crate::function::TOOL_SEARCH_NAME;
            if fns.len() == 1
                && fns[0].name == TOOL_SEARCH_NAME
                && self.config.read().deferred_tools.is_none()
            {
                let all_tools = self
                    .config
                    .read()
                    .full_tool_list_for_deferred(self.role());
                self.config.write().deferred_tools = Some(super::DeferredToolState {
                    all_tools,
                    active_tools: None,
                });
            }
        }

        let output_schema = self.role().output_schema().cloned();
        let extensions = crate::client::lookup_client_extensions(
            &self.config.read(),
            model.client_name(),
        );

        Ok(ChatCompletionsData {
            messages,
            temperature,
            top_p,
            functions,
            stream,
            output_schema,
            extensions,
        })
    }

    pub fn build_messages(&self) -> Result<Vec<Message>> {
        let mut messages = if let Some(session) = self.session(&self.config.read().session) {
            session.build_messages(self)
        } else {
            self.role().build_messages(self)
        };
        // Inject output format suffix into system message when -o is set
        // and the role doesn't already have an output_schema (which takes precedence)
        if let Some(fmt) = &self.config.read().output_format {
            if !self.role().has_output_schema() {
                if let Some(suffix) = fmt.system_prompt_suffix() {
                    let injected = inject_system_suffix(&mut messages, suffix);
                    if !injected {
                        // No system message exists; prepend one
                        messages.insert(
                            0,
                            Message::new(
                                MessageRole::System,
                                MessageContent::Text(suffix.trim_start().to_string()),
                            ),
                        );
                    }
                }
            }
        }
        if let Some(tool_calls) = &self.tool_calls {
            messages.push(Message::new(
                MessageRole::Assistant,
                MessageContent::ToolCalls(tool_calls.clone()),
            ))
        }
        Ok(messages)
    }

    pub fn echo_messages(&self) -> String {
        if let Some(session) = self.session(&self.config.read().session) {
            session.echo_messages(self)
        } else {
            self.role().echo_messages(self)
        }
    }

    pub fn role(&self) -> &Role {
        &self.role
    }

    pub fn session<'a>(&self, session: &'a Option<Session>) -> Option<&'a Session> {
        if self.with_session {
            session.as_ref()
        } else {
            None
        }
    }

    pub fn session_mut<'a>(&self, session: &'a mut Option<Session>) -> Option<&'a mut Session> {
        if self.with_session {
            session.as_mut()
        } else {
            None
        }
    }

    pub fn with_agent(&self) -> bool {
        self.with_agent
    }

    pub fn summary(&self) -> String {
        let text: String = self
            .text
            .trim()
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        if text.width_cjk() > SUMMARY_MAX_WIDTH {
            let mut sum_width = 0;
            let mut chars = vec![];
            for c in text.chars() {
                sum_width += c.width_cjk().unwrap_or(1);
                if sum_width > SUMMARY_MAX_WIDTH - 3 {
                    chars.extend(['.', '.', '.']);
                    break;
                }
                chars.push(c);
            }
            chars.into_iter().collect()
        } else {
            text
        }
    }

    pub fn raw(&self) -> String {
        let (text, files) = &self.raw;
        let mut segments = files.to_vec();
        if !segments.is_empty() {
            segments.insert(0, ".file".into());
        }
        if !text.is_empty() {
            if !segments.is_empty() {
                segments.push("--".into());
            }
            segments.push(text.clone());
        }
        segments.join(" ")
    }

    pub fn render(&self) -> String {
        let text = self.text();
        if self.medias.is_empty() {
            return text;
        }
        let tail_text = if text.is_empty() {
            String::new()
        } else {
            format!(" -- {text}")
        };
        let files: Vec<String> = self
            .medias
            .iter()
            .cloned()
            .map(|url| resolve_data_url(&self.data_urls, url))
            .collect();
        format!(".file {}{}", files.join(" "), tail_text)
    }

    pub fn message_content(&self) -> MessageContent {
        if self.medias.is_empty() {
            MessageContent::Text(self.text())
        } else {
            let mut list: Vec<MessageContentPart> = self
                .medias
                .iter()
                .cloned()
                .map(|url| MessageContentPart::ImageUrl {
                    image_url: ImageUrl { url },
                })
                .collect();
            if !self.text.is_empty() {
                list.insert(0, MessageContentPart::Text { text: self.text() });
            }
            MessageContent::Array(list)
        }
    }
}

/// Phase 11A/B integration glue. Given the raw `-f` document list, decide
/// whether BM25 file ranking + budget-aware selection should kick in. It does
/// only when all of: (a) multiple files were loaded, (b) the user supplied a
/// query to rank against, (c) the role's model advertises a known
/// `max_input_tokens`, and (d) the total concatenated tokens would exceed the
/// files budget. Otherwise the documents list is passed through unchanged.
fn maybe_select_by_budget(
    documents: Vec<(&'static str, String, String)>,
    raw_text: &str,
    role: &Role,
) -> Vec<(&'static str, String, String)> {
    use crate::context_budget::{
        format_selection_summary, rank_files, select_within_budget, ContextBudget,
        DEFAULT_OUTPUT_RESERVE, FILES_SAFETY_MARGIN,
    };

    if documents.len() <= 1 || raw_text.trim().is_empty() {
        return documents;
    }
    let max_input = match role.model().max_input_tokens() {
        Some(v) => v,
        None => return documents,
    };

    let budget = ContextBudget::new(max_input, DEFAULT_OUTPUT_RESERVE);
    let files_budget = budget.remaining(FILES_SAFETY_MARGIN);

    let total_estimate: usize = documents
        .iter()
        .map(|(_, _, c)| crate::utils::estimate_token_length(c))
        .sum();
    if total_estimate <= files_budget {
        // Plenty of room; no ranking needed — preserves existing behavior.
        return documents;
    }

    // Build (path, content) pairs for ranking. Preserve the `kind` to restore
    // the full triple after selection.
    let mut kinds: std::collections::HashMap<String, &'static str> =
        std::collections::HashMap::new();
    let files: Vec<(String, String)> = documents
        .into_iter()
        .map(|(kind, path, content)| {
            kinds.insert(path.clone(), kind);
            (path, content)
        })
        .collect();

    let ranked = rank_files(files, raw_text);
    let outcome = select_within_budget(ranked, files_budget);
    if let Some(summary) = format_selection_summary(&outcome) {
        eprintln!("{summary}");
    }

    outcome
        .selected
        .into_iter()
        .map(|rc| {
            let kind = kinds.get(&rc.path).copied().unwrap_or("FILE");
            (kind, rc.path, rc.content)
        })
        .collect()
}

fn resolve_role(config: &Config, role: Option<Role>) -> (Role, bool, bool) {
    match role {
        Some(v) => (v, false, false),
        None => (
            config.extract_role(),
            config.session.is_some(),
            config.agent.is_some(),
        ),
    }
}

type ResolvePathsOutput = (
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    bool,
);

fn resolve_paths(
    loaders: &HashMap<String, String>,
    paths: Vec<String>,
) -> Result<ResolvePathsOutput> {
    let mut raw_paths = IndexSet::new();
    let mut local_paths = IndexSet::new();
    let mut remote_urls = IndexSet::new();
    let mut external_cmds = IndexSet::new();
    let mut protocol_paths = IndexSet::new();
    let mut with_last_reply = false;
    for path in paths {
        if path == "%%" {
            with_last_reply = true;
            raw_paths.insert(path);
        } else if path.starts_with('`') && path.len() > 2 && path.ends_with('`') {
            external_cmds.insert(path[1..path.len() - 1].to_string());
            raw_paths.insert(path);
        } else if is_url(&path) {
            if path.strip_suffix("**").is_some() {
                bail!("Invalid website '{path}'");
            }
            remote_urls.insert(path.clone());
            raw_paths.insert(path);
        } else if is_loader_protocol(loaders, &path) {
            protocol_paths.insert(path.clone());
            raw_paths.insert(path);
        } else {
            let resolved_path = resolve_home_dir(&path);
            let absolute_path = to_absolute_path(&resolved_path)
                .with_context(|| format!("Invalid path '{path}'"))?;
            local_paths.insert(resolved_path);
            raw_paths.insert(absolute_path);
        }
    }
    Ok((
        raw_paths.into_iter().collect(),
        local_paths.into_iter().collect(),
        remote_urls.into_iter().collect(),
        external_cmds.into_iter().collect(),
        protocol_paths.into_iter().collect(),
        with_last_reply,
    ))
}

async fn load_documents(
    loaders: &HashMap<String, String>,
    local_paths: Vec<String>,
    remote_urls: Vec<String>,
    external_cmds: Vec<String>,
    protocol_paths: Vec<String>,
) -> Result<(
    Vec<(&'static str, String, String)>,
    Vec<String>,
    HashMap<String, String>,
)> {
    let mut files = vec![];
    let mut medias = vec![];
    let mut data_urls = HashMap::new();

    for cmd in external_cmds {
        let output = duct::cmd(&SHELL.cmd, &[&SHELL.arg, &cmd])
            .stderr_to_stdout()
            .unchecked()
            .read()
            .unwrap_or_else(|err| err.to_string());
        files.push(("CMD", cmd, output));
    }

    let local_files = expand_glob_paths(&local_paths, true).await?;
    for file_path in local_files {
        if is_image(&file_path) {
            let contents = read_media_to_data_url(&file_path)
                .with_context(|| format!("Unable to read media '{file_path}'"))?;
            data_urls.insert(sha256(&contents), file_path);
            medias.push(contents)
        } else {
            let document = load_file(loaders, &file_path)
                .await
                .with_context(|| format!("Unable to read file '{file_path}'"))?;
            files.push(("FILE", file_path, document.contents));
        }
    }

    for file_url in remote_urls {
        let (contents, extension) = fetch_with_loaders(loaders, &file_url, true)
            .await
            .with_context(|| format!("Failed to load url '{file_url}'"))?;
        if extension == MEDIA_URL_EXTENSION {
            data_urls.insert(sha256(&contents), file_url);
            medias.push(contents)
        } else {
            files.push(("URL", file_url, contents));
        }
    }

    for protocol_path in protocol_paths {
        let documents = load_protocol_path(loaders, &protocol_path)
            .with_context(|| format!("Failed to load from '{protocol_path}'"))?;
        files.extend(
            documents
                .into_iter()
                .map(|document| ("FROM", document.path, document.contents)),
        );
    }

    Ok((files, medias, data_urls))
}

pub fn resolve_data_url(data_urls: &HashMap<String, String>, data_url: String) -> String {
    if data_url.starts_with("data:") {
        let hash = sha256(&data_url);
        if let Some(path) = data_urls.get(&hash) {
            return path.to_string();
        }
        data_url
    } else {
        data_url
    }
}

fn is_image(path: &str) -> bool {
    get_patch_extension(path)
        .map(|v| IMAGE_EXTS.contains(&v.as_str()))
        .unwrap_or_default()
}

/// Remove the schema-injected suffix (starting at `OUTPUT_SCHEMA_SUFFIX_MARKER`)
/// from the system message. Leaves the rest of the prompt intact. If stripping
/// leaves the system message empty, the message itself is removed so empty
/// `system: ""` never reaches the provider.
fn strip_output_schema_suffix(messages: &mut Vec<Message>) {
    use crate::config::role::OUTPUT_SCHEMA_SUFFIX_MARKER;
    let mut clear_index: Option<usize> = None;
    for (i, msg) in messages.iter_mut().enumerate() {
        if msg.role == MessageRole::System {
            if let MessageContent::Text(ref mut text) = msg.content {
                if let Some(pos) = text.find(OUTPUT_SCHEMA_SUFFIX_MARKER) {
                    text.truncate(pos);
                    let trimmed = text.trim_end().to_string();
                    if trimmed.is_empty() {
                        clear_index = Some(i);
                    } else {
                        *text = trimmed;
                    }
                }
            }
            break;
        }
    }
    if let Some(i) = clear_index {
        messages.remove(i);
    }
}

fn inject_system_suffix(messages: &mut [Message], suffix: &str) -> bool {
    for msg in messages.iter_mut() {
        if msg.role == MessageRole::System {
            if let MessageContent::Text(ref mut text) = msg.content {
                text.push_str(suffix);
                return true;
            }
        }
    }
    false
}

fn read_media_to_data_url(image_path: &str) -> Result<String> {
    let extension = get_patch_extension(image_path).unwrap_or_default();
    let mime_type = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => bail!("Unexpected media type"),
    };
    let mut file = File::open(image_path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    let encoded_image = base64_encode(buffer);
    let data_url = format!("data:{mime_type};base64,{encoded_image}");

    Ok(data_url)
}

#[cfg(test)]
mod schema_suffix_tests {
    use super::*;

    #[test]
    fn strips_suffix_and_keeps_original_system_prompt() {
        let mut messages = vec![
            Message::new(
                MessageRole::System,
                MessageContent::Text(
                    "Be concise.\n\nYou MUST respond with valid JSON conforming to this JSON Schema:\n```json\n{\"type\":\"object\"}\n```\nDo not include any text outside the JSON object.".into(),
                ),
            ),
            Message::new(MessageRole::User, MessageContent::Text("hi".into())),
        ];
        strip_output_schema_suffix(&mut messages);
        assert_eq!(messages.len(), 2);
        if let MessageContent::Text(t) = &messages[0].content {
            assert_eq!(t, "Be concise.");
        } else {
            panic!("system message must stay text");
        }
    }

    #[test]
    fn removes_system_message_entirely_when_only_suffix_was_present() {
        let mut messages = vec![
            Message::new(
                MessageRole::System,
                MessageContent::Text(
                    "You MUST respond with valid JSON conforming to this JSON Schema:\n```json\n{}\n```\nDo not include any text outside the JSON object.".into(),
                ),
            ),
            Message::new(MessageRole::User, MessageContent::Text("hi".into())),
        ];
        strip_output_schema_suffix(&mut messages);
        assert_eq!(messages.len(), 1, "empty system is dropped");
        assert_eq!(messages[0].role, MessageRole::User);
    }

    #[test]
    fn noop_when_no_suffix_present() {
        let mut messages = vec![
            Message::new(MessageRole::System, MessageContent::Text("Be concise.".into())),
        ];
        strip_output_schema_suffix(&mut messages);
        assert_eq!(messages.len(), 1);
        if let MessageContent::Text(t) = &messages[0].content {
            assert_eq!(t, "Be concise.");
        }
    }
}
