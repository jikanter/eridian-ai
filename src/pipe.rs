use crate::cache::StageCache;
use crate::cli::Cli;
use crate::client::{
    call_chat_completions, call_chat_completions_streaming, call_react, CallMetrics,
};
use crate::config::{
    pipeline_stage_admissible, run_lifecycle_hooks, validate_schema_traced, Agent, Config,
    EntityRef, GlobalConfig, Input, MergeStrategy, ParallelNode, PipelineNode, Role, RoleLike,
    RolePipelineStage, SwitchNode,
};
use crate::utils::*;

use anyhow::{bail, Context, Result};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use std::path::Path;

struct PipelineStage {
    role_name: String,
    model_id: Option<String>,
    /// Phase 11D: pre-allocated dollar budget for this stage. `None` means
    /// no enforcement; the stage runs with the model's native context window
    /// as its only limit. When `Some`, `run_stage_inner` tail-truncates the
    /// post-knowledge input text to fit `budget_usd_to_input_token_cap`.
    budget_usd: Option<f64>,
}

/// Phase 17B: per-stage execution trace. Public so server-side invocation
/// can include it in the response envelope (`trace: true`) and the CLI can
/// emit it under `-o json`.
///
/// Phase 21: `branch` is set when this stage ran inside a fan-out — its
/// value is the 1-based branch number within the parent `parallel:` node.
#[derive(Serialize, Clone, Debug)]
pub struct StageTrace {
    pub role: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<usize>,
}

#[derive(Deserialize)]
struct PipelineDef {
    /// Sequential form (preserved for backward compat).
    #[serde(default)]
    stages: Vec<PipelineStageDef>,
    /// Phase 21: full DAG form, mirroring the role-frontmatter `pipeline:`
    /// key. Either `stages:` or `pipeline:` must be set, not both.
    #[serde(default)]
    pipeline: Option<Vec<serde_json::Value>>,
    /// Phase 11D: total dollar budget for the pipeline. Divided across
    /// stages by `budget_weight`. `None` disables per-stage budget enforcement.
    #[serde(default)]
    budget_usd: Option<f64>,
}

#[derive(Deserialize)]
struct PipelineStageDef {
    role: String,
    #[serde(default)]
    model: Option<String>,
    /// Phase 11D: relative share of the pipeline's `budget_usd`. Defaults
    /// to 1.0 when unset.
    #[serde(default)]
    budget_weight: Option<f64>,
}

pub async fn run(config: GlobalConfig, cli: Cli, text: Option<String>) -> Result<()> {
    // Phase 21: `--pipe-def` may carry a DAG; `--stage` is always sequential.
    // Phase 11D: `--pipe-def` files may also declare `budget_usd:` at the
    // top level. CLI `--stage` form has no budget surface yet.
    let (nodes, pipeline_budget_usd): (Vec<PipelineNode>, Option<f64>) =
        if let Some(def_path) = &cli.pipe_def {
            load_pipeline_def_nodes(def_path)?
        } else if !cli.stages.is_empty() {
            (
                parse_stages(&cli.stages)?
                    .into_iter()
                    .map(|s| {
                        PipelineNode::Stage(RolePipelineStage {
                            role: s.role_name,
                            model: s.model_id,
                            budget_weight: None,
                        })
                    })
                    .collect(),
                None,
            )
        } else {
            bail!("Pipeline requires --stage or --pipe-def");
        };

    if nodes.is_empty() {
        bail!("Pipeline has no stages");
    }

    // Phase 11D: allocate per-top-level-node dollar budgets. Only leaf Stage
    // nodes carry a `budget_weight`; nested DAG nodes (parallel/switch) get
    // `None` here and consume their parent's full budget — DAG-aware
    // sub-allocation is a follow-up.
    let per_node_budgets: Option<Vec<f64>> = pipeline_budget_usd
        .filter(|b| *b > 0.0)
        .map(|total| {
            let weights: Vec<Option<f64>> = nodes
                .iter()
                .map(|n| match n {
                    PipelineNode::Stage(s) => s.budget_weight,
                    _ => None,
                })
                .collect();
            crate::context_budget::allocate_stage_budgets(&weights, total)
        });

    // Phase 9D + 21D: pre-flight validate every stage reachable through the
    // DAG (parallel branches + switch arms count) before any LLM call.
    {
        let stage_tuples = collect_preflight_stages(&nodes);
        crate::config::preflight::validate_pipeline_stages(&config.read(), &stage_tuples)?;
        crate::config::preflight::validate_pipeline_dag_structure(&nodes)
            .context("Pipeline DAG validation failed")?;
    }

    let mut input_text = match text {
        Some(t) => t,
        None if !cli.file.is_empty() => {
            let abort_signal = create_abort_signal();
            let input = Input::from_files_with_spinner(
                &config,
                "",
                cli.file.clone(),
                None,
                abort_signal,
            )
            .await?;
            input.text().to_string()
        }
        None => bail!("Pipeline requires input text or files (-f)"),
    };

    let abort_signal = create_abort_signal();
    let output_format = cli.output_format;

    let node_count = nodes.len();
    let mut stage_traces: Vec<StageTrace> = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == node_count - 1;
        let stage_budget = per_node_budgets.as_ref().map(|v| v[i]);
        let (output, mut traces) = run_node(
            &config,
            node,
            i,
            node_count,
            &input_text,
            is_last,
            None,
            stage_budget,
            abort_signal.clone(),
        )
        .await?;
        stage_traces.append(&mut traces);
        input_text = output;
    }

    // JSON envelope with trace metadata when output format is JSON
    if matches!(output_format, Some(crate::cli::OutputFormat::Json)) {
        let total_cost: f64 = stage_traces.iter().map(|s| s.cost_usd).sum();
        let total_latency: u64 = stage_traces.iter().map(|s| s.latency_ms).sum();
        let envelope = serde_json::json!({
            "output": serde_json::from_str::<serde_json::Value>(&input_text).unwrap_or(serde_json::Value::String(input_text)),
            "trace": {
                "stages": stage_traces,
                "total_cost_usd": total_cost,
                "total_latency_ms": total_latency,
            }
        });
        println!("{}", serde_json::to_string_pretty(&envelope)?);
    }

    Ok(())
}

