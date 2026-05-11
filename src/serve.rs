use crate::{client::*, config::*, function::*, rag::*, utils::*};

use anyhow::{anyhow, bail, Result};
use bytes::Bytes;
use chrono::{Timelike, Utc};
use futures_util::StreamExt;
use http::{Method, Response, StatusCode};
use http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody};
use hyper::{
    body::{Frame, Incoming},
    service::service_fn,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use parking_lot::RwLock;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    net::IpAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{
    net::TcpListener,
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        oneshot,
    },
};
use tokio_graceful::Shutdown;
use tokio_stream::wrappers::UnboundedReceiverStream;

const DEFAULT_MODEL_NAME: &str = "default";
const PLAYGROUND_HTML: &[u8] = include_bytes!("../assets/playground.html");
const ARENA_HTML: &[u8] = include_bytes!("../assets/arena.html");

type AppResponse = Response<BoxBody<Bytes, Infallible>>;

pub async fn run(config: GlobalConfig, addr: Option<String>) -> Result<()> {
    let addr = resolve_addr(&config, addr);
    let listener = TcpListener::bind(&addr).await?;
    let stop_server = run_on(listener, &config).await?;
    println!("Chat Completions API: http://{addr}/v1/chat/completions");
    println!("Models API:           http://{addr}/v1/models");
    println!("Roles API:            http://{addr}/v1/roles");
    println!("Prompts API:          http://{addr}/v1/prompts");
    println!("Embeddings API:       http://{addr}/v1/embeddings");
    println!("Rerank API:           http://{addr}/v1/rerank");
    println!("LLM Playground:       http://{addr}/playground");
    println!("LLM Arena:            http://{addr}/arena?num=2");
    shutdown_signal().await;
    let _ = stop_server.send(());
    Ok(())
}

/// Resolve an optional `--serve` address argument to a concrete `host:port`.
///
/// - A bare integer (`8000`) binds to `127.0.0.1:8000`.
/// - A bare IP (`0.0.0.0`) uses port 8000.
/// - Anything else is passed through.
/// - `None` falls back to the configured `serve_addr`.
pub fn resolve_addr(config: &GlobalConfig, addr: Option<String>) -> String {
    match addr {
        Some(addr) => {
            if let Ok(port) = addr.parse::<u16>() {
                format!("127.0.0.1:{port}")
            } else if let Ok(ip) = addr.parse::<IpAddr>() {
                format!("{ip}:8000")
            } else {
                addr
            }
        }
        None => config.read().serve_addr(),
    }
}

/// Start the HTTP server on a pre-bound listener and return the shutdown
/// handle. Unlike [`run`], this does not print the URL banner and does not
/// block on a shutdown signal — the caller owns the lifetime.
///
/// Used by the REPL launcher (`src/repl/pi.rs`) to bring the server up on
/// an ephemeral port in-process while pi handles the terminal.
pub async fn run_on(
    listener: TcpListener,
    config: &GlobalConfig,
) -> Result<oneshot::Sender<()>> {
    let server = Arc::new(Server::new(config));
    server.run(listener).await
}

struct Server {
    /// Shared, live configuration. Phase 2 introduces the bridge endpoints
    /// (`/v1/state/*`) which mutate this lock so subsequent chat completions
    /// observe the new role / agent / session / rag. The CLI `--serve` path
    /// continues to behave identically because nothing else writes to it.
    config: GlobalConfig,
    /// Bridge token sourced from `AICHAT_BRIDGE_TOKEN` at server start.
    /// When `None`, `/v1/state/*` routes 404 (CLI `--serve` users never see
    /// them). When `Some`, requests must carry `Authorization: Bearer <tok>`.
    bridge_token: Option<String>,
    models: Vec<Value>,
    roles: Vec<Role>,
    prompts: Vec<Prompt>,
    rags: Vec<String>,
}

impl Server {
    fn new(config: &GlobalConfig) -> Self {
        // Snapshot enough of the config at boot to populate the static
        // listings the OpenAI-compatible surface exposes. The live lock
        // (`self.config`) is what state-mutating bridge endpoints touch.
        let snapshot = config.read().clone();
        let mut models = list_all_models(&snapshot);
        let mut default_model = snapshot.model.clone();
        default_model.data_mut().name = DEFAULT_MODEL_NAME.into();
        models.insert(0, &default_model);
        let models: Vec<Value> = models
            .into_iter()
            .enumerate()
            .map(|(i, model)| {
                let id = if i == 0 {
                    DEFAULT_MODEL_NAME.into()
                } else {
                    model.id()
                };
                let mut value = json!(model.data());
                if let Some(value_obj) = value.as_object_mut() {
                    value_obj.insert("id".into(), id.into());
                    value_obj.insert("object".into(), "model".into());
                    value_obj.insert("owned_by".into(), model.client_name().into());
                    value_obj.remove("name");
                }
                value
            })
            .collect();
        Self {
            config: config.clone(),
            bridge_token: std::env::var("AICHAT_BRIDGE_TOKEN").ok(),
            models,
            prompts: Config::all_prompts(),
            roles: Config::all_roles(),
            rags: Config::list_rags(),
        }
    }

