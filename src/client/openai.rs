use super::*;

use crate::utils::strip_think_tag;

use anyhow::{bail, Context, Result};
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde_json::{json, Value};

const API_BASE: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OpenAIConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub organization_id: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extensions: Option<Value>,
    pub extra: Option<ExtraConfig>,
}

impl OpenAIClient {
    config_get_fn!(api_key, get_api_key);
    config_get_fn!(api_base, get_api_base);

    pub const PROMPTS: [PromptAction<'static>; 1] = [("api_key", "API Key", None)];
}

impl_client_trait!(
    OpenAIClient,
    (
        prepare_chat_completions,
        openai_chat_completions,
        openai_chat_completions_streaming
    ),
    (prepare_embeddings, openai_embeddings),
    (noop_prepare_rerank, noop_rerank),
);

fn prepare_chat_completions(
    self_: &OpenAIClient,
    data: ChatCompletionsData,
) -> Result<RequestData> {
    let api_key = self_.get_api_key()?;
    let api_base = self_
        .get_api_base()
        .unwrap_or_else(|_| API_BASE.to_string());

    let url = format!("{}/chat/completions", api_base.trim_end_matches('/'));

    let body = openai_build_chat_completions_body(data, &self_.model);

    let mut request_data = RequestData::new(url, body);

    request_data.bearer_auth(api_key);
    if let Some(organization_id) = &self_.config.organization_id {
        request_data.header("OpenAI-Organization", organization_id);
    }

    Ok(request_data)
}

fn prepare_embeddings(self_: &OpenAIClient, data: &EmbeddingsData) -> Result<RequestData> {
    let api_key = self_.get_api_key()?;
    let api_base = self_
        .get_api_base()
        .unwrap_or_else(|_| API_BASE.to_string());

    let url = format!("{api_base}/embeddings");

    let body = openai_build_embeddings_body(data, &self_.model);

    let mut request_data = RequestData::new(url, body);

    request_data.bearer_auth(api_key);
    if let Some(organization_id) = &self_.config.organization_id {
        request_data.header("OpenAI-Organization", organization_id);
    }

    Ok(request_data)
}

pub async fn openai_chat_completions(
    builder: RequestBuilder,
    _model: &Model,
) -> Result<ChatCompletionsOutput> {
    let res = super::retry::send(builder).await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }

    debug!("non-stream-data: {data}");
    openai_extract_chat_completions(&data)
}