/// Phase 21D: flatten a pipeline DAG into `(role_name, model_id)` tuples
/// for preflight. Reaches into parallel branches and switch arms so an
/// unknown role anywhere in the tree fails before any LLM call. Custom
/// merge roles are also surfaced (with no model override, since they
/// inherit the role's own model).
fn collect_preflight_stages(nodes: &[PipelineNode]) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for n in nodes {
        for s in n.all_stages() {
            out.push((s.role.clone(), s.model.clone()));
        }
        for merger in n.merge_role_names() {
            out.push((merger, None));
        }
    }
    out
}

async fn run_stage(
    config: &GlobalConfig,
    stage: &PipelineStage,
    stage_index: usize,
    stage_count: usize,
    input_text: &str,
    is_last: bool,
    abort_signal: AbortSignal,
) -> Result<(String, CallMetrics)> {
    // Phase 10C/10D: peek at the role once for the retry budget and the model
    // fallback chain. If the role fails to load, fall through to a single
    // primary-model attempt so the config error surfaces on the first call.
    let role = config.read().retrieve_role(&stage.role_name).ok();
    let max_stage_retries = role
        .as_ref()
        .and_then(|r| r.stage_retries())
        .unwrap_or(1);
    let fallback_models: Vec<String> = role
        .as_ref()
        .map(|r| r.fallback_models().to_vec())
        .unwrap_or_default();

    // Phase 10D: build the candidate chain — primary first, then each fallback.
    // `None` = use the role's default model (no per-stage override); `Some(id)`
    // forces that model via `set_model` inside `run_stage_inner`.
    let mut candidates: Vec<Option<String>> = vec![stage.model_id.clone()];
    for fb in &fallback_models {
        candidates.push(Some(fb.clone()));
    }
    let total_models = candidates.len();

    for (model_index, model_override) in candidates.into_iter().enumerate() {
        let attempt_stage = PipelineStage {
            role_name: stage.role_name.clone(),
            model_id: model_override.clone(),
            budget_usd: stage.budget_usd,
        };
        let model_label = model_override
            .clone()
            .unwrap_or_else(|| "<role-default>".to_string());

        let mut attempt: usize = 0;
        loop {
            // Phase 0C: save model state per attempt — the inner may have
            // mutated it even on failure; each retry starts from a clean slate.
            let saved_model_id = config.read().current_model().id();

            let result = run_stage_inner(
                config,
                &attempt_stage,
                input_text,
                is_last,
                abort_signal.clone(),
            )
            .await;

            // Phase 0C: restore model state regardless of success/failure.
            if let Err(e) = config.write().set_model(&saved_model_id) {
                debug!("Failed to restore model after pipeline stage: {e}");
            }

            match result {
                Ok(v) => return Ok(v),
                Err(e) if attempt < max_stage_retries && is_retryable_stage_error(&e) => {
                    warn!(
                        "Pipeline stage {}/{} (role '{}', model '{}') failed on attempt {}/{}, retrying: {}",
                        stage_index + 1,
                        stage_count,
                        stage.role_name,
                        model_label,
                        attempt + 1,
                        max_stage_retries + 1,
                        e
                    );
                    attempt += 1;
                    continue;
                }
                Err(e)
                    if is_retryable_stage_error(&e)
                        && model_index + 1 < total_models =>
                {
                    warn!(
                        "Pipeline stage {}/{} (role '{}', model '{}') exhausted retries, falling back to next model: {}",
                        stage_index + 1,
                        stage_count,
                        stage.role_name,
                        model_label,
                        e
                    );
                    break; // advance outer loop to next fallback model
                }
                Err(e) => {
                    // Non-retryable, or retryable with no remaining fallbacks.
                    let final_model_id = model_override
                        .clone()
                        .unwrap_or_else(|| config.read().current_model().id());
                    return Err(anyhow::Error::new(AichatError::PipelineStage {
                        stage: stage_index + 1,
                        total: stage_count,
                        role_name: stage.role_name.clone(),
                        model_id: Some(final_model_id),
                        message: e.to_string(),
                    }));
                }
            }
        }
    }

    // Unreachable: the final candidate's non-retryable / no-fallbacks-left
    // arm always returns Err.
    unreachable!("fallback loop exited without terminating");
}

/// Phase 19C / 20D: resolved pipeline-stage target.
///
/// Local stages collapse to a `Role` (agents via `RoleLike::to_role()`).
/// Remote stages instead carry the resolved HTTP target so `run_stage_inner`
/// can dispatch over the network without re-doing classification.
enum StageTarget {
    Local(Role),
    Remote(crate::config::remote::ResolvedRemote),
}

/// Phase 19C: load the entity for a pipeline stage. Roles use the existing
/// path; agents are loaded via `Agent::init` and bridged to a Role through
/// the `RoleLike::to_role()` synthesis. Macros are rejected — they aren't
/// role-shaped. Phase 20D adds the Remote branch, which classifies but
/// defers the HTTP call to `run_stage_inner`.
///
/// Caveats for the agent path:
/// - Agent variables are not interactively resolved here. Defaults (including
///   shell defaults) apply; missing required variables leave `{{var}}` tokens
///   in the prompt unrendered.
/// - Agent RAG is loaded only if a pre-built RAG file exists. There is no
///   interactive "init RAG?" prompt in the pipeline path.
async fn resolve_stage_entity(
    config: &GlobalConfig,
    raw_name: &str,
    abort_signal: AbortSignal,
) -> Result<StageTarget> {
    let entity = config
        .read()
        .classify_entity(raw_name)
        .with_context(|| format!("Failed to resolve pipeline stage '{raw_name}'"))?;
    pipeline_stage_admissible(&entity)?;
    match entity {
        EntityRef::Role(name) => {
            let r = config.read().retrieve_role(&name).with_context(|| {
                format!("Failed to load role '{name}' for pipeline stage")
            })?;
            Ok(StageTarget::Local(r))
        }
        EntityRef::Agent(name) => {
            let agent = Agent::init(config, &name, abort_signal)
                .await
                .with_context(|| format!("Failed to load agent '{name}' for pipeline stage"))?;
            Ok(StageTarget::Local(agent.to_role()))
        }
        EntityRef::Remote { target, role } => {
            // Phase 20D: turn the parsed target+role into a concrete
            // endpoint via the `remotes:` config table. The HTTP call to
            // discover/invoke happens later, on every retry attempt.
            let resolved = crate::config::remote::resolve_target(
                &config.read().remotes,
                &target,
                &role,
            )?;
            Ok(StageTarget::Remote(resolved))
        }
        EntityRef::Macro(_) => unreachable!("rejected by pipeline_stage_admissible above"),
    }
}