    /// Snapshot the current live config. Existing handlers clone the inner
    /// `Config` at this point to preserve their per-request immutability
    /// semantics; bridge state writes don't tear an in-flight request.
    fn config_snapshot(&self) -> Config {
        let mut snap = self.config.read().clone();
        // Match the historical Server::new behavior: don't expose the
        // configured function set to incoming /v1/chat/completions calls;
        // tool routing on the server is opt-in via the OpenAI `tools` field.
        snap.functions = Functions::default();
        snap
    }

    async fn run(self: Arc<Self>, listener: TcpListener) -> Result<oneshot::Sender<()>> {
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let shutdown = Shutdown::new(async { rx.await.unwrap_or_default() });
            let guard = shutdown.guard_weak();

            loop {
                tokio::select! {
                    res = listener.accept() => {
                        let Ok((cnx, _)) = res else {
                            continue;
                        };

                        let stream = TokioIo::new(cnx);
                        let server = self.clone();
                        shutdown.spawn_task(async move {
                            let hyper_service = service_fn(move |request: hyper::Request<Incoming>| {
                                server.clone().handle(request)
                            });
                            let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                                .serve_connection_with_upgrades(stream, hyper_service)
                                .await;
                        });
                    }
                    _ = guard.cancelled() => {
                        break;
                    }
                }
            }
        });
        Ok(tx)
    }

    async fn handle(
        self: Arc<Self>,
        req: hyper::Request<Incoming>,
    ) -> std::result::Result<AppResponse, hyper::Error> {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let request_origin = req
            .headers()
            .get(hyper::header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let path = uri.path();

        if method == Method::OPTIONS {
            let mut res = Response::default();
            *res.status_mut() = StatusCode::NO_CONTENT;
            set_cors_header(&mut res, request_origin.as_deref());
            return Ok(res);
        }

        // Phase 2 bridge surface: `/v1/state/*` mutates the live config so
        // pi-side slash commands (defined in pi-extensions/) take effect for
        // subsequent /v1/chat/completions on the same server. Gated by a
        // per-launch bearer token; absent CLI `--serve` users never see
        // these routes, they just 404 like any unknown path.
        if path.starts_with("/v1/state/") {
            let mut res = self
                .handle_bridge(&method, path, req)
                .await
                .unwrap_or_else(ret_err);
            set_cors_header(&mut res, request_origin.as_deref());
            info!("{method} {uri} {}", res.status().as_u16());
            return Ok(res);
        }

        let mut status = StatusCode::OK;
        let res = if path == "/v1/chat/completions" {
            self.chat_completions(req).await
        } else if path == "/v1/embeddings" {
            self.embeddings(req).await
        } else if path == "/v1/rerank" {
            self.rerank(req).await
        } else if path == "/v1/models" {
            self.list_models()
        } else if path == "/v1/roles" {
            self.list_roles()
        } else if path == "/v1/prompts" {
            self.list_prompts()
        } else if path == "/v1/rags" {
            self.list_rags()
        } else if path == "/v1/rags/search" {
            self.search_rag(req).await
        } else if path == "/playground" || path == "/playground.html" {
            self.playground_page()
        } else if path == "/arena" || path == "/arena.html" {
            self.arena_page()

    } else {
            status = StatusCode::NOT_FOUND;
            Err(anyhow!("Not Found"))
        };
        let mut res = match res {
            Ok(res) => {
                info!("{method} {uri} {}", status.as_u16());
                res
            }
            Err(err) => {
                if status == StatusCode::OK {
                    status = StatusCode::BAD_REQUEST;
                }
                error!("{method} {uri} {} {err}", status.as_u16());
                ret_err(err)
            }
        };
        *res.status_mut() = status;
        set_cors_header(&mut res, request_origin.as_deref());
        Ok(res)
    }

    fn playground_page(&self) -> Result<AppResponse> {
        let res = Response::builder()
            .header("Content-Type", "text/html; charset=utf-8")
            .body(Full::new(Bytes::from(PLAYGROUND_HTML)).boxed())?;
        Ok(res)
    }

    fn arena_page(&self) -> Result<AppResponse> {
        let res = Response::builder()
            .header("Content-Type", "text/html; charset=utf-8")
            .body(Full::new(Bytes::from(ARENA_HTML)).boxed())?;
        Ok(res)
    }

    fn list_models(&self) -> Result<AppResponse> {
        let data = json!({ "data": self.models });
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .body(Full::new(Bytes::from(data.to_string())).boxed())?;
        Ok(res)
    }

    fn list_roles(&self) -> Result<AppResponse> {
        let data = json!({ "data": self.roles });
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .body(Full::new(Bytes::from(data.to_string())).boxed())?;
        Ok(res)
    }

    fn list_prompts(&self) -> Result<AppResponse> {
        let data = json!({ "data": self.prompts });
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .body(Full::new(Bytes::from(data.to_string())).boxed())?;
        Ok(res)
    }

    fn list_rags(&self) -> Result<AppResponse> {
        let data = json!({ "data": self.rags });
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .body(Full::new(Bytes::from(data.to_string())).boxed())?;
        Ok(res)
    }

    async fn search_rag(&self, req: hyper::Request<Incoming>) -> Result<AppResponse> {
        let req_body = req.collect().await?.to_bytes();
        let req_body: Value = serde_json::from_slice(&req_body)
            .map_err(|err| anyhow!("Invalid request json, {err}"))?;

        debug!("search rag request: {req_body}");
        let SearchRagReqBody { name, input } = serde_json::from_value(req_body)
            .map_err(|err| anyhow!("Invalid request body, {err}"))?;

        let config = Arc::new(RwLock::new(self.config_snapshot()));

        let abort_signal = create_abort_signal();

        let rag_path = config.read().rag_file(&name);
        let rag = Rag::load(&config, &name, &rag_path)?;

        let rag_result = Config::search_rag(&config, &rag, &input, abort_signal).await?;

        let data = json!({ "data": rag_result });
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .body(Full::new(Bytes::from(data.to_string())).boxed())?;
        Ok(res)
    }

    async fn chat_completions(&self, req: hyper::Request<Incoming>) -> Result<AppResponse> {
        let req_body = req.collect().await?.to_bytes();
        let req_body: Value = serde_json::from_slice(&req_body)
            .map_err(|err| anyhow!("Invalid request json, {err}"))?;

        debug!("chat completions request: {req_body}");
        let req_body = serde_json::from_value(req_body)
            .map_err(|err| anyhow!("Invalid request body, {err}"))?;

        let ChatCompletionsReqBody {
            model,
            messages,
            temperature,
            top_p,
            max_tokens,
            stream,
            tools,
        } = req_body;

        let mut messages =
            parse_messages(messages).map_err(|err| anyhow!("Invalid request body, {err}"))?;

        let functions = parse_tools(tools).map_err(|err| anyhow!("Invalid request body, {err}"))?;

        let config = self.config_snapshot();

        let default_model = config.model.clone();

        let config = Arc::new(RwLock::new(config));

        let (model_name, change) = if model == DEFAULT_MODEL_NAME {
            (default_model.id(), true)
        } else if default_model.id() == model {
            (model, false)
        } else {
            (model, true)
        };

        if change {
            config.write().set_model(&model_name)?;
        }

        let mut client = init_client(&config, None)?;
        if max_tokens.is_some() {
            client.model_mut().set_max_tokens(max_tokens, true);
        }
        let abort_signal = create_abort_signal();
        let http_client = client.build_client()?;

        let completion_id = generate_completion_id();
        let created = Utc::now().timestamp();

        patch_messages(&mut messages, client.model());

        let data: ChatCompletionsData = ChatCompletionsData {
            messages,
            temperature,
            top_p,
            functions,
            stream,
            output_schema: None,
            extensions: None,
        };

        if stream {
            let (tx, mut rx) = unbounded_channel();
            tokio::spawn(async move {
                let is_first = Arc::new(AtomicBool::new(true));
                let (sse_tx, sse_rx) = unbounded_channel();
                let mut handler = SseHandler::new(sse_tx, abort_signal);
                async fn map_event(
                    mut sse_rx: UnboundedReceiver<SseEvent>,
                    tx: &UnboundedSender<ResEvent>,
                    is_first: Arc<AtomicBool>,
                ) {
                    while let Some(reply_event) = sse_rx.recv().await {
                        if is_first.load(Ordering::SeqCst) {
                            let _ = tx.send(ResEvent::First(None));
                            is_first.store(false, Ordering::SeqCst)
                        }
                        match reply_event {
                            SseEvent::Text(text) => {
                                let _ = tx.send(ResEvent::Text(text));
                            }
                            SseEvent::Done => {
                                let _ = tx.send(ResEvent::Done);
                                sse_rx.close();
                            }
                        }
                    }
                }
                async fn chat_completions(
                    client: &dyn Client,
                    http_client: &reqwest::Client,
                    handler: &mut SseHandler,
                    mut data: ChatCompletionsData,
                    tx: &UnboundedSender<ResEvent>,
                    is_first: Arc<AtomicBool>,
                ) {
                    if client.model().no_stream() {
                        data.stream = false;
                        let ret = client.chat_completions_inner(http_client, data).await;
                        match ret {
                            Ok(output) => {
                                let ChatCompletionsOutput {
                                    text, tool_calls, ..
                                } = output;
                                let _ = tx.send(ResEvent::First(None));
                                is_first.store(false, Ordering::SeqCst);
                                let _ = tx.send(ResEvent::Text(text));
                                if !tool_calls.is_empty() {
                                    let _ = tx.send(ResEvent::ToolCalls(tool_calls));
                                }
                            }
                            Err(err) => {
                                let _ = tx.send(ResEvent::First(Some(format!("{err:?}"))));
                                is_first.store(false, Ordering::SeqCst)
                            }
                        };
                    } else {
                        let ret = client
                            .chat_completions_streaming_inner(http_client, handler, data)
                            .await;
                        let first = match ret {
                            Ok(()) => None,
                            Err(err) => Some(format!("{err:?}")),
                        };
                        if is_first.load(Ordering::SeqCst) {
                            let _ = tx.send(ResEvent::First(first));
                            is_first.store(false, Ordering::SeqCst)
                        }
                        let tool_calls = handler.tool_calls().to_vec();
                        if !tool_calls.is_empty() {
                            let _ = tx.send(ResEvent::ToolCalls(tool_calls));
                        }
                    }
                    handler.done();
                }
                tokio::join!(
                    map_event(sse_rx, &tx, is_first.clone()),
                    chat_completions(
                        client.as_ref(),
                        &http_client,
                        &mut handler,
                        data,
                        &tx,
                        is_first
                    ),
                );
            });

            let first_event = rx.recv().await;

            if let Some(ResEvent::First(Some(err))) = first_event {
                bail!("{err}");
            }

            let shared: Arc<(String, String, i64, AtomicBool)> =
                Arc::new((completion_id, model_name, created, AtomicBool::new(false)));
            let stream = UnboundedReceiverStream::new(rx);
            let stream = stream.filter_map(move |res_event| {
                let shared = shared.clone();
                async move {
                    let (completion_id, model, created, has_tool_calls) = shared.as_ref();
                    match res_event {
                        ResEvent::Text(text) => {
                            Some(Ok(create_text_frame(completion_id, model, *created, &text)))
                        }
                        ResEvent::ToolCalls(tool_calls) => {
                            has_tool_calls.store(true, Ordering::SeqCst);
                            Some(Ok(create_tool_calls_frame(
                                completion_id,
                                model,
                                *created,
                                &tool_calls,
                            )))
                        }
                        ResEvent::Done => Some(Ok(create_done_frame(
                            completion_id,
                            model,
                            *created,
                            has_tool_calls.load(Ordering::SeqCst),
                        ))),
                        _ => None,
                    }
                }
            });
            let res = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/event-stream")
                .header("Cache-Control", "no-cache")
                .header("Connection", "keep-alive")
                .body(BodyExt::boxed(StreamBody::new(stream)))?;
            Ok(res)
        } else {
            let output = client.chat_completions_inner(&http_client, data).await?;
            let res = Response::builder()
                .header("Content-Type", "application/json")
                .body(
                    Full::new(ret_non_stream(
                        &completion_id,
                        &model_name,
                        created,
                        &output,
                    ))
                    .boxed(),
                )?;
            Ok(res)
        }
    }

    async fn embeddings(&self, req: hyper::Request<Incoming>) -> Result<AppResponse> {
        let req_body = req.collect().await?.to_bytes();
        let req_body: Value = serde_json::from_slice(&req_body)
            .map_err(|err| anyhow!("Invalid request json, {err}"))?;

        debug!("embeddings request: {req_body}");
        let req_body = serde_json::from_value(req_body)
            .map_err(|err| anyhow!("Invalid request body, {err}"))?;

        let EmbeddingsReqBody {
            input,
            model: embedding_model_id,
        } = req_body;

        let config = Arc::new(RwLock::new(self.config_snapshot()));

        let embedding_model =
            Model::retrieve_model(&config.read(), &embedding_model_id, ModelType::Embedding)?;

        let texts = match input {
            EmbeddingsReqBodyInput::Single(v) => vec![v],
            EmbeddingsReqBodyInput::Multiple(v) => v,
        };
        let client = init_client(&config, Some(embedding_model))?;
        let data = client
            .embeddings(&EmbeddingsData {
                query: false,
                texts,
            })
            .await?;
        let data: Vec<_> = data
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                json!({
                        "object": "embedding",
                        "embedding": v,
                        "index": i,
                })
            })
            .collect();
        let output = json!({
            "object": "list",
            "data": data,
            "model": embedding_model_id,
            "usage": {
                "prompt_tokens": 0,
                "total_tokens": 0,
            }
        });
        let res = Response::builder()
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(output.to_string())).boxed())?;
        Ok(res)
    }

    async fn rerank(&self, req: hyper::Request<Incoming>) -> Result<AppResponse> {
        let req_body = req.collect().await?.to_bytes();
        let req_body: Value = serde_json::from_slice(&req_body)
            .map_err(|err| anyhow!("Invalid request json, {err}"))?;

        debug!("rerank request: {req_body}");
        let req_body = serde_json::from_value(req_body)
            .map_err(|err| anyhow!("Invalid request body, {err}"))?;

        let RerankReqBody {
            model: reranker_model_id,
            documents,
            query,
            top_n,
        } = req_body;

        let top_n = top_n.unwrap_or(documents.len());

        let config = Arc::new(RwLock::new(self.config_snapshot()));

        let reranker_model =
            Model::retrieve_model(&config.read(), &reranker_model_id, ModelType::Reranker)?;

        let client = init_client(&config, Some(reranker_model))?;
        let data = client
            .rerank(&RerankData {
                query,
                documents: documents.clone(),
                top_n,
            })
            .await?;

        let results: Vec<_> = data
            .into_iter()
            .map(|v| {
                json!({
                    "index": v.index,
                    "relevance_score": v.relevance_score,
                    "document": documents.get(v.index).map(|v| json!(v)).unwrap_or_default(),
                })
            })
            .collect();
        let output = json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "results": results,
        });
        let res = Response::builder()
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(output.to_string())).boxed())?;
        Ok(res)
    }

    /// Bridge entry: route `/v1/state/*` after authenticating with the
    /// bearer token minted at launch by `src/repl/pi.rs`. Returns a fully
    /// formed `AppResponse` (with status code) so the outer dispatcher can
    /// stamp CORS and log without re-deriving the status.
    ///
    /// Sentinel responses (401, 403, 404, 405) come back as `Ok(...)`; only
    /// internal failures bubble up as `Err`.
    async fn handle_bridge(
        self: &Arc<Self>,
        method: &Method,
        path: &str,
        req: hyper::Request<Incoming>,
    ) -> Result<AppResponse> {
        let token = match &self.bridge_token {
            Some(t) => t.clone(),
            // No token configured (CLI `--serve` mode): pretend the route
            // doesn't exist. Avoids leaking the route surface to operators
            // who didn't opt in to the bridge.
            None => return bridge_status_response(StatusCode::NOT_FOUND, "Not Found"),
        };
        let provided = req
            .headers()
            .get(hyper::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .unwrap_or("");
        if !constant_time_eq(provided.as_bytes(), token.as_bytes()) {
            return bridge_status_response(StatusCode::UNAUTHORIZED, "Unauthorized");
        }

        // Route table. Method gating is per-endpoint because GET/POST
        // semantics diverge sharply between read and mutate operations.
        match (method, path) {
            (&Method::GET, "/v1/state/info") => self.state_info(req).await,
            (&Method::POST, "/v1/state/role") => self.state_role(req).await,
            (&Method::POST, "/v1/state/session") => self.state_session(req).await,
            (&Method::POST, "/v1/state/rag") => self.state_rag(req).await,
            (&Method::POST, "/v1/state/agent") => self.state_agent(req).await,
            (&Method::POST, "/v1/state/exit-context") => self.state_exit_context(req).await,
            (&Method::POST, "/v1/state/macro") => self.state_macro(req).await,
            (_, "/v1/state/info")
            | (_, "/v1/state/role")
            | (_, "/v1/state/session")
            | (_, "/v1/state/rag")
            | (_, "/v1/state/agent")
            | (_, "/v1/state/exit-context")
            | (_, "/v1/state/macro") => {
                bridge_status_response(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed")
            }
            _ => bridge_status_response(StatusCode::NOT_FOUND, "Not Found"),
        }
    }

    /// `GET /v1/state/info?of=role|agent|session|rag` — returns the same
    /// rendered text the legacy REPL emitted via `.info`. Without `?of=`,
    /// returns the implicit context Config::info chooses.
    async fn state_info(self: &Arc<Self>, req: hyper::Request<Incoming>) -> Result<AppResponse> {
        let of = query_param(&req, "of");
        let cfg = self.config.read();
        let text = match of.as_deref() {
            None => cfg.info()?,
            Some("role") => cfg.role_info()?,
            Some("agent") => cfg.agent_info()?,
            Some("session") => match &cfg.session {
                Some(s) => s.export()?,
                None => bail!("No session"),
            },
            Some("rag") => match &cfg.rag {
                Some(r) => r.export()?,
                None => bail!("No rag"),
            },
            Some(other) => bail!("Unknown info kind: {other}"),
        };
        Ok(json_response(StatusCode::OK, json!({ "info": text })))
    }

    /// `POST /v1/state/role` body: `{"name": "<role>"}`
    /// Mirrors `.role <name>` in the legacy REPL.
    async fn state_role(self: &Arc<Self>, req: hyper::Request<Incoming>) -> Result<AppResponse> {
        #[derive(Deserialize)]
        struct Body {
            name: String,
        }
        let body: Body = read_json_body(req).await?;
        self.config.write().use_role(&body.name)?;
        Ok(json_response(
            StatusCode::OK,
            json!({ "ok": true, "kind": "role", "name": body.name }),
        ))
    }

    /// `POST /v1/state/session` body: `{"name": "<session>"}` (name optional)
    /// Mirrors `.session [name]` in the legacy REPL.
    async fn state_session(
        self: &Arc<Self>,
        req: hyper::Request<Incoming>,
    ) -> Result<AppResponse> {
        #[derive(Deserialize, Default)]
        struct Body {
            #[serde(default)]
            name: Option<String>,
        }
        let body: Body = read_json_body(req).await?;
        self.config.write().use_session(body.name.as_deref())?;
        Ok(json_response(
            StatusCode::OK,
            json!({ "ok": true, "kind": "session", "name": body.name }),
        ))
    }

    /// `POST /v1/state/rag` body: `{"name": "<rag>"}` (name optional → temp).
    /// Mirrors `.rag [name]` in the legacy REPL. Uses `Config::use_rag` which
    /// is async and takes a `GlobalConfig`; we already hold one in `self`.
    async fn state_rag(self: &Arc<Self>, req: hyper::Request<Incoming>) -> Result<AppResponse> {
        #[derive(Deserialize, Default)]
        struct Body {
            #[serde(default)]
            name: Option<String>,
        }
        let body: Body = read_json_body(req).await?;
        let abort = create_abort_signal();
        Config::use_rag(&self.config, body.name.as_deref(), abort).await?;
        Ok(json_response(
            StatusCode::OK,
            json!({ "ok": true, "kind": "rag", "name": body.name }),
        ))
    }

    /// `POST /v1/state/agent` body: `{"name": "<agent>", "session": "<sess>"}`
    /// The legacy REPL also accepts agent variables — supported via
    /// `"variables": {"k": "v"}`. For Phase 2 we just bind the agent;
    /// per-invocation variable threading is a follow-up.
    async fn state_agent(self: &Arc<Self>, req: hyper::Request<Incoming>) -> Result<AppResponse> {
        #[derive(Deserialize)]
        struct Body {
            name: String,
            #[serde(default)]
            session: Option<String>,
        }
        let body: Body = read_json_body(req).await?;
        let abort = create_abort_signal();
        Config::use_agent(&self.config, &body.name, body.session.as_deref(), abort).await?;
        Ok(json_response(
            StatusCode::OK,
            json!({ "ok": true, "kind": "agent", "name": body.name }),
        ))
    }

    /// `POST /v1/state/exit-context` body: `{"kind": "role|agent|session|rag"}`
    /// Mirrors `.exit <kind>` in the legacy REPL.
    async fn state_exit_context(
        self: &Arc<Self>,
        req: hyper::Request<Incoming>,
    ) -> Result<AppResponse> {
        #[derive(Deserialize)]
        struct Body {
            kind: String,
        }
        let body: Body = read_json_body(req).await?;
        match body.kind.as_str() {
            "role" => self.config.write().exit_role()?,
            "session" => self.config.write().exit_session()?,
            "rag" => self.config.write().exit_rag()?,
            "agent" => self.config.write().exit_agent()?,
            other => bail!("Unknown exit context kind: {other}"),
        }
        Ok(json_response(
            StatusCode::OK,
            json!({ "ok": true, "exited": body.kind }),
        ))
    }

    /// `POST /v1/state/macro` body: `{"name": "<macro>", "text": "<optional>"}`
    /// Mirrors `.macro <name>` in the legacy REPL. Returns the macro's
    /// recorded output (empty string if the macro produced no text turn).
    async fn state_macro(self: &Arc<Self>, req: hyper::Request<Incoming>) -> Result<AppResponse> {
        #[derive(Deserialize)]
        struct Body {
            name: String,
            #[serde(default)]
            text: Option<String>,
        }
        let body: Body = read_json_body(req).await?;
        let abort = create_abort_signal();
        macro_execute(&self.config, &body.name, body.text.as_deref(), abort).await?;
        let last = self
            .config
            .read()
            .last_message
            .as_ref()
            .map(|m| m.output.clone())
            .unwrap_or_default();
        Ok(json_response(
            StatusCode::OK,
            json!({ "ok": true, "kind": "macro", "name": body.name, "output": last }),
        ))
    }
}

/// Read an optional query-string parameter from the request URI. Returns
/// `None` if absent or empty. Trivial loop avoids pulling in another dep.
fn query_param(req: &hyper::Request<Incoming>, key: &str) -> Option<String> {
    req.uri().query().and_then(|q| {
        q.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            if k == key {
                Some(urlencoding::decode(v).ok()?.into_owned())
            } else {
                None
            }
        })
    })
}

