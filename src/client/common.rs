use super::*;

use crate::{
    config::{Config, GlobalConfig, Input},
    function::{eval_tool_calls, FunctionDeclaration, ToolCall, ToolResult},
    render::render_stream,
    utils::*,
};

use anyhow::{bail, Context, Result};
use fancy_regex::Regex;
use indexmap::IndexMap;
use inquire::{
    list_option::ListOption, required, validator::Validation, MultiSelect, Select, Text,
};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::unbounded_channel;

const MODELS_YAML: &str = include_str!("../../models.yaml");

pub static ALL_PROVIDER_MODELS: LazyLock<Vec<ProviderModels>> = LazyLock::new(|| {
    Config::loal_models_override()
        .ok()
        .unwrap_or_else(|| serde_yaml::from_str(MODELS_YAML).unwrap())
});

static EMBEDDING_MODEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"((^|/)(bge-|e5-|uae-|gte-|text-)|embed|multilingual|minilm)").unwrap()
});

static ESCAPE_SLASH_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?<!\\)/").unwrap());

#[async_trait::async_trait]
pub trait Client: Sync + Send {
    fn global_config(&self) -> &GlobalConfig;

    fn extra_config(&self) -> Option<&ExtraConfig>;

    fn patch_config(&self) -> Option<&RequestPatch>;

    fn name(&self) -> &str;

    fn model(&self) -> &Model;

    fn model_mut(&mut self) -> &mut Model;

    fn build_client(&self) -> Result<ReqwestClient> {
        let mut builder = ReqwestClient::builder();
        let extra = self.extra_config();
        let timeout = extra.and_then(|v| v.connect_timeout).unwrap_or(10);
        if let Some(proxy) = extra.and_then(|v| v.proxy.as_deref()) {
            builder = set_proxy(builder, proxy)?;
        }
        if let Some(user_agent) = self.global_config().read().user_agent.as_ref() {
            builder = builder.user_agent(user_agent);
        }
        let client = builder
            .connect_timeout(Duration::from_secs(timeout))
            .build()
            .with_context(|| "Failed to build client")?;
        Ok(client)
    }

    async fn chat_completions(&self, input: Input) -> Result<ChatCompletionsOutput> {
        // Phase 9D: capability preflight runs before the dry-run short-circuit so
        // misconfigs surface at config time, not at API time.
        input.preflight(self.model())?;
        if self.global_config().read().dry_run {
            let content = input.echo_messages();
            return Ok(ChatCompletionsOutput::new(&content));
        }
        let client = self.build_client()?;
        let data = input.prepare_completion_data(self.model(), false)?;
        self.chat_completions_inner(&client, data)
            .await
            .with_context(|| "Failed to call chat-completions api")
    }

    async fn chat_completions_streaming(
        &self,
        input: &Input,
        handler: &mut SseHandler,
    ) -> Result<()> {
        let abort_signal = handler.abort();
        let input = input.clone();
        tokio::select! {
            ret = async {
                // Phase 9D: capability preflight runs before the dry-run short-circuit so
                // misconfigs surface at config time, not at API time. Inside the select arm
                // so the surrounding handler.done() still fires on early Err.
                input.preflight(self.model())?;
                if self.global_config().read().dry_run {
                    let content = input.echo_messages();
                    handler.text(&content)?;
                    return Ok(());
                }
                let client = self.build_client()?;
                let data = input.prepare_completion_data(self.model(), true)?;
                self.chat_completions_streaming_inner(&client, handler, data).await
            } => {
                handler.done();
                ret.with_context(|| "Failed to call chat-completions api")
            }
            _ = wait_abort_signal(&abort_signal) => {
                handler.done();
                Ok(())
            },
        }
    }

    async fn embeddings(&self, data: &EmbeddingsData) -> Result<Vec<Vec<f32>>> {
        let client = self.build_client()?;
        self.embeddings_inner(&client, data)
            .await
            .context("Failed to call embeddings api")
    }

