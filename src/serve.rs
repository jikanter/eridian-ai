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
use indexmap::IndexMap;
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
    /// Phase 16B: optional public bearer-token gate (`serve_api_key:`). When
    /// `Some`, every request except `OPTIONS` and `GET /health` must present
    /// `Authorization: Bearer <key>`. Distinct from `bridge_token`, which
    /// only guards the `/v1/state/*` REPL bridge.
    api_key: Option<String>,
    /// Phase 16A: cross-origin policy resolved from config at boot.
    cors: CorsPolicy,
    /// Phase 16E: the role/model/prompt/rag listing, behind a lock so
    /// `POST /v1/reload` can rebuild it from disk without a restart.
    listing: RwLock<Listing>,
}

/// Phase 16E: the disk-derived listing the OpenAI-compatible surface serves.
/// Held behind a lock so `/v1/reload` can swap in a fresh snapshot.
struct Listing {
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
        let cors = CorsPolicy::from_config(&snapshot);
        let api_key = snapshot.serve_api_key.clone();
        let listing = Self::build_listing(&snapshot);
        Self {
            config: config.clone(),
            bridge_token: std::env::var("AICHAT_BRIDGE_TOKEN").ok(),
            api_key,
            cors,
            listing: RwLock::new(listing),
        }
    }

    /// Build the model/role/prompt/rag listing from a config snapshot. Used
    /// at boot (`new`) and on hot-reload (`reload`). Roles, prompts, and rags
    /// are re-read from disk on every call; provider models come from the
    /// in-memory config (on-disk `clients:` edits still need a restart).
    fn build_listing(snapshot: &Config) -> Listing {
        let mut models = list_all_models(snapshot);
        let mut default_model = snapshot.model.clone();
        default_model.data_mut().name = DEFAULT_MODEL_NAME.into();
        models.insert(0, &default_model);
        let mut models: Vec<Value> = models
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
        let roles = Config::all_roles();
        // Phase 17A: every locally-known role appears as a virtual model
        // (`role:<name>`). Clients like OpenWebUI see them in the model
        // dropdown alongside provider models; selecting one routes the
        // request through the role's prompt + pipeline.
        for role in &roles {
            if role.name().is_empty() {
                continue;
            }
            models.push(json!({
                "id": format!("role:{}", role.name()),
                "object": "model",
                "owned_by": "aichat-role",
            }));
        }
        Listing {
            models,
            prompts: Config::all_prompts(),
            roles,
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
            set_cors_header(&mut res, request_origin.as_deref(), &self.cors);
            return Ok(res);
        }

        // Phase 2 bridge surface: `/v1/state/*` mutates the live config so
        // pi-side slash commands (defined in pi-extensions/) take effect for
        // subsequent /v1/chat/completions on the same server. Gated by a
        // per-launch bearer token; absent CLI `--serve` users never see
        // these routes, they just 404 like any unknown path. The bridge has
        // its own token, so it is exempt from the public `serve_api_key` gate.
        if path.starts_with("/v1/state/") {
            let mut res = self
                .handle_bridge(&method, path, req)
                .await
                .unwrap_or_else(ret_err);
            set_cors_header(&mut res, request_origin.as_deref(), &self.cors);
            info!("{method} {uri} {}", res.status().as_u16());
            return Ok(res);
        }

        // Phase 16B: optional public bearer-token gate. `/health` stays open
        // so orchestration probes (Docker/K8s/systemd) work without a key.
        if path != "/health" {
            let provided = req
                .headers()
                .get(hyper::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok());
            if !check_api_key(self.api_key.as_deref(), provided) {
                let mut res = json_response(
                    StatusCode::UNAUTHORIZED,
                    json!({ "error": { "code": 401, "message": "Unauthorized" } }),
                );
                set_cors_header(&mut res, request_origin.as_deref(), &self.cors);
                info!("{method} {uri} 401");
                return Ok(res);
            }
        }

        let mut status = StatusCode::OK;
        let res = if path == "/health" {
            self.health()
        } else if path == "/v1/reload" {
            self.reload_endpoint()
        } else if path == "/v1/chat/completions" {
            self.chat_completions(req).await
        } else if path == "/v1/embeddings" {
            self.embeddings(req).await
        } else if path == "/v1/rerank" {
            self.rerank(req).await
        } else if path == "/v1/models" {
            self.list_models()
        } else if path == "/v1/roles" {
            self.list_roles(query_flag(&uri, "include_prompt"))
        } else if let Some(rest) = path.strip_prefix("/v1/roles/") {
            // Phase 17B: `/v1/roles/{name}/invoke` is matched ahead of the
            // bare single-role retrieval. Future Phase 17C streaming variant
            // will dispatch on method=POST + Accept: text/event-stream.
            if let Some(name) = rest.strip_suffix("/invoke") {
                self.invoke_role(name, req).await
            } else {
                self.get_role(rest, query_flag(&uri, "include_prompt"))
            }
        } else if path == "/v1/prompts" {
            self.list_prompts()
        } else if path == "/v1/rags" {
            self.list_rags()
        } else if path == "/v1/rags/search" {
            self.search_rag(req).await
        } else if path == "/v1/pipelines/run" {
            self.run_pipeline(req).await
        } else if path == "/v1/batch" {
            self.batch(req).await
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
                // Phase 16G: handlers may set their own status (e.g. 404 for
                // a missing role). Preserve any non-OK code they emitted
                // instead of overwriting with the route-level default.
                if res.status() != StatusCode::OK {
                    status = res.status();
                }
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
        set_cors_header(&mut res, request_origin.as_deref(), &self.cors);
        Ok(res)
    }

    /// Phase 16C: unauthenticated liveness/readiness probe. Reports the
    /// number of provider models (excluding `role:*` virtual models) and the
    /// number of roles currently served.
    fn health(&self) -> Result<AppResponse> {
        let listing = self.listing.read();
        let n_models = listing
            .models
            .iter()
            .filter(|m| m["owned_by"] != json!("aichat-role"))
            .count();
        let body = json!({
            "status": "ok",
            "models": n_models,
            "roles": listing.roles.len(),
        });
        Ok(json_response(StatusCode::OK, body))
    }

    /// Phase 16E: hot-reload the role/model/prompt/rag listing from disk.
    /// Re-reads role, prompt, and rag files so role-development edits take
    /// effect without a server restart. Returns the new counts.
    fn reload_endpoint(&self) -> Result<AppResponse> {
        let snapshot = self.config.read().clone();
        let listing = Self::build_listing(&snapshot);
        let n_roles = listing.roles.len();
        let n_models = listing
            .models
            .iter()
            .filter(|m| m["owned_by"] != json!("aichat-role"))
            .count();
        *self.listing.write() = listing;
        Ok(json_response(
            StatusCode::OK,
            json!({ "roles": n_roles, "models": n_models }),
        ))
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
        let data = json!({ "data": self.listing.read().models });
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .body(Full::new(Bytes::from(data.to_string())).boxed())?;
        Ok(res)
    }

    fn list_roles(&self, include_prompt: bool) -> Result<AppResponse> {
        // Phase 16F/16G: serialize through `RolePublicView` so the prompt
        // body and any server-local wiring (pipe_to, save_to, mcp_servers,
        // pipeline stage names) stay private by default. The local
        // playground opts back in with `?include_prompt=1`.
        let views: Vec<RolePublicView> = self
            .listing
            .read()
            .roles
            .iter()
            .map(|r| {
                let v = RolePublicView::from(r);
                if include_prompt {
                    v.with_prompt(r)
                } else {
                    v
                }
            })
            .collect();
        let data = json!({ "data": views });
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .body(Full::new(Bytes::from(data.to_string())).boxed())?;
        Ok(res)
    }

    /// Phase 16G: single-role retrieval. Returns the role's public view,
    /// `404 Not Found` when no role by that name exists on this server.
    ///
    /// Path-segment routing: `/v1/roles/` and `/v1/roles/foo/bar` fall through
    /// to the global Not-Found handler so they can't accidentally collide
    /// with role names. The Phase 17B `/v1/roles/{name}/invoke` route is
    /// matched ahead of this one in `handle()`.
    fn get_role(&self, name: &str, include_prompt: bool) -> Result<AppResponse> {
        if name.is_empty() || name.contains('/') {
            return self.build_not_found("Not Found");
        }
        let listing = self.listing.read();
        let role = match listing.roles.iter().find(|r| r.name() == name) {
            Some(r) => r,
            None => return self.build_not_found(&format!("Role '{name}' not found")),
        };
        let mut view = RolePublicView::from(role);
        if include_prompt {
            view = view.with_prompt(role);
        }
        let body = serde_json::to_string(&view)?;
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .body(Full::new(Bytes::from(body)).boxed())?;
        Ok(res)
    }

    fn build_not_found(&self, message: &str) -> Result<AppResponse> {
        let body = json!({ "error": { "code": 404, "message": message } }).to_string();
        let res = Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", "application/json; charset=utf-8")
            .body(Full::new(Bytes::from(body)).boxed())?;
        Ok(res)
    }

    /// Phase 17A: handle a `/v1/chat/completions` request whose `model` field
    /// was a `role:<name>` virtual model. Pulls the latest user message out
    /// of the conversation, runs the role, and wraps the result in an
    /// OpenAI-compatible `chat.completion` envelope.
    ///
    /// Conversation context (everything before the last user message) is
    /// dropped. Roles are single-shot operators; clients that want
    /// multi-turn behavior should use a session via the CLI rather than
    /// asking the server to interpret history.
    async fn chat_completions_via_role(
        &self,
        role_name: &str,
        full_model_id: &str,
        messages: Vec<Value>,
        stream: bool,
    ) -> Result<AppResponse> {
        let input = extract_last_user_message(&messages)
            .ok_or_else(|| anyhow!("Chat completion via role requires at least one user message"))?;

        let config = Arc::new(RwLock::new(self.config_snapshot()));
        let abort_signal = create_abort_signal();
        let result =
            crate::pipe::invoke_role(&config, role_name, &input, abort_signal).await?;

        let completion_id = generate_completion_id();
        let created = Utc::now().timestamp();
        let usage = json!({
            "prompt_tokens": result.metrics.input_tokens,
            "completion_tokens": result.metrics.output_tokens,
            "total_tokens": result.metrics.input_tokens + result.metrics.output_tokens,
            "cost_usd": result.metrics.cost_usd,
        });

        if stream {
            // Single-chunk SSE: roles run to completion before we know the
            // output, so we emit one delta with the full body and then [DONE].
            // Phase 17C will turn this into per-stage streaming.
            let chunk = json!({
                "id": completion_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": full_model_id,
                "choices": [{
                    "index": 0,
                    "delta": { "role": "assistant", "content": result.output },
                    "finish_reason": "stop",
                }],
            });
            let final_chunk = json!({
                "id": completion_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": full_model_id,
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": "stop",
                }],
                "usage": usage,
            });
            let body = format!(
                "data: {chunk}\n\ndata: {final_chunk}\n\ndata: [DONE]\n\n"
            );
            let res = Response::builder()
                .header("Content-Type", "text/event-stream; charset=utf-8")
                .header("Cache-Control", "no-cache")
                .header("X-AIChat-Cost-USD", format!("{:.6}", result.metrics.cost_usd))
                .body(Full::new(Bytes::from(body)).boxed())?;
            return Ok(res);
        }

        let envelope = json!({
            "id": completion_id,
            "object": "chat.completion",
            "created": created,
            "model": full_model_id,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": result.output,
                },
                "finish_reason": "stop",
            }],
            "usage": usage,
        });
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .header("X-AIChat-Cost-USD", format!("{:.6}", result.metrics.cost_usd))
            .body(Full::new(Bytes::from(envelope.to_string())).boxed())?;
        Ok(res)
    }

    /// Phase 17B: `POST /v1/roles/{name}/invoke` — dedicated role invocation.
    ///
    /// Request body:
    /// ```json
    /// {
    ///   "input": "string (required)",
    ///   "variables": { "k": "v", ... },
    ///   "model": "model-id",
    ///   "trace": true
    /// }
    /// ```
    ///
    /// Response:
    /// ```json
    /// {
    ///   "output": "...",
    ///   "usage": {
    ///     "input_tokens": 0, "output_tokens": 0,
    ///     "cost_usd": 0.0, "latency_ms": 0,
    ///     "model": "..."
    ///   },
    ///   "schema_valid": true,
    ///   "trace": { "stages": [...] }   // only when trace=true
    /// }
    /// ```
    async fn invoke_role(
        &self,
        role_name: &str,
        req: hyper::Request<Incoming>,
    ) -> Result<AppResponse> {
        // Path-segment sanity: `/v1/roles//invoke` and nested paths fall
        // through to 404 rather than running an empty/garbled lookup.
        if role_name.is_empty() || role_name.contains('/') {
            return self.build_not_found("Not Found");
        }
        // Phase 16F: 404 before reading the body when the role doesn't exist
        // on this server, so a misaddressed caller doesn't waste bandwidth
        // on a payload we'll discard. Scope the read guard so it isn't held
        // across the `await`s below.
        if !self.listing.read().roles.iter().any(|r| r.name() == role_name) {
            return self.build_not_found(&format!("Role '{role_name}' not found"));
        }

        let req_body = req.collect().await?.to_bytes();
        let req_body: Value = serde_json::from_slice(&req_body)
            .map_err(|err| anyhow!("Invalid request json, {err}"))?;
        debug!("invoke role request: {req_body}");
        let body: InvokeRoleReqBody = serde_json::from_value(req_body)
            .map_err(|err| anyhow!("Invalid request body, {err}"))?;

        if body.input.is_empty() {
            bail!("Field 'input' is required and must be non-empty");
        }

        // Each request runs against a fresh config snapshot. The base
        // server-time config holds defaults; we layer the request-time
        // model override and variables on top so concurrent invocations
        // don't see each other's mutations.
        let mut config = self.config_snapshot();
        config.role_variables = if body.variables.is_empty() {
            None
        } else {
            Some(body.variables.clone())
        };
        let config = Arc::new(RwLock::new(config));
        if let Some(model_id) = &body.model {
            config.write().set_model(model_id)?;
        }

        let abort_signal = create_abort_signal();

        // Phase 17C: streaming SSE response — stage.start / stage.end / done.
        if body.stream {
            return self
                .invoke_role_streaming_response(
                    config,
                    role_name.to_string(),
                    body.input.clone(),
                    body.trace,
                    abort_signal,
                )
                .await;
        }

        let result =
            crate::pipe::invoke_role(&config, role_name, &body.input, abort_signal).await?;

        let mut usage = json!({
            "input_tokens": result.metrics.input_tokens,
            "output_tokens": result.metrics.output_tokens,
            "cost_usd": result.metrics.cost_usd,
            "latency_ms": result.metrics.latency_ms,
        });
        if !result.metrics.model_id.is_empty() {
            usage["model"] = json!(result.metrics.model_id);
        }
        let mut envelope = json!({
            "output": result.output,
            "usage": usage,
            "schema_valid": result.schema_valid,
        });
        if body.trace {
            envelope["trace"] = json!({ "stages": result.stages });
        }
        // Phase 16H surfaces cost in a header too, for proxies that strip
        // bodies (e.g. CDN caches with text-only routes).
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .header("X-AIChat-Cost-USD", format!("{:.6}", result.metrics.cost_usd))
            .body(Full::new(Bytes::from(envelope.to_string())).boxed())?;
        Ok(res)
    }

    /// Phase 17C: stream the invoke response as `text/event-stream`.
    ///
    /// Wire format (one event per `\n\n`-separated frame):
    ///
    /// ```text
    /// event: stage.start
    /// data: {"index":0,"total":2,"role":"extract","model":null}
    ///
    /// event: stage.end
    /// data: {"index":0,"role":"extract","trace":{...},"output":"..."}
    ///
    /// event: done
    /// data: {"output":"...","usage":{...},"schema_valid":true}
    ///
    /// data: [DONE]
    /// ```
    ///
    /// The `done` event carries the full final output and aggregated usage;
    /// callers that only want stage-level telemetry can stop reading after
    /// the last `stage.end`.
    async fn invoke_role_streaming_response(
        &self,
        config: Arc<RwLock<Config>>,
        role_name: String,
        input: String,
        include_trace: bool,
        abort_signal: AbortSignal,
    ) -> Result<AppResponse> {
        let (event_tx, mut event_rx) = unbounded_channel::<crate::pipe::StageEvent>();
        let (frame_tx, frame_rx) =
            unbounded_channel::<std::result::Result<Frame<Bytes>, Infallible>>();

        // Driver: runs the role and pushes StageEvents into event_rx.
        let driver_config = config.clone();
        let driver_role = role_name.clone();
        let driver_input = input.clone();
        let driver_abort = abort_signal.clone();
        let driver_handle = tokio::spawn(async move {
            crate::pipe::invoke_role_streaming(
                &driver_config,
                &driver_role,
                &driver_input,
                driver_abort,
                event_tx,
            )
            .await
        });

        // Forwarder: drains StageEvents, writes SSE frames into frame_rx.
        let forwarder_tx = frame_tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = event_rx.recv().await {
                let frame = match ev {
                    crate::pipe::StageEvent::Start {
                        index,
                        total,
                        role,
                        model_override,
                    } => format!(
                        "event: stage.start\ndata: {}\n\n",
                        json!({
                            "index": index,
                            "total": total,
                            "role": role,
                            "model": model_override,
                        })
                    ),
                    crate::pipe::StageEvent::End {
                        index,
                        role,
                        trace,
                        output,
                    } => format!(
                        "event: stage.end\ndata: {}\n\n",
                        json!({
                            "index": index,
                            "role": role,
                            "trace": trace,
                            "output": output,
                        })
                    ),
                };
                let _ = forwarder_tx.send(Ok(Frame::data(Bytes::from(frame))));
            }
            // event_rx closed — driver task is done; await its result to
            // emit the final done / error frame.
            let join_result = driver_handle.await;
            let final_frame = match join_result {
                Ok(Ok(result)) => {
                    let mut done = json!({
                        "output": result.output,
                        "usage": {
                            "input_tokens": result.metrics.input_tokens,
                            "output_tokens": result.metrics.output_tokens,
                            "cost_usd": result.metrics.cost_usd,
                            "latency_ms": result.metrics.latency_ms,
                            "model": result.metrics.model_id,
                        },
                        "schema_valid": result.schema_valid,
                    });
                    if include_trace {
                        done["trace"] = json!({ "stages": result.stages });
                    }
                    format!("event: done\ndata: {done}\n\ndata: [DONE]\n\n")
                }
                Ok(Err(err)) => format!(
                    "event: error\ndata: {}\n\n",
                    json!({ "message": format!("{err}") })
                ),
                Err(join_err) => format!(
                    "event: error\ndata: {}\n\n",
                    json!({ "message": format!("invocation task panicked: {join_err}") })
                ),
            };
            let _ = forwarder_tx.send(Ok(Frame::data(Bytes::from(final_frame))));
        });

        let stream = UnboundedReceiverStream::new(frame_rx);
        let body = BodyExt::boxed(StreamBody::new(stream));
        let res = Response::builder()
            .header("Content-Type", "text/event-stream; charset=utf-8")
            .header("Cache-Control", "no-cache")
            .body(body)?;
        Ok(res)
    }

    fn list_prompts(&self) -> Result<AppResponse> {
        let data = json!({ "data": self.listing.read().prompts });
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .body(Full::new(Bytes::from(data.to_string())).boxed())?;
        Ok(res)
    }

    fn list_rags(&self) -> Result<AppResponse> {
        let data = json!({ "data": self.listing.read().rags });
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

    /// Phase 17D: `POST /v1/pipelines/run` — execute either a named pipeline
    /// (`<config>/pipelines/<name>.yaml`) or an inline list of stages.
    ///
    /// Request:
    /// ```json
    /// { "input": "...", "stages": [{"role": "a"}, {"role": "b"}], "trace": true }
    /// ```
    /// or
    /// ```json
    /// { "input": "...", "pipeline": "summarize-then-rate" }
    /// ```
    ///
    /// Response shape mirrors the Phase 17B invoke envelope.
    async fn run_pipeline(&self, req: hyper::Request<Incoming>) -> Result<AppResponse> {
        let req_body = req.collect().await?.to_bytes();
        let req_body: Value = serde_json::from_slice(&req_body)
            .map_err(|err| anyhow!("Invalid request json, {err}"))?;
        let body: RunPipelineReqBody = serde_json::from_value(req_body)
            .map_err(|err| anyhow!("Invalid request body, {err}"))?;

        if body.input.is_empty() {
            bail!("Field 'input' is required and must be non-empty");
        }

        let stages: Vec<crate::pipe::InlineStage> = match (body.stages, body.pipeline) {
            (Some(s), None) => {
                if s.is_empty() {
                    bail!("Field 'stages' must contain at least one stage");
                }
                s
            }
            (None, Some(name)) => crate::pipe::load_pipeline_stages(&name)?,
            (Some(_), Some(_)) => {
                bail!("Provide either 'stages' (inline) or 'pipeline' (named), not both")
            }
            (None, None) => bail!("Request must specify either 'stages' or 'pipeline'"),
        };

        let mut config = self.config_snapshot();
        config.role_variables = if body.variables.is_empty() {
            None
        } else {
            Some(body.variables.clone())
        };
        let config = Arc::new(RwLock::new(config));

        let abort_signal = create_abort_signal();
        let result =
            crate::pipe::run_inline_pipeline(&config, &stages, &body.input, abort_signal).await?;

        let envelope = json!({
            "output": result.output,
            "usage": {
                "input_tokens": result.metrics.input_tokens,
                "output_tokens": result.metrics.output_tokens,
                "cost_usd": result.metrics.cost_usd,
                "latency_ms": result.metrics.latency_ms,
                "model": result.metrics.model_id,
            },
            "schema_valid": result.schema_valid,
            "trace": { "stages": result.stages },
        });
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .header("X-AIChat-Cost-USD", format!("{:.6}", result.metrics.cost_usd))
            .body(Full::new(Bytes::from(envelope.to_string())).boxed())?;
        Ok(res)
    }

    /// Phase 17E: `POST /v1/batch` — apply a role (or pipeline) to a list of
    /// inputs, with bounded concurrency.
    ///
    /// Request:
    /// ```json
    /// {
    ///   "inputs": ["text1", "text2", ...],
    ///   "role": "classify",          // OR
    ///   "stages": [...],             // OR
    ///   "pipeline": "name",
    ///   "concurrency": 4             // optional, default 4, max 32
    /// }
    /// ```
    ///
    /// Response:
    /// ```json
    /// {
    ///   "results": [{ "index": 0, "output": "...", "usage": {...}, "error": null }, ...],
    ///   "usage": { aggregate across all items }
    /// }
    /// ```
    ///
    /// Per-item errors are captured in the `error` field; one bad input
    /// does not fail the whole batch. Batch-level errors (missing fields,
    /// unknown role) still 400.
    async fn batch(&self, req: hyper::Request<Incoming>) -> Result<AppResponse> {
        let req_body = req.collect().await?.to_bytes();
        let req_body: Value = serde_json::from_slice(&req_body)
            .map_err(|err| anyhow!("Invalid request json, {err}"))?;
        let body: BatchReqBody = serde_json::from_value(req_body)
            .map_err(|err| anyhow!("Invalid request body, {err}"))?;

        if body.inputs.is_empty() {
            bail!("Field 'inputs' must be a non-empty array");
        }

        let target = match (body.role, body.stages, body.pipeline) {
            (Some(r), None, None) => BatchTarget::Role(r),
            (None, Some(s), None) => {
                if s.is_empty() {
                    bail!("Field 'stages' must contain at least one stage");
                }
                BatchTarget::Inline(s)
            }
            (None, None, Some(name)) => {
                BatchTarget::Inline(crate::pipe::load_pipeline_stages(&name)?)
            }
            _ => bail!("Specify exactly one of: 'role', 'stages', 'pipeline'"),
        };

        // Bounded concurrency: default 4, capped at 32 so a single batch
        // call can't blow past the server's per-provider rate budget.
        let concurrency = body.concurrency.unwrap_or(4).clamp(1, 32);
        let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));

        let mut config = self.config_snapshot();
        config.role_variables = if body.variables.is_empty() {
            None
        } else {
            Some(body.variables.clone())
        };
        let config = Arc::new(RwLock::new(config));
        let target = Arc::new(target);

        let mut handles = Vec::with_capacity(body.inputs.len());
        for (i, input) in body.inputs.into_iter().enumerate() {
            let permit_sem = semaphore.clone();
            let cfg = config.clone();
            let tgt = target.clone();
            let abort = create_abort_signal();
            handles.push(tokio::spawn(async move {
                let _permit = permit_sem.acquire_owned().await.expect("semaphore closed");
                let result = match &*tgt {
                    BatchTarget::Role(r) => {
                        crate::pipe::invoke_role(&cfg, r, &input, abort).await
                    }
                    BatchTarget::Inline(stages) => {
                        crate::pipe::run_inline_pipeline(&cfg, stages, &input, abort).await
                    }
                };
                (i, result)
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for h in handles {
            match h.await {
                Ok((i, Ok(r))) => results.push((i, Ok(r))),
                Ok((i, Err(e))) => results.push((i, Err(e.to_string()))),
                Err(join_err) => {
                    // Spawn failure — surface as an item-level error with no
                    // index recovery; the batch as a whole still succeeds.
                    results.push((results.len(), Err(format!("task panic: {join_err}"))));
                }
            }
        }
        // Preserve original input order.
        results.sort_by_key(|(i, _)| *i);

        let mut agg_in: u64 = 0;
        let mut agg_out: u64 = 0;
        let mut agg_cost: f64 = 0.0;
        let mut agg_latency: u64 = 0;
        let items: Vec<Value> = results
            .into_iter()
            .map(|(i, r)| match r {
                Ok(invoke) => {
                    agg_in += invoke.metrics.input_tokens;
                    agg_out += invoke.metrics.output_tokens;
                    agg_cost += invoke.metrics.cost_usd;
                    agg_latency += invoke.metrics.latency_ms;
                    json!({
                        "index": i,
                        "output": invoke.output,
                        "usage": {
                            "input_tokens": invoke.metrics.input_tokens,
                            "output_tokens": invoke.metrics.output_tokens,
                            "cost_usd": invoke.metrics.cost_usd,
                            "latency_ms": invoke.metrics.latency_ms,
                        },
                        "schema_valid": invoke.schema_valid,
                        "error": Value::Null,
                    })
                }
                Err(msg) => json!({
                    "index": i,
                    "output": Value::Null,
                    "usage": Value::Null,
                    "schema_valid": false,
                    "error": msg,
                }),
            })
            .collect();
        let envelope = json!({
            "results": items,
            "usage": {
                "input_tokens": agg_in,
                "output_tokens": agg_out,
                "cost_usd": agg_cost,
                "latency_ms": agg_latency,
            },
        });
        let res = Response::builder()
            .header("Content-Type", "application/json; charset=utf-8")
            .header("X-AIChat-Cost-USD", format!("{:.6}", agg_cost))
            .body(Full::new(Bytes::from(envelope.to_string())).boxed())?;
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
            stream_options,
            tools,
        } = req_body;

        // Phase 17A: when the caller asks for a `role:<name>` virtual model,
        // route to the role-invocation path instead of a raw model call. The
        // role's own model + pipeline take over from here; `temperature`,
        // `top_p`, and `tools` from the request body are deliberately
        // ignored so the role's declaration wins.
        if let Some(role_name) = model.strip_prefix("role:") {
            if !self.listing.read().roles.iter().any(|r| r.name() == role_name) {
                return self.build_not_found(&format!("Role '{role_name}' not found"));
            }
            return self
                .chat_completions_via_role(role_name, &model, messages, stream)
                .await;
        }

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

        // Phase 16D: honor `stream_options: {include_usage: true}`. When the
        // model streams, inject the same option upstream so OpenAI-compatible
        // providers append a usage block to their final chunk; the handler
        // captures it and we re-emit it to the caller. For `no_stream` models
        // the usage comes straight from the buffered non-streaming response.
        let include_usage = stream_options
            .as_ref()
            .map(|o| o.include_usage)
            .unwrap_or(false);
        let extensions = if include_usage && stream && !client.model().no_stream() {
            Some(json!({ "stream_options": { "include_usage": true } }))
        } else {
            None
        };

        let data: ChatCompletionsData = ChatCompletionsData {
            messages,
            temperature,
            top_p,
            functions,
            stream,
            output_schema: None,
            extensions,
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
                    include_usage: bool,
                ) {
                    if client.model().no_stream() {
                        data.stream = false;
                        let ret = client.chat_completions_inner(http_client, data).await;
                        match ret {
                            Ok(output) => {
                                let ChatCompletionsOutput {
                                    text,
                                    tool_calls,
                                    input_tokens,
                                    output_tokens,
                                    ..
                                } = output;
                                let _ = tx.send(ResEvent::First(None));
                                is_first.store(false, Ordering::SeqCst);
                                let _ = tx.send(ResEvent::Text(text));
                                if !tool_calls.is_empty() {
                                    let _ = tx.send(ResEvent::ToolCalls(tool_calls));
                                }
                                if include_usage {
                                    let _ = tx.send(ResEvent::Usage(build_usage_value(
                                        client,
                                        input_tokens.unwrap_or(0),
                                        output_tokens.unwrap_or(0),
                                    )));
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
                        // Phase 16D: emit the captured usage (if any) just
                        // before Done so it lands as the final chunk's usage.
                        if include_usage {
                            let (it, ot) = handler.usage();
                            let _ = tx.send(ResEvent::Usage(build_usage_value(
                                client,
                                it.unwrap_or(0),
                                ot.unwrap_or(0),
                            )));
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
                        is_first,
                        include_usage,
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
                        ResEvent::Usage(usage) => Some(Ok(create_usage_frame(
                            completion_id,
                            model,
                            *created,
                            &usage,
                        ))),
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

/// Read a boolean-ish query-string flag from a URI. Treats `?key`, `?key=1`,
/// `?key=true`, `?key=yes`, and `?key=on` as `true`; everything else (absent
/// or any other value) as `false`. Case-insensitive on the value.
fn query_flag(uri: &hyper::Uri, key: &str) -> bool {
    let Some(query) = uri.query() else {
        return false;
    };
    query.split('&').any(|kv| match kv.split_once('=') {
        Some((k, v)) if k == key => matches!(
            v.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Some(_) => false,
        None => kv == key,
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

/// Phase 17D: request body for `POST /v1/pipelines/run`.
#[derive(Debug, Deserialize)]
struct RunPipelineReqBody {
    input: String,
    /// Inline stage list. Mutually exclusive with `pipeline`.
    #[serde(default)]
    stages: Option<Vec<crate::pipe::InlineStage>>,
    /// Named pipeline (looked up at `<config>/pipelines/<name>.yaml`).
    /// Mutually exclusive with `stages`.
    #[serde(default)]
    pipeline: Option<String>,
    #[serde(default)]
    variables: IndexMap<String, String>,
}

/// Phase 17E: request body for `POST /v1/batch`.
#[derive(Debug, Deserialize)]
struct BatchReqBody {
    inputs: Vec<String>,
    /// Apply this role to each input.
    #[serde(default)]
    role: Option<String>,
    /// Inline stage list applied to each input.
    #[serde(default)]
    stages: Option<Vec<crate::pipe::InlineStage>>,
    /// Named pipeline applied to each input.
    #[serde(default)]
    pipeline: Option<String>,
    /// Shared variables applied to every batch item.
    #[serde(default)]
    variables: IndexMap<String, String>,
    /// Max in-flight invocations. Default 4, clamped to [1, 32].
    #[serde(default)]
    concurrency: Option<usize>,
}

/// Phase 17E: internal dispatch shape — picked between role or pipeline once,
/// shared across all items in the batch.
enum BatchTarget {
    Role(String),
    Inline(Vec<crate::pipe::InlineStage>),
}

/// Phase 17B: request body for `POST /v1/roles/{name}/invoke`.
#[derive(Debug, Deserialize)]
struct InvokeRoleReqBody {
    input: String,
    /// Optional role variables (`-v key=value` equivalent). Empty map is
    /// treated the same as omitted.
    #[serde(default)]
    variables: IndexMap<String, String>,
    /// Optional per-request model override. When set, overrides the role's
    /// declared model AND the server's default.
    #[serde(default)]
    model: Option<String>,
    /// When `true`, include a per-stage breakdown under `trace.stages` in
    /// the response. Defaults to off to keep responses small.
    #[serde(default)]
    trace: bool,
    /// Phase 17C: when `true`, response is `text/event-stream` with
    /// `stage.start` / `stage.end` / `done` SSE events.
    #[serde(default)]
    stream: bool,
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
    /// Phase 16D: OpenAI's `stream_options`. Only `include_usage` is honored.
    #[serde(default)]
    stream_options: Option<StreamOptions>,
    tools: Option<Vec<Value>>,
}

/// Phase 16D: subset of OpenAI's `stream_options`. When `include_usage` is
/// true, the streaming response carries a trailing usage chunk before
/// `[DONE]` (matching `stream_options: {"include_usage": true}`).
#[derive(Debug, Deserialize)]
struct StreamOptions {
    #[serde(default)]
    include_usage: bool,
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
    /// Phase 16D: a finished `usage` block (prompt/completion/total tokens +
    /// `cost_usd`) emitted just before `Done` when the caller requested it.
    Usage(Value),
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

/// Phase 16A: which cross-origin requests `--serve` answers with CORS
/// headers. Localhost is always allowed (the bundled playground/arena run
/// same-origin, but browsers still send `Origin` on some requests). Operators
/// widen this with `serve_cors_origins:` or, on trusted networks,
/// `serve_cors_allow_all: true`.
#[derive(Clone, Default)]
struct CorsPolicy {
    allow_all: bool,
    origins: Vec<String>,
}

impl CorsPolicy {
    fn from_config(config: &Config) -> Self {
        Self {
            allow_all: config.serve_cors_allow_all,
            origins: config.serve_cors_origins.clone().unwrap_or_default(),
        }
    }

    /// True when a request from `origin` should receive CORS headers.
    fn allows(&self, origin: &str) -> bool {
        self.allow_all
            || self.origins.iter().any(|o| o == origin)
            || is_local_origin(origin)
    }
}

/// Phase 16B: returns whether a request clears the optional bearer-token gate.
/// When no key is configured every request passes (historical behavior).
/// Comparison is constant-time to avoid leaking the key via timing.
fn check_api_key(configured: Option<&str>, auth_header: Option<&str>) -> bool {
    let Some(key) = configured else {
        return true;
    };
    let provided = auth_header
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("");
    constant_time_eq(provided.as_bytes(), key.as_bytes())
}

/// Set CORS headers when the request origin is permitted by `policy`.
///
/// This prevents arbitrary websites from making cross-origin requests to the
/// API server (e.g. a malicious page exfiltrating data via the LLM). Localhost
/// is always allowed; `serve_cors_origins` / `serve_cors_allow_all` widen it.
/// Same-origin requests (playground, arena) are unaffected by CORS.
fn set_cors_header(res: &mut AppResponse, request_origin: Option<&str>, policy: &CorsPolicy) {
    let origin = match request_origin {
        Some(o) if policy.allows(o) => o,
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

/// Phase 16D: an OpenAI-style usage-only chunk — `choices: []` plus a
/// `usage` object — emitted right before `[DONE]` when the caller set
/// `stream_options: {include_usage: true}`.
fn create_usage_frame(id: &str, model: &str, created: i64, usage: &Value) -> Frame<Bytes> {
    let value = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [],
        "usage": usage,
    });
    Frame::data(Bytes::from(format!("data: {value}\n\n")))
}

/// Phase 16D: assemble the streaming `usage` object, multiplying token
/// counts by the active model's prices for `cost_usd`.
fn build_usage_value(client: &dyn Client, input_tokens: u64, output_tokens: u64) -> Value {
    json!({
        "prompt_tokens": input_tokens,
        "completion_tokens": output_tokens,
        "total_tokens": input_tokens + output_tokens,
        "cost_usd": compute_cost(client.model(), input_tokens, output_tokens),
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

/// Phase 17A: pull the last user message's text out of a chat-completions
/// `messages` array. Roles consume a single input string, so we don't need
/// the prior conversation; the LLM-side history belongs to the calling
/// app, not the role.
///
/// Returns `None` when no user message is present (an all-system prompt has
/// nothing to act on). String and OpenAI-style multipart-array content are
/// both supported; for multipart we concatenate `text` parts and skip any
/// image / file parts (roles don't carry vision context in this path).
fn extract_last_user_message(messages: &[Value]) -> Option<String> {
    for msg in messages.iter().rev() {
        if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        let content = msg.get("content")?;
        if let Some(s) = content.as_str() {
            return Some(s.to_string());
        }
        if let Some(parts) = content.as_array() {
            let text: String = parts
                .iter()
                .filter_map(|p| {
                    if p.get("type").and_then(|v| v.as_str()) == Some("text") {
                        p.get("text").and_then(|v| v.as_str()).map(str::to_string)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Phase 17A: extract_last_user_message ----

    #[test]
    fn extracts_string_content_from_user_message() {
        let msgs = vec![json!({ "role": "user", "content": "hello" })];
        assert_eq!(extract_last_user_message(&msgs), Some("hello".to_string()));
    }

    #[test]
    fn returns_the_latest_user_message_only() {
        let msgs = vec![
            json!({ "role": "user", "content": "first question" }),
            json!({ "role": "assistant", "content": "first answer" }),
            json!({ "role": "user", "content": "second question" }),
        ];
        assert_eq!(
            extract_last_user_message(&msgs),
            Some("second question".to_string())
        );
    }

    #[test]
    fn skips_system_and_assistant_messages() {
        let msgs = vec![
            json!({ "role": "system", "content": "you are X" }),
            json!({ "role": "assistant", "content": "ok" }),
        ];
        assert_eq!(extract_last_user_message(&msgs), None);
    }

    #[test]
    fn concatenates_text_parts_from_array_content() {
        let msgs = vec![json!({
            "role": "user",
            "content": [
                { "type": "text", "text": "part one" },
                { "type": "image_url", "image_url": { "url": "data:..." } },
                { "type": "text", "text": "part two" },
            ]
        })];
        assert_eq!(
            extract_last_user_message(&msgs),
            Some("part one\npart two".to_string())
        );
    }

    #[test]
    fn returns_none_for_empty_messages() {
        assert_eq!(extract_last_user_message(&[]), None);
    }

    // ---- Phase 16A: CorsPolicy ----

    #[test]
    fn cors_localhost_is_always_allowed() {
        let policy = CorsPolicy::default();
        assert!(policy.allows("http://localhost:3000"));
        assert!(policy.allows("http://127.0.0.1:8000"));
        assert!(policy.allows("http://localhost"));
        assert!(policy.allows("http://0.0.0.0:8000"));
    }

    #[test]
    fn cors_rejects_unlisted_remote_origin_by_default() {
        let policy = CorsPolicy::default();
        assert!(!policy.allows("https://evil.example.com"));
        assert!(!policy.allows("http://host.docker.internal:3000"));
    }

    #[test]
    fn cors_allows_configured_origin() {
        let policy = CorsPolicy {
            allow_all: false,
            origins: vec!["http://host.docker.internal:3000".to_string()],
        };
        assert!(policy.allows("http://host.docker.internal:3000"));
        // Still localhost, still rejected-elsewhere.
        assert!(policy.allows("http://localhost:3000"));
        assert!(!policy.allows("https://evil.example.com"));
    }

    #[test]
    fn cors_allow_all_echoes_any_origin() {
        let policy = CorsPolicy {
            allow_all: true,
            origins: vec![],
        };
        assert!(policy.allows("https://anything.example.com"));
        assert!(policy.allows("http://localhost:3000"));
    }

    // ---- Phase 16B: check_api_key ----

    #[test]
    fn auth_passes_when_no_key_configured() {
        assert!(check_api_key(None, None));
        assert!(check_api_key(None, Some("Bearer whatever")));
    }

    #[test]
    fn auth_requires_matching_bearer_when_key_set() {
        assert!(check_api_key(Some("sk-secret"), Some("Bearer sk-secret")));
        assert!(!check_api_key(Some("sk-secret"), Some("Bearer wrong")));
        assert!(!check_api_key(Some("sk-secret"), Some("sk-secret"))); // no Bearer prefix
        assert!(!check_api_key(Some("sk-secret"), None));
        assert!(!check_api_key(Some("sk-secret"), Some("")));
    }

    // ---- Phase 16D: stream_options + usage chunk ----

    #[test]
    fn stream_options_include_usage_parses() {
        let body: ChatCompletionsReqBody = serde_json::from_value(json!({
            "model": "default",
            "messages": [],
            "stream": true,
            "stream_options": { "include_usage": true },
        }))
        .unwrap();
        assert!(body.stream_options.unwrap().include_usage);
    }

    #[test]
    fn stream_options_absent_defaults_to_none() {
        let body: ChatCompletionsReqBody = serde_json::from_value(json!({
            "model": "default",
            "messages": [],
            "stream": true,
        }))
        .unwrap();
        assert!(body.stream_options.is_none());
    }

    #[test]
    fn usage_frame_is_openai_shaped() {
        let usage = json!({
            "prompt_tokens": 12,
            "completion_tokens": 5,
            "total_tokens": 17,
            "cost_usd": 0.0001,
        });
        let frame = create_usage_frame("chatcmpl-x", "gpt-test", 42, &usage);
        let bytes = frame.into_data().expect("data frame");
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        // SSE framing.
        assert!(text.starts_with("data: "));
        assert!(text.ends_with("\n\n"));
        let payload: Value = serde_json::from_str(text.trim_start_matches("data: ").trim()).unwrap();
        assert_eq!(payload["object"], "chat.completion.chunk");
        assert_eq!(payload["choices"], json!([]));
        assert_eq!(payload["usage"]["total_tokens"], 17);
        assert_eq!(payload["usage"]["cost_usd"], 0.0001);
    }
}