/// Collect the request body, decode as JSON, and deserialize. Errors map
/// to `anyhow::Error` so they surface as 400 with a helpful message.
async fn read_json_body<T: for<'de> Deserialize<'de>>(
    req: hyper::Request<Incoming>,
) -> Result<T> {
    let bytes = req.collect().await?.to_bytes();
    if bytes.is_empty() {
        // Allow empty body when T can deserialize from `{}` (e.g. Default).
        let value: Value = json!({});
        return serde_json::from_value(value)
            .map_err(|e| anyhow!("empty body cannot be parsed: {e}"));
    }
    serde_json::from_slice(&bytes).map_err(|e| anyhow!("invalid JSON: {e}"))
}

/// Build a plain text/plain status-only response. Used for 401/404/405 so
/// `Authorization` failures and route misses look the same to a curl user.
fn bridge_status_response(status: StatusCode, body: &str) -> Result<AppResponse> {
    let resp = Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from(body.to_string())).boxed())?;
    Ok(resp)
}

/// Build a `Content-Type: application/json` response with the given status.
fn json_response(status: StatusCode, body: Value) -> AppResponse {
    let bytes = body.to_string();
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json; charset=utf-8")
        .body(Full::new(Bytes::from(bytes)).boxed())
        .expect("json_response build")
}