pub async fn openai_chat_completions_streaming(
    builder: RequestBuilder,
    handler: &mut SseHandler,
    _model: &Model,
) -> Result<()> {
    let mut call_id = String::new();
    let mut function_name = String::new();
    let mut function_arguments = String::new();
    let mut function_id = String::new();
    let mut reasoning_state = 0;
    let handle = |message: SseMmessage| -> Result<bool> {
        if message.data == "[DONE]" {
            if !function_name.is_empty() {
                if function_arguments.is_empty() {
                    function_arguments = String::from("{}");
                }
                let arguments: Value = function_arguments.parse().with_context(|| {
                    format!("Tool call '{function_name}' have non-JSON arguments '{function_arguments}'")
                })?;
                handler.tool_call(ToolCall::new(
                    function_name.clone(),
                    arguments,
                    normalize_function_id(&function_id),
                ))?;
            }
            return Ok(true);
        }
        let data: Value = serde_json::from_str(&message.data)?;
        debug!("stream-data: {data}");
        if let Some(text) = data["choices"][0]["delta"]["content"]
            .as_str()
            .filter(|v| !v.is_empty())
        {
            if reasoning_state == 1 {
                handler.text("\n</think>\n\n")?;
                reasoning_state = 0;
            }
            handler.text(text)?;
        } else if let Some(text) = data["choices"][0]["delta"]["reasoning_content"]
            .as_str()
            .or_else(|| data["choices"][0]["delta"]["reasoning"].as_str())
            .filter(|v| !v.is_empty())
        {
            if reasoning_state == 0 {
                handler.text("<think>\n")?;
                reasoning_state = 1;
            }
            handler.text(text)?;
        }
        // Capture usage from final streaming chunk if available
        if let Some(usage) = data.get("usage") {
            handler.set_usage(
                usage["prompt_tokens"].as_u64(),
                usage["completion_tokens"].as_u64(),
            );
        }
        if let (Some(function), index, id) = (
            data["choices"][0]["delta"]["tool_calls"][0]["function"].as_object(),
            data["choices"][0]["delta"]["tool_calls"][0]["index"].as_u64(),
            data["choices"][0]["delta"]["tool_calls"][0]["id"]
                .as_str()
                .filter(|v| !v.is_empty()),
        ) {
            if reasoning_state == 1 {
                handler.text("\n</think>\n\n")?;
                reasoning_state = 0;
            }
            let maybe_call_id = format!("{}/{}", id.unwrap_or_default(), index.unwrap_or_default());
            if maybe_call_id != call_id && maybe_call_id.len() >= call_id.len() {
                if !function_name.is_empty() {
                    if function_arguments.is_empty() {
                        function_arguments = String::from("{}");
                    }
                    let arguments: Value = function_arguments.parse().with_context(|| {
                        format!("Tool call '{function_name}' have non-JSON arguments '{function_arguments}'")
                    })?;
                    handler.tool_call(ToolCall::new(
                        function_name.clone(),
                        arguments,
                        normalize_function_id(&function_id),
                    ))?;
                }
                function_name.clear();
                function_arguments.clear();
                function_id.clear();
                call_id = maybe_call_id;
            }
            if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                if name.starts_with(&function_name) {
                    function_name = name.to_string();
                } else {
                    function_name.push_str(name);
                }
            }
            if let Some(arguments) = function.get("arguments").and_then(|v| v.as_str()) {
                function_arguments.push_str(arguments);
            }
            if let Some(id) = id {
                function_id = id.to_string();
            }
        }
        Ok(false)
    };

    sse_stream(builder, handle).await
}

pub async fn openai_embeddings(
    builder: RequestBuilder,
    _model: &Model,
) -> Result<EmbeddingsOutput> {
    let res = super::retry::send(builder).await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    let res_body: EmbeddingsResBody =
        serde_json::from_value(data).context("Invalid embeddings data")?;
    let output = res_body.data.into_iter().map(|v| v.embedding).collect();
    Ok(output)
}

#[derive(Deserialize)]
struct EmbeddingsResBody {
    data: Vec<EmbeddingsResBodyEmbedding>,
}

#[derive(Deserialize)]
struct EmbeddingsResBodyEmbedding {
    embedding: Vec<f32>,
}