async fn run_stage_inner(
    config: &GlobalConfig,
    stage: &PipelineStage,
    input_text: &str,
    is_last: bool,
    abort_signal: AbortSignal,
) -> Result<(String, CallMetrics)> {
    let target = resolve_stage_entity(config, &stage.role_name, abort_signal.clone()).await?;
    let role = match target {
        StageTarget::Local(r) => r,
        StageTarget::Remote(resolved) => {
            // Phase 20D: federated stage — HTTP-invoke the remote and
            // return its output as if it were a local stage's output.
            // Schema validation, retries, caching, lifecycle hooks all
            // live on the remote side; we only carry the result through.
            let http = reqwest::Client::builder()
                .build()
                .context("Failed to build HTTP client for remote stage")?;
            let result = crate::config::remote::invoke(
                &http,
                &resolved,
                input_text,
                &Default::default(),
                false,
            )
            .await?;
            let output = if is_last {
                result.output.clone()
            } else {
                strip_think_tag(&result.output).to_string()
            };
            if is_last && !output.is_empty() {
                print!("{output}");
                std::io::Write::flush(&mut std::io::stdout())?;
                if !output.ends_with('\n') {
                    println!();
                }
            }
            return Ok((output, result.metrics));
        }
    };

    if let Some(model_id) = &stage.model_id {
        config.write().set_model(model_id)?;
    }

    let trace_emitter = config
        .read()
        .trace_config
        .clone()
        .map(crate::utils::trace::TraceEmitter::new);

    if let Some(schema) = role.input_schema() {
        validate_schema_traced("input", schema, input_text, trace_emitter.as_ref())?;
    }

    let has_tools = role.use_tools().is_some();
    let mut input = Input::from_str(config, input_text, Some(role.clone()));

    // Phase 26D: inject knowledge-base context per stage. No-op unless this
    // stage's role declares `knowledge:` or the user passed `--knowledge`.
    input.use_knowledge()?;

    let client = input.create_client()?;

    // Phase 11D: per-stage budget enforcement. Convert the dollar budget into
    // an input-token cap using the resolved model's prices, then tail-truncate
    // the post-knowledge input text to fit. Truncation is preferred over
    // hard-failing — losing the bottom of a long context is recoverable, a
    // refused pipeline run is not. We clear `patched_text` first so the
    // post-knowledge text is the one being capped.
    if let Some(budget_usd) = stage.budget_usd {
        let model_data = client.model().data();
        let input_price = model_data.input_price.unwrap_or(0.0);
        let output_price = model_data.output_price.unwrap_or(0.0);
        let cap = crate::context_budget::budget_usd_to_input_token_cap(
            budget_usd,
            input_price,
            crate::context_budget::DEFAULT_OUTPUT_RESERVE,
            output_price,
        );
        let current = input.text();
        let (trimmed, was_truncated) =
            crate::context_budget::truncate_to_token_budget(&current, cap);
        if was_truncated {
            let original_tokens = crate::utils::estimate_token_length(&current);
            eprintln!(
                "Stage budget: role '{}' input truncated {original_tokens} → {cap} tokens (budget ${budget_usd:.4})",
                stage.role_name
            );
            input.clear_patch();
            input.set_text(trimmed);
        }
    }

    // Phase 10B: content-addressable stage output cache. Skips when caching is
    // disabled (`--no-cache`), on dry-run, or for tool-using stages (tool calls
    // carry non-deterministic side effects and must not be replayed).
    let cache_enabled = !config.read().no_cache
        && !config.read().dry_run
        && !has_tools;
    let cache_key = if cache_enabled {
        // Hash the post-injection text so a change in the knowledge context
        // (new bindings, recompiled KB) invalidates the cache entry.
        Some(StageCache::key(
            &stage.role_name,
            &client.model().id(),
            &input.text(),
        ))
    } else {
        None
    };
    if let Some(key) = &cache_key {
        let cache = StageCache::new(
            Config::local_path(".cache/stages"),
            config.read().cache_ttl_secs,
        );
        if let Some(cached) = cache.get(key) {
            debug!("Stage cache hit for role '{}'", stage.role_name);
            let model_id = client.model().id();
            let metrics = CallMetrics {
                model_id,
                turns: 1,
                ..Default::default()
            };
            if is_last && !input.stream() {
                let final_output = if let Some(fmt) = config.read().output_format {
                    if fmt.is_structured() {
                        fmt.clean_output(&cached)?
                    } else {
                        cached.clone()
                    }
                } else {
                    cached.clone()
                };
                print!("{final_output}");
                std::io::Write::flush(&mut std::io::stdout())?;
                if !final_output.ends_with('\n') {
                    println!();
                }
            }
            let cached_for_caller = if is_last {
                cached
            } else {
                strip_think_tag(&cached).to_string()
            };
            return Ok((cached_for_caller, metrics));
        }
    }

    config.write().before_chat_completion(&input)?;

    // Phase 9C: schema retry budget for this stage. Short-circuits to 0 when
    // the provider enforces the schema natively (Phase 9A/9B).
    let native_structured = role.has_output_schema()
        && role.model().data().supports_response_format_json_schema;
    let max_schema_retries = if role.has_output_schema() && !native_structured {
        role.schema_retries().unwrap_or(1)
    } else {
        0
    };
    let original_input = input.clone();

    // Phase 0B: Use call_react when the stage role has tools
    let (mut output, mut tool_results, mut metrics) = if has_tools {
        call_react(&mut input, client.as_ref(), abort_signal.clone()).await?
    } else if input.stream() && is_last {
        call_chat_completions_streaming(&input, client.as_ref(), abort_signal.clone()).await?
    } else {
        call_chat_completions(&input, false, false, client.as_ref(), abort_signal.clone()).await?
    };

    // Phase 9C: retry loop on output schema failure.
    if let Some(schema) = role.output_schema() {
        if max_schema_retries > 0 {
            let mut attempt: usize = 0;
            loop {
                match validate_schema_traced("output", schema, &output, trace_emitter.as_ref()) {
                    Ok(()) => break,
                    Err(e) if attempt < max_schema_retries => {
                        attempt += 1;
                        let retry_prompt = format!(
                            "Your previous output failed schema validation:\n{e}\n\nPlease regenerate your response to conform to the required schema. Return ONLY valid JSON."
                        );
                        let mut retry_input = original_input
                            .clone()
                            .with_retry_prompt(&output, &retry_prompt);
                        let (new_output, new_tool_results, new_metrics) = if has_tools {
                            call_react(
                                &mut retry_input,
                                client.as_ref(),
                                abort_signal.clone(),
                            )
                            .await?
                        } else {
                            // Never stream during retry: even on the last
                            // stage, the first (failed) output was already
                            // emitted path-suppressed because output_schema
                            // forces stream() == false.
                            call_chat_completions(
                                &retry_input,
                                false,
                                false,
                                client.as_ref(),
                                abort_signal.clone(),
                            )
                            .await?
                        };
                        output = new_output;
                        tool_results = new_tool_results;
                        metrics.merge(&new_metrics);
                        input = retry_input;
                    }
                    Err(e) => return Err(e),
                }
            }
        } else {
            validate_schema_traced("output", schema, &output, trace_emitter.as_ref())?;
        }
    }

    // Phase 10B: persist successful output to the cache. Written before
    // message-history save / printing so a later stage's cache hit sees the
    // exact text we just produced.
    if let Some(key) = &cache_key {
        let cache = StageCache::new(
            Config::local_path(".cache/stages"),
            config.read().cache_ttl_secs,
        );
        if let Err(e) = cache.put(key, &output) {
            debug!("Failed to write stage cache entry: {e}");
        }
    }

    // Only save to message history for the last stage
    if is_last {
        config
            .write()
            .after_chat_completion(&input, &output, &tool_results)?;
    }

    if is_last && !input.stream() {
        let final_output = if let Some(fmt) = config.read().output_format {
            if fmt.is_structured() {
                fmt.clean_output(&output)?
            } else {
                output.to_string()
            }
        } else {
            output.to_string()
        };
        print!("{final_output}");
        std::io::Write::flush(&mut std::io::stdout())?;
        if !final_output.ends_with('\n') {
            println!();
        }
    }

    // Phase 6B: Run lifecycle hooks on the last stage
    if is_last {
        run_lifecycle_hooks(&role, &output)?;
    }

    // Strip think tags from intermediate output
    let output = if !is_last {
        strip_think_tag(&output).to_string()
    } else {
        output
    };
    Ok((output, metrics))
}