/// Constant-time byte comparison. We compare bridge tokens this way to
/// avoid leaking length / prefix information through timing side channels.
/// 32 hex chars per token, but the same routine handles bad-length inputs
/// safely by short-circuiting to false.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[derive(Debug, Deserialize)]
struct SearchRagReqBody {
    name: String,
    input: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsReqBody {
    model: String,
    messages: Vec<Value>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    max_tokens: Option<isize>,
    #[serde(default)]
    stream: bool,
    tools: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingsReqBody {
    input: EmbeddingsReqBodyInput,
    model: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum EmbeddingsReqBodyInput {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct RerankReqBody {
    documents: Vec<String>,
    query: String,
    model: String,
    top_n: Option<usize>,
}

#[derive(Debug)]
enum ResEvent {
    First(Option<String>),
    Text(String),
    ToolCalls(Vec<ToolCall>),
    Done,
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C signal handler")
}

fn generate_completion_id() -> String {
    let random_id = chrono::Utc::now().nanosecond();
    format!("chatcmpl-{random_id}")
}

/// Set CORS headers only for requests originating from localhost.
///
/// This prevents arbitrary websites from making cross-origin requests to the
/// local API server (e.g. a malicious page exfiltrating data via the LLM).
/// Same-origin requests (playground, arena) are unaffected by CORS.
fn set_cors_header(res: &mut AppResponse, request_origin: Option<&str>) {
    let origin = match request_origin {
        Some(o) if is_local_origin(o) => o,
        _ => return,
    };
    if let Ok(value) = hyper::header::HeaderValue::from_str(origin) {
        res.headers_mut()
            .insert(hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    res.headers_mut().insert(
        hyper::header::ACCESS_CONTROL_ALLOW_METHODS,
        hyper::header::HeaderValue::from_static("GET,POST,PUT,PATCH,DELETE"),
    );
    res.headers_mut().insert(
        hyper::header::ACCESS_CONTROL_ALLOW_HEADERS,
        hyper::header::HeaderValue::from_static("Content-Type,Authorization"),
    );
}

fn is_local_origin(origin: &str) -> bool {
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    match rest {
        Some(authority) => {
            let host = authority.split(':').next().unwrap_or("");
            matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]" | "0.0.0.0")
        }
        None => false,
    }
}

fn create_text_frame(id: &str, model: &str, created: i64, content: &str) -> Frame<Bytes> {
    let delta = if content.is_empty() {
        json!({ "role": "assistant", "content": content })
    } else {
        json!({ "content": content })
    };
    let choice = json!({
        "index": 0,
        "delta": delta,
        "finish_reason": null,
    });
    let value = build_chat_completion_chunk_json(id, model, created, &choice);
    Frame::data(Bytes::from(format!("data: {value}\n\n")))
}

fn create_tool_calls_frame(
    id: &str,
    model: &str,
    created: i64,
    tool_calls: &[ToolCall],
) -> Frame<Bytes> {
    let chunks = tool_calls
        .iter()
        .enumerate()
        .flat_map(|(i, call)| {
            let choice1 = json!({
              "index": 0,
              "delta": {
                "role": "assistant",
                "content": null,
                "tool_calls": [
                  {
                    "index": i,
                    "id": call.id,
                    "type": "function",
                    "function": {
                      "name": call.name,
                      "arguments": ""
                    }
                  }
                ]
              },
              "finish_reason": null
            });
            let choice2 = json!({
              "index": 0,
              "delta": {
                "tool_calls": [
                  {
                    "index": i,
                    "function": {
                      "arguments": call.arguments.to_string(),
                    }
                  }
                ]
              },
              "finish_reason": null
            });
            vec![
                build_chat_completion_chunk_json(id, model, created, &choice1),
                build_chat_completion_chunk_json(id, model, created, &choice2),
            ]
        })
        .map(|v| format!("data: {v}\n\n"))
        .collect::<Vec<String>>()
        .join("");
    Frame::data(Bytes::from(chunks))
}

fn create_done_frame(id: &str, model: &str, created: i64, has_tool_calls: bool) -> Frame<Bytes> {
    let finish_reason = if has_tool_calls { "tool_calls" } else { "stop" };
    let choice = json!({
        "index": 0,
        "delta": {},
        "finish_reason": finish_reason,
    });
    let value = build_chat_completion_chunk_json(id, model, created, &choice);
    Frame::data(Bytes::from(format!("data: {value}\n\ndata: [DONE]\n\n")))
}

fn build_chat_completion_chunk_json(id: &str, model: &str, created: i64, choice: &Value) -> Value {
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [choice],
    })
}

fn ret_non_stream(id: &str, model: &str, created: i64, output: &ChatCompletionsOutput) -> Bytes {
    let id = output.id.as_deref().unwrap_or(id);
    let input_tokens = output.input_tokens.unwrap_or_default();
    let output_tokens = output.output_tokens.unwrap_or_default();
    let total_tokens = input_tokens + output_tokens;
    let choice = if output.tool_calls.is_empty() {
        json!({
            "index": 0,
            "message": {
                "role": "assistant",
                "content": output.text,
            },
            "logprobs": null,
            "finish_reason": "stop",
        })
    } else {
        let content = if output.text.is_empty() {
            Value::Null
        } else {
            output.text.clone().into()
        };
        let tool_calls: Vec<_> = output
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": {
                        "name": call.name,
                        "arguments": call.arguments.to_string(),
                    }
                })
            })
            .collect();
        json!({
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content,
                "tool_calls": tool_calls,
            },
            "logprobs": null,
            "finish_reason": "tool_calls",
        })
    };
    let res_body = json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [choice],
        "usage": {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": total_tokens,
        },
    });
    Bytes::from(res_body.to_string())
}