pub fn openai_build_chat_completions_body(data: ChatCompletionsData, model: &Model) -> Value {
    let ChatCompletionsData {
        messages,
        temperature,
        top_p,
        functions,
        stream,
        output_schema,
        extensions,
    } = data;

    let messages_len = messages.len();
    let messages: Vec<Value> = messages
        .into_iter()
        .enumerate()
        .flat_map(|(i, message)| {
            let Message { role, content } = message;
            match content {
                MessageContent::ToolCalls(MessageContentToolCalls {
                    tool_results,
                    text: _,
                    sequence,
                }) => {
                    if !sequence {
                        let tool_calls: Vec<_> = tool_results
                            .iter()
                            .map(|tool_result| {
                                json!({
                                    "id": tool_result.call.id,
                                    "type": "function",
                                    "function": {
                                        "name": tool_result.call.name,
                                        "arguments": tool_result.call.arguments.to_string(),
                                    },
                                })
                            })
                            .collect();
                        let mut messages = vec![
                            json!({ "role": MessageRole::Assistant, "tool_calls": tool_calls }),
                        ];
                        for tool_result in tool_results {
                            messages.push(json!({
                                "role": "tool",
                                "content": tool_result.output.to_string(),
                                "tool_call_id": tool_result.call.id,
                            }));
                        }
                        messages
                    } else {
                        tool_results.into_iter().flat_map(|tool_result| {
                            vec![
                                json!({
                                    "role": MessageRole::Assistant,
                                    "tool_calls": [
                                        {
                                            "id": tool_result.call.id,
                                            "type": "function",
                                            "function": {
                                                "name": tool_result.call.name,
                                                "arguments": tool_result.call.arguments.to_string(),
                                            },
                                        }
                                    ]
                                }),
                                json!({
                                    "role": "tool",
                                    "content": tool_result.output.to_string(),
                                    "tool_call_id": tool_result.call.id,
                                })
                            ]

                        }).collect()
                    }
                }
                MessageContent::Text(text) if role.is_assistant() && i != messages_len - 1 => {
                    vec![json!({ "role": role, "content": strip_think_tag(&text) }
                    )]
                }
                _ => vec![json!({ "role": role, "content": content })],
            }
        })
        .collect();

    let mut body = json!({
        "model": &model.real_name(),
        "messages": messages,
    });

    if let Some(v) = model.max_tokens_param() {
        if model
            .patch()
            .and_then(|v| v.get("body").and_then(|v| v.get("max_tokens")))
            == Some(&Value::Null)
        {
            body["max_completion_tokens"] = v.into();
        } else {
            body["max_tokens"] = v.into();
        }
    }
    if let Some(v) = temperature {
        body["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        body["top_p"] = v.into();
    }
    if stream {
        body["stream"] = true.into();
    }
    if let Some(functions) = functions {
        body["tools"] = functions
            .iter()
            .map(|v| {
                json!({
                    "type": "function",
                    "function": v,
                })
            })
            .collect();
    }

    // Phase 9A: provider-native structured output. When the model declares
    // `supports_response_format_json_schema` and the role has an `output_schema`,
    // use OpenAI's `response_format: json_schema` so conformance is enforced by
    // the API rather than by a prompt-injected instruction. The prompt suffix is
    // stripped upstream in `Input::prepare_completion_data` to avoid paying for
    // it twice.
    if let Some(schema) = output_schema {
        if model.data().supports_response_format_json_schema {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "output",
                    "strict": true,
                    "schema": schema,
                }
            });
        }
    }

    if let Some(extensions) = extensions {
        json_patch::merge(&mut body, &extensions);
    }

    if let Some(extensions) = model.extensions() {
        json_patch::merge(&mut body, extensions);
    }

    body
}

pub fn openai_build_embeddings_body(data: &EmbeddingsData, model: &Model) -> Value {
    json!({
        "input": data.texts,
        "model": model.real_name()
    })
}

pub fn openai_extract_chat_completions(data: &Value) -> Result<ChatCompletionsOutput> {
    let text = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();

    let reasoning = data["choices"][0]["message"]["reasoning_content"]
        .as_str()
        .or_else(|| data["choices"][0]["message"]["reasoning"].as_str())
        .unwrap_or_default()
        .trim();

    let mut tool_calls = vec![];
    if let Some(calls) = data["choices"][0]["message"]["tool_calls"].as_array() {
        for call in calls {
            if let (Some(name), Some(arguments), Some(id)) = (
                call["function"]["name"].as_str(),
                call["function"]["arguments"].as_str(),
                call["id"].as_str(),
            ) {
                let arguments: Value = arguments.parse().with_context(|| {
                    format!("Tool call '{name}' have non-JSON arguments '{arguments}'")
                })?;
                tool_calls.push(ToolCall::new(
                    name.to_string(),
                    arguments,
                    Some(id.to_string()),
                ));
            }
        }
    };

    if text.is_empty() && tool_calls.is_empty() {
        bail!("Invalid response data: {data}");
    }
    let text = if !reasoning.is_empty() {
        format!("<think>\n{reasoning}\n</think>\n\n{text}")
    } else {
        text.to_string()
    };
    let output = ChatCompletionsOutput {
        text,
        tool_calls,
        id: data["id"].as_str().map(|v| v.to_string()),
        input_tokens: data["usage"]["prompt_tokens"].as_u64(),
        output_tokens: data["usage"]["completion_tokens"].as_u64(),
    };
    Ok(output)
}