fn parse_stages(stage_specs: &[String]) -> Result<Vec<PipelineStage>> {
    stage_specs
        .iter()
        .map(|spec| {
            let (role_name, model_id) = match spec.split_once('@') {
                Some((role, model)) => (role.to_string(), Some(model.to_string())),
                None => (spec.to_string(), None),
            };
            Ok(PipelineStage {
                role_name,
                model_id,
                budget_usd: None,
            })
        })
        .collect()
}

fn load_pipeline_def_nodes(path: &str) -> Result<(Vec<PipelineNode>, Option<f64>)> {
    let path = Path::new(path);
    let content = if path.exists() {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read pipeline definition: {}", path.display()))?
    } else {
        let pipelines_dir = Config::local_path("pipelines");
        let full_path = pipelines_dir.join(format!("{}.yaml", path.display()));
        if full_path.exists() {
            std::fs::read_to_string(&full_path).with_context(|| {
                format!(
                    "Failed to read pipeline definition: {}",
                    full_path.display()
                )
            })?
        } else {
            bail!(
                "Pipeline definition not found: {} or {}",
                path.display(),
                full_path.display()
            );
        }
    };

    let def: PipelineDef =
        serde_yaml::from_str(&content).context("Failed to parse pipeline definition YAML")?;

    if def.pipeline.is_some() && !def.stages.is_empty() {
        bail!(
            "Pipeline definition has both `stages:` and `pipeline:` — pick one. \
             Use `pipeline:` for DAG primitives (parallel/switch) and `stages:` \
             for purely sequential roles."
        );
    }

    let nodes = if let Some(items) = def.pipeline {
        items
            .iter()
            .map(crate::config::role::parse_pipeline_node)
            .collect::<Result<Vec<_>>>()
            .context("Failed to parse `pipeline:` node list")?
    } else {
        def.stages
            .into_iter()
            .map(|s| {
                PipelineNode::Stage(RolePipelineStage {
                    role: s.role,
                    model: s.model,
                    budget_weight: s.budget_weight,
                })
            })
            .collect()
    };
    Ok((nodes, def.budget_usd))
}

/// Phase 17B: aggregated result of invoking a role end-to-end. Returned by
/// [`invoke_role`] to the HTTP server (and any future programmatic caller).
///
/// `metrics` is the sum across all stages; `stages` carries the per-stage
/// breakdown that the server emits when the caller requests `trace: true`.
#[derive(Debug, Clone)]
pub struct InvokeResult {
    pub output: String,
    pub metrics: CallMetrics,
    pub stages: Vec<StageTrace>,
    /// True when the role's `output_schema` validated (or no schema was
    /// declared). `run_stage_inner` returns Err on terminal schema failure,
    /// so reaching this struct already means the output is conformant.
    pub schema_valid: bool,
}

/// Phase 17C: per-stage event used by [`invoke_role_streaming`]. The HTTP
/// server forwards these as SSE `stage.start` / `stage.end` events with the
/// `role` and `trace` fields as data payloads.
#[derive(Debug, Clone)]
pub enum StageEvent {
    Start {
        index: usize,
        total: usize,
        role: String,
        model_override: Option<String>,
    },
    End {
        index: usize,
        role: String,
        trace: StageTrace,
        output: String,
    },
}