fn ret_err<T: std::fmt::Display>(err: T) -> AppResponse {
    let data = json!({
        "error": {
            "message": err.to_string(),
            "type": "invalid_request_error",
        },
    });
    Response::builder()
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(data.to_string())).boxed())
        .unwrap()
}

fn parse_messages(message: Vec<Value>) -> Result<Vec<Message>> {
    let mut output = vec![];
    let mut tool_results = None;
    for (i, message) in message.into_iter().enumerate() {
        let err = || anyhow!("Failed to parse '.messages[{i}]'");
        let role = message["role"].as_str().ok_or_else(err)?;
        let content = match message.get("content") {
            Some(value) => {
                if let Some(value) = value.as_str() {
                    MessageContent::Text(value.to_string())
                } else if value.is_array() {
                    let value = serde_json::from_value(value.clone()).map_err(|_| err())?;
                    MessageContent::Array(value)
                } else if value.is_null() {
                    MessageContent::Text(String::new())
                } else {
                    return Err(err());
                }
            }
            None => MessageContent::Text(String::new()),
        };
        match role {
            "system" | "user" => {
                let role = match role {
                    "system" => MessageRole::System,
                    "user" => MessageRole::User,
                    _ => unreachable!(),
                };
                output.push(Message::new(role, content))
            }
            "assistant" => {
                let role = MessageRole::Assistant;
                match message["tool_calls"].as_array() {
                    Some(tool_calls) => {
                        if tool_results.is_some() {
                            return Err(err());
                        }
                        let mut list = vec![];
                        for tool_call in tool_calls {
                            if let (id, Some(name), Some(arguments)) = (
                                tool_call["id"].as_str().map(|v| v.to_string()),
                                tool_call["function"]["name"].as_str(),
                                tool_call["function"]["arguments"].as_str(),
                            ) {
                                let arguments =
                                    serde_json::from_str(arguments).map_err(|_| err())?;
                                list.push((id, name.to_string(), arguments));
                            } else {
                                return Err(err());
                            }
                        }
                        tool_results = Some((content.to_text(), list, vec![]));
                    }
                    None => output.push(Message::new(role, content)),
                }
            }
            "tool" => match tool_results.take() {
                Some((text, tool_calls, mut tool_values)) => {
                    let tool_call_id = message["tool_call_id"].as_str().map(|v| v.to_string());
                    let content = content.to_text();
                    let value: Value = serde_json::from_str(&content)
                        .ok()
                        .unwrap_or_else(|| content.into());

                    tool_values.push((value, tool_call_id));

                    if tool_calls.len() == tool_values.len() {
                        let mut list = vec![];
                        for ((id, name, arguments), (value, tool_call_id)) in
                            tool_calls.into_iter().zip(tool_values.into_iter())
                        {
                            if id != tool_call_id {
                                return Err(err());
                            }
                            list.push(ToolResult::new(ToolCall::new(name, arguments, id), value))
                        }
                        output.push(Message::new(
                            MessageRole::Assistant,
                            MessageContent::ToolCalls(MessageContentToolCalls::new(list, text)),
                        ));
                        tool_results = None;
                    } else {
                        tool_results = Some((text, tool_calls, tool_values));
                    }
                }
                None => return Err(err()),
            },
            _ => {
                return Err(err());
            }
        }
    }

    if tool_results.is_some() {
        bail!("Invalid messages");
    }

    Ok(output)
}

fn parse_tools(tools: Option<Vec<Value>>) -> Result<Option<Vec<FunctionDeclaration>>> {
    let tools = match tools {
        Some(v) => v,
        None => return Ok(None),
    };
    let mut functions = vec![];
    for (i, tool) in tools.into_iter().enumerate() {
        if let (Some("function"), Some(function)) = (
            tool["type"].as_str(),
            tool["function"]
                .as_object()
                .and_then(|v| serde_json::from_value(json!(v)).ok()),
        ) {
            functions.push(function);
        } else {
            bail!("Failed to parse '.tools[{i}]'")
        }
    }
    Ok(Some(functions))
}