    async fn rerank(&self, data: &RerankData) -> Result<RerankOutput> {
        let client = self.build_client()?;
        self.rerank_inner(&client, data)
            .await
            .context("Failed to call rerank api")
    }

    async fn chat_completions_inner(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<ChatCompletionsOutput>;

    async fn chat_completions_streaming_inner(
        &self,
        client: &ReqwestClient,
        handler: &mut SseHandler,
        data: ChatCompletionsData,
    ) -> Result<()>;

    async fn embeddings_inner(
        &self,
        _client: &ReqwestClient,
        _data: &EmbeddingsData,
    ) -> Result<EmbeddingsOutput> {
        bail!("The client doesn't support embeddings api")
    }

    async fn rerank_inner(
        &self,
        _client: &ReqwestClient,
        _data: &RerankData,
    ) -> Result<RerankOutput> {
        bail!("The client doesn't support rerank api")
    }

    fn request_builder(
        &self,
        client: &reqwest::Client,
        mut request_data: RequestData,
    ) -> RequestBuilder {
        self.patch_request_data(&mut request_data);
        request_data.into_builder(client)
    }

    fn patch_request_data(&self, request_data: &mut RequestData) {
        let model_type = self.model().model_type();
        if let Some(patch) = self.model().patch() {
            request_data.apply_patch(patch.clone());
        }

        let patch_map = std::env::var(get_env_name(&format!(
            "patch_{}_{}",
            self.model().client_name(),
            model_type.api_name(),
        )))
        .ok()
        .and_then(|v| serde_json::from_str(&v).ok())
        .or_else(|| {
            self.patch_config()
                .and_then(|v| model_type.extract_patch(v))
                .cloned()
        });
        let patch_map = match patch_map {
            Some(v) => v,
            _ => return,
        };
        for (key, patch) in patch_map {
            let key = ESCAPE_SLASH_RE.replace_all(&key, r"\/");
            if let Ok(regex) = Regex::new(&format!("^({key})$")) {
                if let Ok(true) = regex.is_match(self.model().name()) {
                    request_data.apply_patch(patch);
                    return;
                }
            }
        }
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self::OpenAIConfig(OpenAIConfig::default())
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExtraConfig {
    pub proxy: Option<String>,
    pub connect_timeout: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RequestPatch {
    pub chat_completions: Option<ApiPatch>,
    pub embeddings: Option<ApiPatch>,
    pub rerank: Option<ApiPatch>,
}

pub type ApiPatch = IndexMap<String, Value>;

pub struct RequestData {
    pub url: String,
    pub headers: IndexMap<String, String>,
    pub body: Value,
}

impl RequestData {
    pub fn new<T>(url: T, body: Value) -> Self
    where
        T: std::fmt::Display,
    {
        Self {
            url: url.to_string(),
            headers: Default::default(),
            body,
        }
    }

    pub fn bearer_auth<T>(&mut self, auth: T)
    where
        T: std::fmt::Display,
    {
        self.headers
            .insert("authorization".into(), format!("Bearer {auth}"));
    }

    pub fn header<K, V>(&mut self, key: K, value: V)
    where
        K: std::fmt::Display,
        V: std::fmt::Display,
    {
        self.headers.insert(key.to_string(), value.to_string());
    }

    pub fn into_builder(self, client: &ReqwestClient) -> RequestBuilder {
        let RequestData { url, headers, body } = self;
        debug!("Request {url} {body}");

        let mut builder = client.post(url);
        for (key, value) in headers {
            builder = builder.header(key, value);
        }
        builder = builder.json(&body);
        builder
    }

    pub fn apply_patch(&mut self, patch: Value) {
        if let Some(patch_url) = patch["url"].as_str() {
            self.url = patch_url.into();
        }
        if let Some(patch_body) = patch.get("body") {
            json_patch::merge(&mut self.body, patch_body)
        }
        if let Some(patch_headers) = patch["headers"].as_object() {
            for (key, value) in patch_headers {
                if let Some(value) = value.as_str() {
                    self.header(key, value)
                } else if value.is_null() {
                    self.headers.swap_remove(key);
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct ChatCompletionsData {
    pub messages: Vec<Message>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub functions: Option<Vec<FunctionDeclaration>>,
    pub stream: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ChatCompletionsOutput {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub id: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

impl ChatCompletionsOutput {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CallMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub latency_ms: u64,
    pub model_id: String,
    pub turns: u32,
}

impl CallMetrics {
    pub fn merge(&mut self, other: &CallMetrics) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cost_usd += other.cost_usd;
        self.latency_ms += other.latency_ms;
        self.turns += other.turns;
        if !other.model_id.is_empty() {
            self.model_id.clone_from(&other.model_id);
        }
    }
}

pub fn compute_cost(model: &Model, input_tokens: u64, output_tokens: u64) -> f64 {
    let ip = model.data().input_price.unwrap_or(0.0);
    let op = model.data().output_price.unwrap_or(0.0);
    (input_tokens as f64 * ip + output_tokens as f64 * op) / 1_000_000.0
}

#[derive(Debug)]
pub struct EmbeddingsData {
    pub texts: Vec<String>,
    pub query: bool,
}

impl EmbeddingsData {
    pub fn new(texts: Vec<String>, query: bool) -> Self {
        Self { texts, query }
    }
}

pub type EmbeddingsOutput = Vec<Vec<f32>>;

#[derive(Debug)]
pub struct RerankData {
    pub query: String,
    pub documents: Vec<String>,
    pub top_n: usize,
}

impl RerankData {
    pub fn new(query: String, documents: Vec<String>, top_n: usize) -> Self {
        Self {
            query,
            documents,
            top_n,
        }
    }
}

pub type RerankOutput = Vec<RerankResult>;

#[derive(Debug, Deserialize)]
pub struct RerankResult {
    pub index: usize,
    pub relevance_score: f64,
}

pub type PromptAction<'a> = (&'a str, &'a str, Option<&'a str>);

pub async fn create_config(
    prompts: &[PromptAction<'static>],
    client: &str,
) -> Result<(String, Value)> {
    let mut config = json!({
        "type": client,
    });
    for (key, desc, help_message) in prompts {
        let env_name = format!("{client}_{key}").to_ascii_uppercase();
        let required = std::env::var(&env_name).is_err();
        let value = prompt_input_string(desc, required, *help_message)?;
        if !value.is_empty() {
            config[key] = value.into();
        }
    }
    let model = set_client_models_config(&mut config, client).await?;
    let clients = json!(vec![config]);
    Ok((model, clients))
}

pub async fn create_openai_compatible_client_config(
    client: &str,
) -> Result<Option<(String, Value)>> {
    let api_base = super::OPENAI_COMPATIBLE_PROVIDERS
        .into_iter()
        .find(|(name, _)| client == *name)
        .map(|(_, api_base)| api_base)
        .unwrap_or("http(s)://{API_ADDR}/v1");

    let name = if client == OpenAICompatibleClient::NAME {
        let value = prompt_input_string("Provider Name", true, None)?;
        value.replace(' ', "-")
    } else {
        client.to_string()
    };

    let mut config = json!({
        "type": OpenAICompatibleClient::NAME,
        "name": &name,
    });

    let api_base = if api_base.contains('{') {
        prompt_input_string("API Base", true, Some(&format!("e.g. {api_base}")))?
    } else {
        api_base.to_string()
    };
    config["api_base"] = api_base.into();

    let api_key = prompt_input_string("API Key", false, None)?;
    if !api_key.is_empty() {
        config["api_key"] = api_key.into();
    }

    let model = set_client_models_config(&mut config, &name).await?;
    let clients = json!(vec![config]);
    Ok(Some((model, clients)))
}

const MAX_REACT_STEPS: usize = 10;

/// Maximum retries for the same (tool, error_signature) pair before escalating.
const MAX_TOOL_RETRIES: usize = 2;

/// Penalty applied to remaining steps for each repeated tool failure.
const STEP_PENALTY_PER_REPEAT: usize = 2;

pub async fn call_react(
    input: &mut Input,
    client: &dyn Client,
    abort_signal: AbortSignal,
) -> Result<(String, Vec<ToolResult>, CallMetrics)> {
    let mut total_text = String::new();
    let mut cumulative_metrics = CallMetrics::default();
    let mut step = 0;
    let mut max_steps = MAX_REACT_STEPS;
    let has_structured_output = input.role().has_output_schema()
        || input.has_structured_output_format();
    let print_output = !has_structured_output;

    // Phase 8F/8G: Create trace emitter if tracing is active
    let mut tracer = client
        .global_config()
        .read()
        .trace_config
        .clone()
        .map(crate::utils::trace::TraceEmitter::new);

    // Phase 7C: Track repeated tool failures for retry budget.
    // Key: (tool_name, error_signature_hash), Value: retry count
    let mut failure_counts: std::collections::HashMap<(String, u64), usize> =
        std::collections::HashMap::new();

    loop {
        let (text, tool_results, metrics) = if input.stream() {
            call_chat_completions_streaming(input, client, abort_signal.clone()).await?
        } else {
            call_chat_completions(input, print_output, false, client, abort_signal.clone()).await?
        };

        // Trace: emit request info
        if let Some(ref mut t) = tracer {
            let tool_names: Vec<String> = tool_results.iter().map(|r| r.call.name.clone()).collect();
            t.emit_request(
                &metrics.model_id,
                metrics.input_tokens,
                metrics.output_tokens,
                metrics.latency_ms,
                &tool_names,
                &text,
            );
        }

        cumulative_metrics.merge(&metrics);
        if !total_text.is_empty() {
            total_text.push('\n');
        }
        total_text.push_str(&text);
        if tool_results.is_empty() {
            cumulative_metrics.turns = (step + 1).min(u32::MAX as usize) as u32;
            if let Some(ref t) = tracer {
                t.emit_summary(&cumulative_metrics);
            }
            return Ok((total_text, vec![], cumulative_metrics));
        }

        // Phase 7C: Check for repeated failures and annotate tool results
        let tool_results = annotate_repeated_failures(
            tool_results,
            &mut failure_counts,
            &mut max_steps,
        );

        // Trace: emit tool results
        if let Some(ref mut t) = tracer {
            t.emit_tool_results(&tool_results, metrics.latency_ms);
        }

        step += 1;
        if step >= max_steps {
            bail!("ReAct loop exceeded maximum steps ({max_steps})");
        }
        *input = input.clone().merge_tool_results(text, tool_results);
    }
}

/// Check tool results for repeated failures. Annotate results with retry warnings
/// and decay the step budget for repeated errors.
fn annotate_repeated_failures(
    mut results: Vec<ToolResult>,
    failure_counts: &mut std::collections::HashMap<(String, u64), usize>,
    max_steps: &mut usize,
) -> Vec<ToolResult> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    for result in results.iter_mut() {
        let output_str = result.output.to_string();
        if !output_str.contains("[TOOL_ERROR]") {
            continue;
        }

        // Hash the error signature (tool name + error content)
        let mut hasher = DefaultHasher::new();
        output_str.hash(&mut hasher);
        let error_hash = hasher.finish();
        let key = (result.call.name.clone(), error_hash);

        let count = failure_counts.entry(key).or_insert(0);
        *count += 1;

        if *count >= MAX_TOOL_RETRIES + 1 {
            // Third identical failure — append escalation notice
            let escalation = format!(
                "\n\n[TOOL_ERROR] This is attempt #{} with identical arguments and error. \
                 Do NOT retry this tool with the same arguments. \
                 Either use different arguments, try a different tool, or ask the user for help.",
                count
            );
            result.output =
                json!(format!("{}{}", output_str.trim_matches('"'), escalation));
        } else if *count == MAX_TOOL_RETRIES {
            // Second identical failure — append warning
            let warning = format!(
                "\n\nThis is the second time this exact call failed. \
                 Do not retry with identical arguments."
            );
            result.output =
                json!(format!("{}{}", output_str.trim_matches('"'), warning));
        }

        // Decay step budget for each repeated failure
        if *count > 1 {
            *max_steps = max_steps.saturating_sub(STEP_PENALTY_PER_REPEAT);
            if *max_steps < 2 {
                *max_steps = 2; // Always allow at least 2 steps
            }
        }
    }
    results
}

pub async fn call_chat_completions(
    input: &Input,
    print: bool,
    extract_code: bool,
    client: &dyn Client,
    abort_signal: AbortSignal,
) -> Result<(String, Vec<ToolResult>, CallMetrics)> {
    let start = Instant::now();
    let ret = abortable_run_with_spinner(
        client.chat_completions(input.clone()),
        "Generating",
        abort_signal,
    )
    .await;

    match ret {
        Ok(ret) => {
            let ChatCompletionsOutput {
                mut text,
                tool_calls,
                input_tokens,
                output_tokens,
                ..
            } = ret;
            let it = input_tokens.unwrap_or(0);
            let ot = output_tokens.unwrap_or(0);
            let metrics = CallMetrics {
                input_tokens: it,
                output_tokens: ot,
                cost_usd: compute_cost(client.model(), it, ot),
                latency_ms: start.elapsed().as_millis() as u64,
                model_id: client.model().id(),
                turns: 1,
            };
            if !text.is_empty() {
                if extract_code {
                    text = extract_code_block(&strip_think_tag(&text)).to_string();
                } else if client.global_config().read().strip_thinking {
                    text = crate::strip_thinking::strip_thinking_str(&text);
                }
                if print {
                    client.global_config().read().print_markdown(&text)?;
                }
            }
            Ok((text, eval_tool_calls(client.global_config(), tool_calls).await?, metrics))
        }
        Err(err) => Err(err),
    }
}

pub async fn call_chat_completions_streaming(
    input: &Input,
    client: &dyn Client,
    abort_signal: AbortSignal,
) -> Result<(String, Vec<ToolResult>, CallMetrics)> {
    let start = Instant::now();
    let (tx, rx) = unbounded_channel();
    let mut handler = SseHandler::new(tx, abort_signal.clone());

    let (send_ret, render_ret) = tokio::join!(
        client.chat_completions_streaming(input, &mut handler),
        render_stream(rx, client.global_config(), abort_signal.clone()),
    );

    if handler.abort().aborted() {
        bail!("Aborted.");
    }

    render_ret?;

    let (text, tool_calls, input_tokens, output_tokens) = handler.take();
    let it = input_tokens.unwrap_or(0);
    let ot = output_tokens.unwrap_or(0);
    let metrics = CallMetrics {
        input_tokens: it,
        output_tokens: ot,
        cost_usd: compute_cost(client.model(), it, ot),
        latency_ms: start.elapsed().as_millis() as u64,
        model_id: client.model().id(),
        turns: 1,
    };
    match send_ret {
        Ok(_) => {
            if !text.is_empty() && !text.ends_with('\n') {
                println!();
            }
            Ok((text, eval_tool_calls(client.global_config(), tool_calls).await?, metrics))
        }
        Err(err) => {
            if !text.is_empty() {
                println!();
            }
            Err(err)
        }
    }
}

pub fn noop_prepare_embeddings<T>(_client: &T, _data: &EmbeddingsData) -> Result<RequestData> {
    bail!("The client doesn't support embeddings api")
}

pub async fn noop_embeddings(_builder: RequestBuilder, _model: &Model) -> Result<EmbeddingsOutput> {
    bail!("The client doesn't support embeddings api")
}

pub fn noop_prepare_rerank<T>(_client: &T, _data: &RerankData) -> Result<RequestData> {
    bail!("The client doesn't support rerank api")
}

pub async fn noop_rerank(_builder: RequestBuilder, _model: &Model) -> Result<RerankOutput> {
    bail!("The client doesn't support rerank api")
}

pub fn catch_error(data: &Value, status: u16) -> Result<()> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    debug!("Invalid response, status: {status}, data: {data}");
    if let Some(error) = data["error"].as_object() {
        if let (Some(typ), Some(message)) = (
            json_str_from_map(error, "type"),
            json_str_from_map(error, "message"),
        ) {
            bail!("{message} (type: {typ})");
        } else if let (Some(typ), Some(message)) = (
            json_str_from_map(error, "code"),
            json_str_from_map(error, "message"),
        ) {
            bail!("{message} (code: {typ})");
        }
    } else if let Some(error) = data["errors"][0].as_object() {
        if let (Some(code), Some(message)) = (
            error.get("code").and_then(|v| v.as_u64()),
            json_str_from_map(error, "message"),
        ) {
            bail!("{message} (status: {code})")
        }
    } else if let Some(error) = data[0]["error"].as_object() {
        if let (Some(status), Some(message)) = (
            json_str_from_map(error, "status"),
            json_str_from_map(error, "message"),
        ) {
            bail!("{message} (status: {status})")
        }
    } else if let (Some(detail), Some(status)) = (data["detail"].as_str(), data["status"].as_i64())
    {
        bail!("{detail} (status: {status})");
    } else if let Some(error) = data["error"].as_str() {
        bail!("{error}");
    } else if let Some(message) = data["message"].as_str() {
        bail!("{message}");
    }
    bail!("Invalid response data: {data} (status: {status})");
}

pub fn json_str_from_map<'a>(
    map: &'a serde_json::Map<String, Value>,
    field_name: &str,
) -> Option<&'a str> {
    map.get(field_name).and_then(|v| v.as_str())
}

async fn set_client_models_config(client_config: &mut Value, client: &str) -> Result<String> {
    if let Some(provider) = ALL_PROVIDER_MODELS.iter().find(|v| v.provider == client) {
        let models: Vec<String> = provider
            .models
            .iter()
            .filter(|v| v.model_type == "chat")
            .map(|v| v.name.clone())
            .collect();
        let model_name = select_model(models)?;
        return Ok(format!("{client}:{model_name}"));
    }
    let mut model_names = vec![];
    if let (Some(true), Some(api_base), api_key) = (
        client_config["type"]
            .as_str()
            .map(|v| v == OpenAICompatibleClient::NAME),
        client_config["api_base"].as_str(),
        client_config["api_key"]
            .as_str()
            .map(|v| v.to_string())
            .or_else(|| {
                let env_name = format!("{client}_api_key").to_ascii_uppercase();
                std::env::var(&env_name).ok()
            }),
    ) {
        match abortable_run_with_spinner(
            fetch_models(api_base, api_key.as_deref()),
            "Fetching models",
            create_abort_signal(),
        )
        .await
        {
            Ok(fetched_models) => {
                model_names = MultiSelect::new("LLMs to include (required):", fetched_models)
                    .with_validator(|list: &[ListOption<&String>]| {
                        if list.is_empty() {
                            Ok(Validation::Invalid(
                                "At least one item must be selected".into(),
                            ))
                        } else {
                            Ok(Validation::Valid)
                        }
                    })
                    .prompt()?;
            }
            Err(err) => {
                eprintln!("✗ Fetch models failed: {err}");
            }
        }
    }
    if model_names.is_empty() {
        model_names = prompt_input_string(
            "LLMs to add",
            true,
            Some("Separated by commas, e.g. llama3.3,qwen2.5"),
        )?
        .split(',')
        .filter_map(|v| {
            let v = v.trim();
            if v.is_empty() {
                None
            } else {
                Some(v.to_string())
            }
        })
        .collect::<Vec<_>>();
    }
    if model_names.is_empty() {
        bail!("No models");
    }
    let models: Vec<Value> = model_names
        .iter()
        .map(|v| {
            let l = v.to_lowercase();
            if l.contains("rank") {
                json!({
                    "name": v,
                    "type": "reranker",
                })
            } else if let Ok(true) = EMBEDDING_MODEL_RE.is_match(&l) {
                json!({
                    "name": v,
                    "type": "embedding",
                    "default_chunk_size": 1000,
                    "max_batch_size": 100
                })
            } else if v.contains("vision") {
                json!({
                    "name": v,
                    "supports_vision": true
                })
            } else {
                json!({
                    "name": v,
                })
            }
        })
        .collect();
    client_config["models"] = models.into();
    let model_name = select_model(model_names)?;
    Ok(format!("{client}:{model_name}"))
}

fn select_model(model_names: Vec<String>) -> Result<String> {
    if model_names.is_empty() {
        bail!("No models");
    }
    let model = if model_names.len() == 1 {
        model_names[0].clone()
    } else {
        Select::new("Default Model (required):", model_names).prompt()?
    };
    Ok(model)
}

fn prompt_input_string(
    desc: &str,
    required: bool,
    help_message: Option<&str>,
) -> anyhow::Result<String> {
    let desc = if required {
        format!("{desc} (required):")
    } else {
        format!("{desc} (optional):")
    };
    let mut text = Text::new(&desc);
    if required {
        text = text.with_validator(required!("This field is required"))
    }
    if let Some(help_message) = help_message {
        text = text.with_help_message(help_message);
    }
    let text = text.prompt()?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call_metrics_merge() {
        let mut a = CallMetrics {
            input_tokens: 100,
            output_tokens: 50,
            cost_usd: 0.001,
            latency_ms: 500,
            model_id: "model-a".into(),
            turns: 1,
        };
        let b = CallMetrics {
            input_tokens: 200,
            output_tokens: 100,
            cost_usd: 0.002,
            latency_ms: 300,
            model_id: "model-b".into(),
            turns: 1,
        };
        a.merge(&b);
        assert_eq!(a.input_tokens, 300);
        assert_eq!(a.output_tokens, 150);
        assert!((a.cost_usd - 0.003).abs() < 1e-10);
        assert_eq!(a.latency_ms, 800);
        assert_eq!(a.model_id, "model-b");
        assert_eq!(a.turns, 2);
    }

    #[test]
    fn test_call_metrics_merge_empty_model_id() {
        let mut a = CallMetrics {
            model_id: "original".into(),
            ..Default::default()
        };
        let b = CallMetrics::default(); // empty model_id
        a.merge(&b);
        assert_eq!(a.model_id, "original"); // should not overwrite with empty
    }

    #[test]
    fn test_compute_cost_with_prices() {
        let mut model = Model::new("test", "gpt-4o");
        let data = model.data_mut();
        data.input_price = Some(5.0); // $5 per 1M tokens
        data.output_price = Some(15.0); // $15 per 1M tokens

        let cost = compute_cost(&model, 1000, 500);
        // (1000 * 5.0 + 500 * 15.0) / 1_000_000 = (5000 + 7500) / 1_000_000 = 0.0125
        assert!((cost - 0.0125).abs() < 1e-10);
    }

    #[test]
    fn test_compute_cost_no_prices() {
        let model = Model::new("test", "unknown-model");
        let cost = compute_cost(&model, 1000, 500);
        assert!((cost - 0.0).abs() < 1e-10);
    }
}