/// Phase 17B: programmatic role invocation. Loads the role, walks its
/// pipeline (or a synthetic one-stage pipeline if the role has none), and
/// returns the final text plus aggregated metrics.
///
/// Mirrors the CLI's `run()` flow but stays silent: every stage is invoked
/// with `is_last=false` so nothing prints to stdout and the chat-history
/// save side-effect is suppressed.
///
/// The caller is responsible for setting `config.role_variables` (and any
/// model override via `config.set_model`) BEFORE invoking — `run_stage`
/// reads those at stage start.
pub async fn invoke_role(
    config: &GlobalConfig,
    role_name: &str,
    input_text: &str,
    abort_signal: AbortSignal,
) -> Result<InvokeResult> {
    let role = config
        .read()
        .retrieve_role(role_name)
        .with_context(|| format!("Failed to load role '{role_name}'"))?;

    // Phase 21: a DAG pipeline (fan-out or switch) routes through `run_node`
    // for concurrent / conditional execution. Purely sequential pipelines
    // keep the existing flat-list fast path so behavior is unchanged for
    // pre-DAG callers.
    let pipeline_budget_usd = role.pipeline_budget_usd();

    if role.pipeline_has_dag() {
        let nodes = role.pipeline().expect("DAG implies pipeline present").to_vec();
        let stage_tuples = collect_preflight_stages(&nodes);
        crate::config::preflight::validate_pipeline_stages(&config.read(), &stage_tuples)?;
        crate::config::preflight::validate_pipeline_dag_structure(&nodes)
            .context("Pipeline DAG validation failed")?;

        // Phase 11D: same top-level allocation as `run()` — only leaf Stage
        // nodes get a per-stage share; nested DAG nodes carry `None`.
        let per_node_budgets: Option<Vec<f64>> = pipeline_budget_usd
            .filter(|b| *b > 0.0)
            .map(|total| {
                let weights: Vec<Option<f64>> = nodes
                    .iter()
                    .map(|n| match n {
                        PipelineNode::Stage(s) => s.budget_weight,
                        _ => None,
                    })
                    .collect();
                crate::context_budget::allocate_stage_budgets(&weights, total)
            });

        let node_count = nodes.len();
        let mut current = input_text.to_string();
        let mut traces: Vec<StageTrace> = Vec::new();
        for (i, node) in nodes.iter().enumerate() {
            let stage_budget = per_node_budgets.as_ref().map(|v| v[i]);
            let (out, mut t) = run_node(
                config,
                node,
                i,
                node_count,
                &current,
                false,
                None,
                stage_budget,
                abort_signal.clone(),
            )
            .await?;
            traces.append(&mut t);
            current = out;
        }

        let total = CallMetrics {
            input_tokens: traces.iter().map(|t| t.input_tokens).sum(),
            output_tokens: traces.iter().map(|t| t.output_tokens).sum(),
            cost_usd: traces.iter().map(|t| t.cost_usd).sum(),
            latency_ms: traces.iter().map(|t| t.latency_ms).sum(),
            model_id: traces.last().map(|t| t.model.clone()).unwrap_or_default(),
            ..Default::default()
        };
        return Ok(InvokeResult {
            output: current,
            metrics: total,
            stages: traces,
            schema_valid: true,
        });
    }

    let pipeline_stages: Vec<PipelineStage> = if let Some(stages) = role.pipeline_sequential() {
        // Phase 11D: allocate the role's `pipeline_budget_usd` proportionally
        // across stages by `budget_weight`.
        let per_stage_budgets: Option<Vec<f64>> = pipeline_budget_usd
            .filter(|b| *b > 0.0)
            .map(|total| {
                let weights: Vec<Option<f64>> =
                    stages.iter().map(|s| s.budget_weight).collect();
                crate::context_budget::allocate_stage_budgets(&weights, total)
            });
        stages
            .iter()
            .enumerate()
            .map(|(i, s)| PipelineStage {
                role_name: s.role.clone(),
                model_id: s.model.clone(),
                budget_usd: per_stage_budgets.as_ref().map(|v| v[i]),
            })
            .collect()
    } else {
        // Non-pipeline role: run it as a single-stage pipeline so we get the
        // same retry / fallback / preflight machinery for free. The whole
        // `pipeline_budget_usd` (if any) applies to this single stage.
        vec![PipelineStage {
            role_name: role_name.to_string(),
            model_id: None,
            budget_usd: pipeline_budget_usd.filter(|b| *b > 0.0),
        }]
    };

    // Phase 9D preflight applies to inline runs too — surface model/tool
    // mismatches before any LLM call.
    {
        let stage_tuples: Vec<(String, Option<String>)> = pipeline_stages
            .iter()
            .map(|s| (s.role_name.clone(), s.model_id.clone()))
            .collect();
        crate::config::preflight::validate_pipeline_stages(&config.read(), &stage_tuples)?;
    }

    let stage_count = pipeline_stages.len();
    let mut current = input_text.to_string();
    let mut total = CallMetrics::default();
    let mut traces: Vec<StageTrace> = Vec::with_capacity(stage_count);

    for (i, stage) in pipeline_stages.iter().enumerate() {
        let (out, m) = run_stage(
            config,
            stage,
            i,
            stage_count,
            &current,
            // Always false — server callers want the text back, not stdout.
            false,
            abort_signal.clone(),
        )
        .await?;
        traces.push(StageTrace {
            role: stage.role_name.clone(),
            model: m.model_id.clone(),
            input_tokens: m.input_tokens,
            output_tokens: m.output_tokens,
            cost_usd: m.cost_usd,
            latency_ms: m.latency_ms,
            branch: None,
        });
        total.merge(&m);
        current = out;
    }

    Ok(InvokeResult {
        output: current,
        metrics: total,
        stages: traces,
        schema_valid: true,
    })
}