fn normalize_function_id(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Message, MessageContent, MessageRole};
    use crate::function::FunctionDeclaration;

    fn openai_model(native_schema: bool) -> Model {
        let mut m = Model::new("openai", "gpt-test");
        m.data_mut().supports_function_calling = true;
        m.data_mut().supports_response_format_json_schema = native_schema;
        m
    }

    fn user_msg(text: &str) -> Message {
        Message::new(MessageRole::User, MessageContent::Text(text.into()))
    }

    fn sample_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "answer": {"type": "string"},
                "confidence": {"type": "number"}
            },
            "required": ["answer", "confidence"]
        })
    }

    fn data_with_schema(
        schema: Option<Value>,
        functions: Option<Vec<FunctionDeclaration>>,
    ) -> ChatCompletionsData {
        ChatCompletionsData {
            messages: vec![user_msg("hi")],
            temperature: None,
            top_p: None,
            functions,
            stream: false,
            output_schema: schema,
            extensions: None,
        }
    }

    #[test]
    fn body_injects_response_format_when_native_schema_active() {
        let model = openai_model(true);
        let schema = sample_schema();
        let data = data_with_schema(Some(schema.clone()), None);
        let body = openai_build_chat_completions_body(data, &model);

        let rf = body.get("response_format").expect("response_format present");
        assert_eq!(rf["type"], "json_schema");
        assert_eq!(rf["json_schema"]["name"], "output");
        assert_eq!(rf["json_schema"]["strict"], true);
        assert_eq!(rf["json_schema"]["schema"], schema);
    }

    #[test]
    fn body_omits_response_format_when_capability_off() {
        let model = openai_model(false);
        let data = data_with_schema(Some(sample_schema()), None);
        let body = openai_build_chat_completions_body(data, &model);
        assert!(
            body.get("response_format").is_none(),
            "no response_format when model doesn't support it"
        );
    }

    #[test]
    fn body_omits_response_format_when_schema_missing() {
        let model = openai_model(true);
        let data = data_with_schema(None, None);
        let body = openai_build_chat_completions_body(data, &model);
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn body_preserves_tools_alongside_response_format() {
        let model = openai_model(true);
        let existing = vec![FunctionDeclaration {
            name: "get_weather".into(),
            description: "lookup weather".into(),
            parameters: json!({"type": "object", "properties": {}}),
            agent: false,
            source: Default::default(),
            examples: None,
            timeout: None,
        }];
        let data = data_with_schema(Some(sample_schema()), Some(existing));
        let body = openai_build_chat_completions_body(data, &model);

        assert!(body.get("response_format").is_some());
        let tools = body["tools"].as_array().expect("tools array present");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "get_weather");
    }

    #[test]
    fn body_merges_model_level_extensions() {
        let mut model = openai_model(false);
        model.extensions_mut().replace(json!({
            "num_ctx": 4096,
            "custom_flag": true
        }));

        let data = data_with_schema(None, None);
        let body = openai_build_chat_completions_body(data, &model);

        assert_eq!(body["num_ctx"], 4096);
        assert_eq!(body["custom_flag"], true);
        assert_eq!(body["model"], "gpt-test");
    }

    #[test]
    fn body_merges_client_level_extensions() {
        let model = openai_model(false);
        let mut data = data_with_schema(None, None);
        data.extensions = Some(json!({
            "num_ctx": 2048,
            "repeat_penalty": 1.1,
        }));
        let body = openai_build_chat_completions_body(data, &model);

        assert_eq!(body["num_ctx"], 2048);
        assert_eq!(body["repeat_penalty"], 1.1);
    }

    #[test]
    fn model_extensions_override_client_extensions() {
        let mut model = openai_model(false);
        model.extensions_mut().replace(json!({
            "num_ctx": 32768,
            "top_k": 50,
        }));
        let mut data = data_with_schema(None, None);
        data.extensions = Some(json!({
            "num_ctx": 4096,
            "repeat_penalty": 1.1,
        }));
        let body = openai_build_chat_completions_body(data, &model);

        // Model-level wins on overlap; client-level fills the rest.
        assert_eq!(body["num_ctx"], 32768);
        assert_eq!(body["top_k"], 50);
        assert_eq!(body["repeat_penalty"], 1.1);
    }
}