/// Phase 17C: streaming variant of [`invoke_role`]. Emits a `StageEvent`
/// over `tx` at each stage boundary so the HTTP server can forward them
/// as SSE events. Returns the same `InvokeResult` as the non-streaming
/// variant.
///
/// Caveat: this gives *stage*-granularity streaming, not token-granularity.
/// `run_stage` runs each stage to completion before returning, so the
/// per-stage `output` text arrives in the `End` event. Token streaming
/// during a stage would require rewiring `run_stage_inner` to expose an
/// `SseHandler`, which the design defers to a future iteration.
pub async fn invoke_role_streaming(
    config: &GlobalConfig,
    role_name: &str,
    input_text: &str,
    abort_signal: AbortSignal,
    tx: tokio::sync::mpsc::UnboundedSender<StageEvent>,
) -> Result<InvokeResult> {
    let role = config
        .read()
        .retrieve_role(role_name)
        .with_context(|| format!("Failed to load role '{role_name}'"))?;

    // Phase 21: DAG pipelines stream stage-by-stage in DAG traversal order.
    // Fan-out branches emit Start/End sequentially per the iterator order
    // of `run_node` collection — the underlying execution is concurrent,
    // but the SSE channel sees each branch's End event when its future
    // resolves. For purely sequential pipelines, behavior is identical to
    // the pre-Phase-21 implementation.
    let pipeline_budget_usd = role.pipeline_budget_usd();

    if role.pipeline_has_dag() {
        let nodes = role.pipeline().expect("DAG implies pipeline present").to_vec();
        let stage_tuples = collect_preflight_stages(&nodes);
        crate::config::preflight::validate_pipeline_stages(&config.read(), &stage_tuples)?;
        crate::config::preflight::validate_pipeline_dag_structure(&nodes)
            .context("Pipeline DAG validation failed")?;

        let per_node_budgets: Option<Vec<f64>> = pipeline_budget_usd
            .filter(|b| *b > 0.0)
            .map(|total| {
                let weights: Vec<Option<f64>> = nodes
                    .iter()
                    .map(|n| match n {
                        PipelineNode::Stage(s) => s.budget_weight,
                        _ => None,
                    })
                    .collect();
                crate::context_budget::allocate_stage_budgets(&weights, total)
            });

        let node_count = nodes.len();
        let mut current = input_text.to_string();
        let mut traces: Vec<StageTrace> = Vec::new();
        for (i, node) in nodes.iter().enumerate() {
            // We can't emit per-leaf Start events because `run_node` is
            // opaque to the streamer. Emit one node-level Start so the
            // client at least sees progress; the End event carries the
            // full trace list returned by the node.
            let leaf_label = match node {
                PipelineNode::Stage(s) => s.role.clone(),
                PipelineNode::Parallel(_) => format!("parallel#{}", i + 1),
                PipelineNode::Switch(_) => format!("switch#{}", i + 1),
            };
            let _ = tx.send(StageEvent::Start {
                index: i,
                total: node_count,
                role: leaf_label.clone(),
                model_override: None,
            });
            let stage_budget = per_node_budgets.as_ref().map(|v| v[i]);
            let (out, mut t) = run_node(
                config,
                node,
                i,
                node_count,
                &current,
                false,
                None,
                stage_budget,
                abort_signal.clone(),
            )
            .await?;
            // Surface the *last* leaf trace as the node's "end" trace so
            // existing SSE consumers keep one trace per Start event.
            let summary_trace = t.last().cloned().unwrap_or_else(|| StageTrace {
                role: leaf_label.clone(),
                model: String::new(),
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: 0.0,
                latency_ms: 0,
                branch: None,
            });
            let _ = tx.send(StageEvent::End {
                index: i,
                role: leaf_label,
                trace: summary_trace,
                output: out.clone(),
            });
            traces.append(&mut t);
            current = out;
        }
        let total = CallMetrics {
            input_tokens: traces.iter().map(|t| t.input_tokens).sum(),
            output_tokens: traces.iter().map(|t| t.output_tokens).sum(),
            cost_usd: traces.iter().map(|t| t.cost_usd).sum(),
            latency_ms: traces.iter().map(|t| t.latency_ms).sum(),
            model_id: traces.last().map(|t| t.model.clone()).unwrap_or_default(),
            ..Default::default()
        };
        return Ok(InvokeResult {
            output: current,
            metrics: total,
            stages: traces,
            schema_valid: true,
        });
    }

    let pipeline_stages: Vec<PipelineStage> = if let Some(stages) = role.pipeline_sequential() {
        let per_stage_budgets: Option<Vec<f64>> = pipeline_budget_usd
            .filter(|b| *b > 0.0)
            .map(|total| {
                let weights: Vec<Option<f64>> =
                    stages.iter().map(|s| s.budget_weight).collect();
                crate::context_budget::allocate_stage_budgets(&weights, total)
            });
        stages
            .iter()
            .enumerate()
            .map(|(i, s)| PipelineStage {
                role_name: s.role.clone(),
                model_id: s.model.clone(),
                budget_usd: per_stage_budgets.as_ref().map(|v| v[i]),
            })
            .collect()
    } else {
        vec![PipelineStage {
            role_name: role_name.to_string(),
            model_id: None,
            budget_usd: pipeline_budget_usd.filter(|b| *b > 0.0),
        }]
    };

    // Preflight matches the non-streaming path so a mismatched stage fails
    // before we start emitting events.
    {
        let stage_tuples: Vec<(String, Option<String>)> = pipeline_stages
            .iter()
            .map(|s| (s.role_name.clone(), s.model_id.clone()))
            .collect();
        crate::config::preflight::validate_pipeline_stages(&config.read(), &stage_tuples)?;
    }

    let stage_count = pipeline_stages.len();
    let mut current = input_text.to_string();
    let mut total = CallMetrics::default();
    let mut traces: Vec<StageTrace> = Vec::with_capacity(stage_count);

    for (i, stage) in pipeline_stages.iter().enumerate() {
        let _ = tx.send(StageEvent::Start {
            index: i,
            total: stage_count,
            role: stage.role_name.clone(),
            model_override: stage.model_id.clone(),
        });
        let (out, m) = run_stage(
            config,
            stage,
            i,
            stage_count,
            &current,
            false,
            abort_signal.clone(),
        )
        .await?;
        let trace = StageTrace {
            role: stage.role_name.clone(),
            model: m.model_id.clone(),
            input_tokens: m.input_tokens,
            output_tokens: m.output_tokens,
            cost_usd: m.cost_usd,
            latency_ms: m.latency_ms,
            branch: None,
        };
        let _ = tx.send(StageEvent::End {
            index: i,
            role: stage.role_name.clone(),
            trace: trace.clone(),
            output: out.clone(),
        });
        traces.push(trace);
        total.merge(&m);
        current = out;
    }

    Ok(InvokeResult {
        output: current,
        metrics: total,
        stages: traces,
        schema_valid: true,
    })
}

/// Phase 17D: stage descriptor accepted by the HTTP pipeline-run endpoint.
/// Same shape as the YAML pipeline-def schema (`role:` + optional `model:`),
/// hoisted to the public API so the server can deserialize inline stages
/// without re-defining the type.
#[derive(Debug, Clone, Deserialize)]
pub struct InlineStage {
    pub role: String,
    #[serde(default)]
    pub model: Option<String>,
}

/// Phase 17D: run an arbitrary list of `InlineStage`s. Used by the
/// `/v1/pipelines/run` endpoint (and by 17E batch). Returns the same
/// `InvokeResult` envelope as [`invoke_role`].
pub async fn run_inline_pipeline(
    config: &GlobalConfig,
    stages: &[InlineStage],
    input_text: &str,
    abort_signal: AbortSignal,
) -> Result<InvokeResult> {
    if stages.is_empty() {
        bail!("Pipeline has no stages");
    }
    let pipeline_stages: Vec<PipelineStage> = stages
        .iter()
        .map(|s| PipelineStage {
            role_name: s.role.clone(),
            model_id: s.model.clone(),
            budget_usd: None,
        })
        .collect();
    {
        let stage_tuples: Vec<(String, Option<String>)> = pipeline_stages
            .iter()
            .map(|s| (s.role_name.clone(), s.model_id.clone()))
            .collect();
        crate::config::preflight::validate_pipeline_stages(&config.read(), &stage_tuples)?;
    }
    let stage_count = pipeline_stages.len();
    let mut current = input_text.to_string();
    let mut total = CallMetrics::default();
    let mut traces: Vec<StageTrace> = Vec::with_capacity(stage_count);
    for (i, stage) in pipeline_stages.iter().enumerate() {
        let (out, m) = run_stage(
            config,
            stage,
            i,
            stage_count,
            &current,
            false,
            abort_signal.clone(),
        )
        .await?;
        traces.push(StageTrace {
            role: stage.role_name.clone(),
            model: m.model_id.clone(),
            input_tokens: m.input_tokens,
            output_tokens: m.output_tokens,
            cost_usd: m.cost_usd,
            latency_ms: m.latency_ms,
            branch: None,
        });
        total.merge(&m);
        current = out;
    }
    Ok(InvokeResult {
        output: current,
        metrics: total,
        stages: traces,
        schema_valid: true,
    })
}

/// Phase 17D: load a named pipeline definition from `<config>/pipelines/<name>.yaml`.
/// Returns the parsed stage list. Used by the server's `/v1/pipelines/run`
/// endpoint when a request specifies `pipeline: "name"`.
pub fn load_pipeline_stages(name: &str) -> Result<Vec<InlineStage>> {
    let pipelines_dir = Config::local_path("pipelines");
    let path = pipelines_dir.join(format!("{name}.yaml"));
    if !path.exists() {
        bail!("Pipeline '{name}' not found at {}", path.display());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read pipeline '{name}': {}", path.display()))?;
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        stages: Vec<InlineStage>,
    }
    let file: File = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse pipeline '{name}' YAML"))?;
    if file.stages.is_empty() {
        bail!("Pipeline '{name}' has no stages");
    }
    Ok(file.stages)
}

/// Run a pipeline defined in a role's frontmatter. Called from tool dispatch.
/// Returns the final output text.
pub async fn run_pipeline_role(
    config: &GlobalConfig,
    nodes: &[PipelineNode],
    input_text: &str,
) -> Result<String> {
    if nodes.is_empty() {
        bail!("Pipeline role has no stages");
    }

    // Phase 21D: structural + reachability checks before any LLM call.
    crate::config::preflight::validate_pipeline_dag_structure(nodes)
        .context("Pipeline DAG validation failed")?;

    let abort_signal = create_abort_signal();
    let node_count = nodes.len();
    let mut current_input = input_text.to_string();

    for (i, node) in nodes.iter().enumerate() {
        // Pipeline-as-tool: never print output, the caller consumes it.
        // Phase 11D: pipeline-as-tool doesn't currently surface budget; the
        // caller can set it via the role's `pipeline_budget_usd` when invoking.
        let (output, _traces) = run_node(
            config,
            node,
            i,
            node_count,
            &current_input,
            false,
            None,
            None,
            abort_signal.clone(),
        )
        .await?;
        current_input = output;
    }

    Ok(current_input)
}

/// Phase 21: recursively execute a pipeline DAG node and return its
/// produced text plus the flat list of leaf-stage traces it generated.
///
/// `branch_label` is `Some(n)` only when we're inside a fan-out — it's
/// stamped onto every trace produced by this subtree so the JSON envelope
/// shows which branch each stage belongs to.
fn run_node<'a>(
    config: &'a GlobalConfig,
    node: &'a PipelineNode,
    node_index: usize,
    node_count: usize,
    input_text: &'a str,
    is_last: bool,
    branch_label: Option<usize>,
    stage_budget_usd: Option<f64>,
    abort_signal: AbortSignal,
) -> futures_util::future::BoxFuture<'a, Result<(String, Vec<StageTrace>)>> {
    Box::pin(async move {
        match node {
            PipelineNode::Stage(s) => {
                let stage = PipelineStage {
                    role_name: s.role.clone(),
                    model_id: s.model.clone(),
                    budget_usd: stage_budget_usd,
                };
                let (output, metrics) = run_stage(
                    config,
                    &stage,
                    node_index,
                    node_count,
                    input_text,
                    is_last,
                    abort_signal,
                )
                .await?;
                let trace = StageTrace {
                    role: s.role.clone(),
                    model: metrics.model_id.clone(),
                    input_tokens: metrics.input_tokens,
                    output_tokens: metrics.output_tokens,
                    cost_usd: metrics.cost_usd,
                    latency_ms: metrics.latency_ms,
                    branch: branch_label,
                };
                Ok((output, vec![trace]))
            }
            PipelineNode::Parallel(p) => {
                run_parallel(
                    config,
                    p,
                    node_index,
                    node_count,
                    input_text,
                    is_last,
                    branch_label,
                    abort_signal,
                )
                .await
            }
            PipelineNode::Switch(s) => {
                run_switch(
                    config,
                    s,
                    node_index,
                    node_count,
                    input_text,
                    is_last,
                    branch_label,
                    abort_signal,
                )
                .await
            }
        }
    })
}

/// Phase 21A/21C: fan out the same input across N branches, await all,
/// then combine their outputs via the configured merge strategy.
async fn run_parallel(
    config: &GlobalConfig,
    p: &ParallelNode,
    node_index: usize,
    node_count: usize,
    input_text: &str,
    is_last: bool,
    branch_label: Option<usize>,
    abort_signal: AbortSignal,
) -> Result<(String, Vec<StageTrace>)> {
    // Each branch sees the same input. We don't propagate `is_last=true`
    // into branches: a branch's output is consumed by the merge, never
    // printed directly. The merged output is what propagates downstream.
    let branch_count = p.branches.len();
    let futs = p.branches.iter().enumerate().map(|(bi, branch)| {
        let stamp = match branch_label {
            // Preserve the outermost branch label for nested fans.
            Some(outer) => Some(outer),
            None => Some(bi + 1),
        };
        run_node(
            config,
            branch,
            node_index,
            node_count,
            input_text,
            false,
            stamp,
            // Phase 11D: nested DAG budget propagation deferred; branches
            // consume their model's native context window for now.
            None,
            abort_signal.clone(),
        )
    });
    let results: Vec<Result<(String, Vec<StageTrace>)>> = join_all(futs).await;

    let mut outputs: Vec<String> = Vec::with_capacity(branch_count);
    let mut traces: Vec<StageTrace> = Vec::new();
    for r in results {
        let (out, mut t) = r?;
        outputs.push(out);
        traces.append(&mut t);
    }

    let merged = match &p.merge {
        MergeStrategy::Concatenate => outputs.join("\n---\n"),
        MergeStrategy::JsonArray => {
            // Try to preserve each branch output's native JSON shape;
            // fall back to a string element when the branch produced
            // non-JSON text.
            let arr: Vec<serde_json::Value> = outputs
                .iter()
                .map(|s| {
                    serde_json::from_str::<serde_json::Value>(s)
                        .unwrap_or_else(|_| serde_json::Value::String(s.clone()))
                })
                .collect();
            serde_json::to_string(&arr).context("Failed to serialize json_array merge")?
        }
        MergeStrategy::CustomRole(role_name) => {
            let stage = PipelineStage {
                role_name: role_name.clone(),
                model_id: None,
                budget_usd: None,
            };
            let concatenated = outputs.join("\n---\n");
            let (out, metrics) = run_stage(
                config,
                &stage,
                node_index,
                node_count,
                &concatenated,
                is_last,
                abort_signal,
            )
            .await?;
            traces.push(StageTrace {
                role: role_name.clone(),
                model: metrics.model_id.clone(),
                input_tokens: metrics.input_tokens,
                output_tokens: metrics.output_tokens,
                cost_usd: metrics.cost_usd,
                latency_ms: metrics.latency_ms,
                branch: branch_label,
            });
            return Ok((out, traces));
        }
    };

    // For built-in merges (concatenate / json_array), the parallel node
    // itself doesn't run an extra stage. If this node is the last in the
    // top-level pipeline and we're printing, emit the merged output here.
    if is_last {
        print_final_output(config, &merged)?;
    }

    Ok((merged, traces))
}

/// Phase 21B: pick the first branch whose `when:` predicate matches the
/// prior output (or the `otherwise:` fallback) and execute it.
async fn run_switch(
    config: &GlobalConfig,
    s: &SwitchNode,
    node_index: usize,
    node_count: usize,
    input_text: &str,
    is_last: bool,
    branch_label: Option<usize>,
    abort_signal: AbortSignal,
) -> Result<(String, Vec<StageTrace>)> {
    let mut chosen: Option<&PipelineNode> = None;
    for b in &s.branches {
        match &b.predicate {
            Some(p) => {
                if p.evaluate(input_text) {
                    chosen = Some(&b.node);
                    break;
                }
            }
            None => {
                // Defer the `otherwise:` until after all `when:` branches
                // failed — guaranteed by parse order since `otherwise:`
                // can appear anywhere but only matches when no `when:`
                // does. The loop continues; if a later `when:` matches,
                // it still wins.
            }
        }
    }
    if chosen.is_none() {
        chosen = s
            .branches
            .iter()
            .find(|b| b.predicate.is_none())
            .map(|b| b.node.as_ref());
    }

    let node = chosen.ok_or_else(|| {
        anyhow::anyhow!(
            "Switch routed no branch: no `when:` matched and no `otherwise:` defined"
        )
    })?;

    run_node(
        config,
        node,
        node_index,
        node_count,
        input_text,
        is_last,
        branch_label,
        // Phase 11D: switch arm inherits no per-stage budget — the routing
        // decision happens after the prior stage's spend; arm-level budgets
        // would need a separate config surface.
        None,
        abort_signal,
    )
    .await
}

/// Phase 21: print the final pipeline output when a fan-out lands on the
/// last position of the top-level pipeline. Mirrors the printing block in
/// `run_stage_inner` for sequential stages.
fn print_final_output(config: &GlobalConfig, output: &str) -> Result<()> {
    let final_output = if let Some(fmt) = config.read().output_format {
        if fmt.is_structured() {
            fmt.clean_output(output)?
        } else {
            output.to_string()
        }
    } else {
        output.to_string()
    };
    print!("{final_output}");
    std::io::Write::flush(&mut std::io::stdout())?;
    if !final_output.ends_with('\n') {
        println!();
    }
    Ok(())
}
