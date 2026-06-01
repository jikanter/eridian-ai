use crate::cache::StageCache;
use crate::cli::Cli;
use crate::client::{
    call_chat_completions, call_chat_completions_streaming, call_react, CallMetrics,
};
use crate::config::{
    pipeline_stage_admissible, run_lifecycle_hooks, validate_schema_traced, Agent, Config,
    EntityRef, GlobalConfig, Input, MergeStrategy, ParallelNode, PartialConfig, PipelineNode, Role,
    RoleLike, RolePipelineStage, SwitchNode,
};
use crate::utils::*;

use anyhow::{bail, Context, Result};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Default)]
struct PipelineStage {
    role_name: String,
    model_id: Option<String>,
    /// Phase 11D: pre-allocated dollar budget for this stage. `None` means
    /// no enforcement; the stage runs with the model's native context window
    /// as its only limit. When `Some`, `run_stage_inner` tail-truncates the
    /// post-knowledge input text to fit `budget_usd_to_input_token_cap`.
    budget_usd: Option<f64>,
    /// Phase 36A: opt-in config isolation for this stage (see
    /// `RolePipelineStage::config_override`). `None` ⇒ shared `Config`, as
    /// before this phase. Carried through retries/fallbacks (`attempt_stage`)
    /// so isolation survives a model fallback.
    config_override: Option<PartialConfig>,
}

/// Phase 17B: per-stage execution trace. Public so server-side invocation
/// can include it in the response envelope (`trace: true`) and the CLI can
/// emit it under `-o json`.
///
/// Phase 21: `branch` is set when this stage ran inside a fan-out — its
/// value is the 1-based branch number within the parent `parallel:` node.
///
/// Phase 22A/22D: `node_index` is the 0-based position of the *top-level*
/// pipeline node this stage belongs to — fan-out branches and switch arms
/// inherit their enclosing node's index, so consumers can group a flat trace
/// list back into the DAG. `cached` is true when the stage's output was
/// replayed from the content-addressable stage cache instead of an LLM call.
#[derive(Serialize, Clone, Debug, Default)]
pub struct StageTrace {
    pub role: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<usize>,
    #[serde(default)]
    pub node_index: usize,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cached: bool,
    /// Phase 36D: names of the `config_override:` fields applied to this stage
    /// (e.g. `["use_tools","working_directory"]`). Empty/absent for a stage
    /// that declared no override. `-o json` consumers see exactly what changed
    /// per stage; `-o text` / the trace tree omit it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config_overrides_applied: Vec<String>,
}

/// serde `skip_serializing_if` predicate for booleans — keeps `cached: false`
/// out of the JSON envelope so the trace stays terse for the common case.
fn is_false(b: &bool) -> bool {
    !*b
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

/// Phase 21 / 15C: assemble the pipeline DAG from CLI flags. `--pipe-def`
/// may carry a full DAG and a top-level `budget_usd:`; `--stage` is always a
/// sequential leaf list with no budget surface. Shared by `run` (execution)
/// and `run_check` (validation only — it discards the budget).
fn build_pipeline_nodes(cli: &Cli) -> Result<(Vec<PipelineNode>, Option<f64>)> {
    // Phase 11D: `--pipe-def` files may also declare `budget_usd:` at the
    // top level; the CLI `--stage` form has no budget surface yet.
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
                            // CLI `--stage` carries no override surface.
                            config_override: None,
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
    Ok((nodes, pipeline_budget_usd))
}

pub async fn run(config: GlobalConfig, cli: Cli, text: Option<String>) -> Result<()> {
    let (nodes, pipeline_budget_usd) = build_pipeline_nodes(&cli)?;

    // Phase 11D: allocate per-top-level-node dollar budgets. Only leaf Stage
    // nodes carry a `budget_weight`; parallel/switch nodes weight as 1.0 and
    // receive a node-level share. Phase 22C then sub-allocates that share —
    // `run_parallel` splits it across branches, `run_switch` hands it to the
    // chosen arm.
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
        // Phase 36C: the CLI `--stage` / `--pipe-def` forms carry no parent role
        // and no `config_override:`, so the escalation guard is a no-op here —
        // it runs in the role-frontmatter paths (`invoke_role[_streaming]`).
    }
    // Phase 33D: adjacent-stage shape check (sequential pipelines only).
    preflight_shape(&config, &nodes)?;

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

    // Phase 22A: when we'll emit a JSON trace envelope below, tell the stages to
    // stay silent so stdout is exactly the envelope (one valid JSON document).
    let emits_envelope = matches!(output_format, Some(crate::cli::OutputFormat::Json));
    config.write().pipeline_emits_envelope = emits_envelope;

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

    // Phase 22A/23C: the pipeline label is the pipeline-def name when one was
    // given, otherwise the generic "pipeline". Shared by the trace tree (22A)
    // and the per-stage run-log records (23C).
    let label = cli
        .pipe_def
        .as_deref()
        .map(|p| {
            Path::new(p)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(p)
                .to_string()
        })
        .unwrap_or_else(|| "pipeline".to_string());

    // Phase 23C: cost attribution by role. Append one run-log record per stage,
    // all sharing a single pipeline-run id, so downstream tooling can
    // `GROUP BY stage_role`. The single-role path writes its own record in
    // `start_directive`; this is the pipeline equivalent.
    let run_log = config.read().run_log.clone();
    if let Some(log_path) = run_log {
        let log_path = std::path::PathBuf::from(log_path);
        let run_id = uuid::Uuid::new_v4().to_string();
        for (i, trace) in stage_traces.iter().enumerate() {
            let record = stage_run_log_record(&run_id, &label, i + 1, trace);
            if let Err(e) = crate::utils::ledger::append_run_log(&log_path, &record) {
                warn!("Failed to write pipeline run log: {e}");
            }
        }
    }

    // Phase 22A: render the DAG execution as a tree on stderr when `--trace`
    // (human trace) is active.
    let human_trace = config
        .read()
        .trace_config
        .as_ref()
        .map(|t| t.human_trace)
        .unwrap_or(false);
    if human_trace {
        eprint!("{}", render_trace_tree(&label, &nodes, &stage_traces));
    }

    // JSON envelope with trace metadata when output format is JSON
    if matches!(output_format, Some(crate::cli::OutputFormat::Json)) {
        let total_cost: f64 = stage_traces.iter().map(|s| s.cost_usd).sum();
        // Phase 22A: `total_latency_ms` is the sequential sum (preserved for
        // back-compat); `wall_latency_ms` accounts for fan-out concurrency.
        let (wall_latency, total_latency) = pipeline_timing(&stage_traces);
        let envelope = serde_json::json!({
            "output": serde_json::from_str::<serde_json::Value>(&input_text).unwrap_or(serde_json::Value::String(input_text)),
            "trace": {
                "stages": stage_traces,
                "total_cost_usd": total_cost,
                "total_latency_ms": total_latency,
                "wall_latency_ms": wall_latency,
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

/// Phase 33D: run the adjacent-stage shape check before executing a pipeline.
/// Only purely-sequential pipelines have well-defined output→input boundaries,
/// so a DAG with fan-out/switch is skipped here (its structure is validated
/// separately; cross-branch shape checking is out of scope).
fn preflight_shape(config: &GlobalConfig, nodes: &[PipelineNode]) -> Result<()> {
    if let Some(seq) = sequential_stage_tuples(nodes) {
        crate::config::preflight::validate_pipeline_shape(&config.read(), &seq)?;
    }
    Ok(())
}

/// Phase 36C: run the escalation guard over every stage that declared a
/// `config_override:`. The parent (pipeline-owning) `role` supplies the
/// permission ceiling. A no-op for `--stage` / inline-server pipelines (no
/// parent role, no overrides). Called from the same preflight blocks that
/// already run `validate_pipeline_stages`.
fn preflight_overrides(
    config: &GlobalConfig,
    parent_role: &Role,
    stages: &[RolePipelineStage],
) -> Result<()> {
    let overrides: Vec<(usize, String, PartialConfig)> = stages
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.config_override.clone().map(|ov| (i, s.role.clone(), ov)))
        .collect();
    if overrides.is_empty() {
        return Ok(());
    }
    let parent_cwd = config
        .read()
        .working_directory
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    crate::config::preflight::validate_pipeline_overrides(
        parent_role.name(),
        parent_role.use_tools().as_deref(),
        &parent_cwd,
        &overrides,
    )
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
    // Phase 36B: config isolation seam. When the stage declares a non-empty
    // `config_override:`, run it against a *clone* of the global `Config` with
    // the config-scoped fields (`working_directory`, `mcp_servers`) merged in.
    // The clone is dropped when this stage returns, so mutations never leak to
    // the parent config or sibling stages — mirroring the macro-isolation
    // pattern in `config/resolver.rs`. Role-scoped fields (tools, sampling) are
    // applied to the stage's own role inside `run_stage_inner`. Stages without
    // an override share the parent handle, byte-for-byte today's behavior.
    let owned_config: Option<GlobalConfig> = match &stage.config_override {
        Some(p) if !p.is_empty() => {
            let mut cloned = config.read().clone();
            cloned.apply_partial(p)?;
            Some(std::sync::Arc::new(parking_lot::RwLock::new(cloned)))
        }
        _ => None,
    };
    let config: &GlobalConfig = owned_config.as_ref().unwrap_or(config);

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
            // Carry isolation forward across model fallbacks.
            config_override: stage.config_override.clone(),
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
                stage_index,
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
    stage_index: usize,
    input_text: &str,
    is_last: bool,
    abort_signal: AbortSignal,
) -> Result<(String, CallMetrics)> {
    let target = resolve_stage_entity(config, &stage.role_name, abort_signal.clone()).await?;
    let mut role = match target {
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
            if is_last && !output.is_empty() && !json_envelope_mode(config) {
                print!("{output}");
                std::io::Write::flush(&mut std::io::stdout())?;
                if !output.ends_with('\n') {
                    println!();
                }
            }
            return Ok((output, result.metrics));
        }
    };

    // Phase 36B: apply the role-scoped fields of the stage override (tools,
    // sampling) directly to this stage's resolved role. The pipeline path
    // selects tools and sampling off the role, not the global `Config`, so
    // these must land here to take effect. The role is a local clone, so the
    // mutation is naturally isolated to this stage. Config-scoped fields
    // (`working_directory`, `mcp_servers`) were already merged into the cloned
    // config by `run_stage`.
    if let Some(p) = &stage.config_override {
        p.apply_to_role(&mut role);
    }

    if let Some(model_id) = &stage.model_id {
        config.write().set_model(model_id)?;
    }

    let trace_emitter = config
        .read()
        .trace_config
        .clone()
        .map(crate::utils::trace::TraceEmitter::new);

    // Phase 33C: skip raw-message validation for a stdin-routed role (its schema
    // describes slots, and the message is the free-text body slot).
    if let Some(schema) = role.input_schema().filter(|_| !role.has_stdin_slot()) {
        if let Err(e) =
            validate_schema_traced("input", schema, input_text, trace_emitter.as_ref())
        {
            // Phase 13B: replace the terse validator error with a teaching one
            // that shows the producer→consumer field delta and a fork-role
            // hint. Reuses the `format_pipeline_input_schema_error` helper; the
            // underlying message still carries the "Schema input validation
            // failed" phrase so error classification is unchanged.
            bail!(
                "{}",
                crate::config::format_pipeline_input_schema_error(
                    stage_index + 1,
                    &stage.role_name,
                    schema,
                    input_text,
                    &e.to_string(),
                )
            );
        }
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
        // Phase 36B: fold the stage override's value fingerprint into the role
        // component of the key so a sampling override (e.g. temperature 0.0 vs
        // 1.0) correctly invalidates a previously cached stage output — those
        // values change the output but not the model id or input text.
        let cache_role_key = match &stage.config_override {
            Some(p) if !p.is_empty() => {
                format!("{}\u{1e}{}", stage.role_name, p.cache_fingerprint())
            }
            _ => stage.role_name.clone(),
        };
        // Hash the post-injection text so a change in the knowledge context
        // (new bindings, recompiled KB) invalidates the cache entry.
        Some(StageCache::key(
            &cache_role_key,
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
            // Phase 22D: flag the replay so the DAG trace can mark this stage
            // `(cached)` and attribute $0 / 0ms to it.
            let metrics = CallMetrics {
                model_id,
                turns: 1,
                cached: true,
                ..Default::default()
            };
            if is_last && !input.stream() && !json_envelope_mode(config) {
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

    // Phase 0B: Use call_react when the stage role has tools.
    // Phase 22A: when the run will emit a JSON envelope, the last stage must
    // compute silently — streaming would print tokens straight to stdout ahead
    // of the envelope. (In the `--pipe` path `output_format` is None, so
    // `input.stream()` isn't already disabled the way it is for a structured
    // single-shot call.)
    let stream_last = input.stream() && is_last && !json_envelope_mode(config);
    let (mut output, mut tool_results, mut metrics) = if has_tools {
        call_react(&mut input, client.as_ref(), abort_signal.clone()).await?
    } else if stream_last {
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

    if is_last && !input.stream() && !json_envelope_mode(config) {
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
                config_override: None,
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
                    // Legacy `--pipe-def stages:` form has no override surface;
                    // use `pipeline:` with `config_override:` for isolation.
                    config_override: None,
                })
            })
            .collect()
    };
    Ok((nodes, def.budget_usd))
}

// ---------------------------------------------------------------------------
// Phase 15C: `--check` — validate a role/pipeline definition without running it.
// ---------------------------------------------------------------------------

use crate::config::preflight::{
    validate_pipeline_dag_cycles, validate_pipeline_dag_structure,
    validate_pipeline_schema_containment, validate_pipeline_stages, BoundaryReport,
    ContainmentVerdict,
};

/// Exit code 3 (ConfigError): the definition is invalid.
const CHECK_EXIT_INVALID: i32 = 3;
/// Exit code 2 (UsageError): nothing to check.
const CHECK_EXIT_USAGE: i32 = 2;

/// Phase 15C: entry point for `--check`. Validates the role named by `-r`, or
/// the ad-hoc pipeline described by `--pipe --stage/--pipe-def`, without making
/// any LLM call. Prints a human report (or JSON with `-o json`) and returns the
/// process exit code: 0 valid, 3 invalid, 2 usage.
pub async fn run_check(config: &GlobalConfig, cli: &Cli) -> Result<i32> {
    let json = matches!(cli.output_format, Some(crate::cli::OutputFormat::Json));

    if cli.pipe {
        // `--check` validates structure/contracts; the budget is irrelevant.
        let nodes = match build_pipeline_nodes(cli) {
            Ok((n, _budget)) => n,
            Err(e) => {
                emit_check_error(json, "pipeline", &e.to_string());
                return Ok(CHECK_EXIT_INVALID);
            }
        };
        return Ok(check_pipeline(config, "<pipeline>", &nodes, json));
    }

    if let Some(role_name) = &cli.role {
        return Ok(check_role(config, role_name, json));
    }

    // Nothing to check.
    let msg = "--check requires a target: -r <role>, --pipe --stage <role>…, or --pipe --pipe-def <file>";
    if json {
        let payload = serde_json::json!({ "valid": false, "error": msg });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    } else {
        eprintln!("error: {msg}");
    }
    Ok(CHECK_EXIT_USAGE)
}

/// Validate a single role. If it declares a `pipeline:`, defer to the pipeline
/// checker; otherwise validate the role in isolation (existence, capability,
/// and that its own declared schemas are valid JSON Schema).
fn check_role(config: &GlobalConfig, role_name: &str, json: bool) -> i32 {
    let role = {
        let cfg = config.read();
        match cfg.retrieve_role(role_name) {
            Ok(r) => r,
            Err(e) => {
                emit_check_error(json, role_name, &format!("failed to load role: {e}"));
                return CHECK_EXIT_INVALID;
            }
        }
    };

    if let Some(nodes) = role.pipeline() {
        let nodes = nodes.to_vec();
        return check_pipeline(config, role_name, &nodes, json);
    }

    // Standalone role: capability + schema-validity checks. Wrap as a
    // single-stage list so `validate_pipeline_stages` covers model/tool fit.
    let cfg = config.read();
    let mut errors: Vec<String> = Vec::new();
    let tuples = vec![(role_name.to_string(), None)];
    if let Err(e) = validate_pipeline_stages(&cfg, &tuples) {
        errors.push(e.to_string());
    }
    for (label, schema) in [
        ("input_schema", role.input_schema()),
        ("output_schema", role.output_schema()),
    ] {
        if let Some(s) = schema {
            if let Err(e) = jsonschema::validator_for(s) {
                errors.push(format!("{label} is not a valid JSON Schema: {e}"));
            }
        }
    }

    let input = role.port_input_summary();
    let output = role.port_output_summary();
    render_role_report(role_name, &input, &output, &errors, json)
}

/// Validate a pipeline DAG: existence + capability of every reachable stage,
/// structural integrity, cycle-freedom, and (for sequential pipelines) the
/// Phase 15B cross-stage schema containment at each boundary.
fn check_pipeline(config: &GlobalConfig, entry: &str, nodes: &[PipelineNode], json: bool) -> i32 {
    let cfg = config.read();
    let mut errors: Vec<String> = Vec::new();

    let stage_tuples = collect_preflight_stages(nodes);
    if let Err(e) = validate_pipeline_stages(&cfg, &stage_tuples) {
        errors.push(e.to_string());
    }
    if let Err(e) = validate_pipeline_dag_structure(nodes) {
        errors.push(e.to_string());
    }
    if let Err(e) = validate_pipeline_dag_cycles(&cfg, entry, nodes) {
        errors.push(e.to_string());
    }

    // Phase 36C: escalation guard, surfaced at `--check` time so config_override
    // misuse is caught offline before any LLM call.
    if let Ok(parent_role) = cfg.retrieve_role(entry) {
        let stages = parent_role.pipeline_all_stages();
        let overrides: Vec<(usize, String, PartialConfig)> = stages
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.config_override.clone().map(|ov| (i, s.role.clone(), ov)))
            .collect();
        if !overrides.is_empty() {
            let parent_cwd = cfg
                .working_directory
                .clone()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            if let Err(e) = crate::config::preflight::validate_pipeline_overrides(
                parent_role.name(),
                parent_role.use_tools().as_deref(),
                &parent_cwd,
                &overrides,
            ) {
                errors.push(e.to_string());
            }
        }
    }

    // Containment is only well-defined for a purely sequential pipeline, and
    // only worth computing once the structural checks above pass (an unknown
    // stage would just produce noise). Fan-out / switch DAGs are noted as
    // unchecked — adjacent-stage shape validation across branches is Phase 33D.
    let sequential = sequential_stage_tuples(nodes);
    let boundaries: Vec<BoundaryReport> = match &sequential {
        Some(seq) if errors.is_empty() => {
            validate_pipeline_schema_containment(&cfg, seq)
        }
        _ => Vec::new(),
    };

    // Per-stage port descriptions for the report.
    let ports: Vec<StagePort> = stage_tuples
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            let (input, output) = stage_ports(&cfg, name);
            StagePort {
                position: i + 1,
                role: name.clone(),
                input,
                output,
            }
        })
        .collect();

    render_pipeline_report(
        entry,
        &ports,
        &boundaries,
        &errors,
        sequential.is_none(),
        json,
    )
}

/// Flatten a pipeline into its sequential leaf-stage tuples, or `None` if any
/// top-level node is a fan-out or switch (i.e. not purely sequential).
fn sequential_stage_tuples(nodes: &[PipelineNode]) -> Option<Vec<(String, Option<String>)>> {
    let mut out = Vec::with_capacity(nodes.len());
    for n in nodes {
        match n {
            PipelineNode::Stage(s) => out.push((s.role.clone(), s.model.clone())),
            _ => return None,
        }
    }
    Some(out)
}

/// Resolve a stage name to its `(input_port, output_port)` summary strings.
fn stage_ports(cfg: &Config, name: &str) -> (String, String) {
    match cfg.classify_entity(name) {
        Ok(EntityRef::Role(n)) => match cfg.retrieve_role(&n) {
            Ok(role) => (role.port_input_summary(), role.port_output_summary()),
            Err(_) => ("?".to_string(), "?".to_string()),
        },
        Ok(EntityRef::Agent(_)) => ("agent".to_string(), "agent".to_string()),
        Ok(EntityRef::Remote { .. }) => ("remote".to_string(), "remote".to_string()),
        _ => ("?".to_string(), "?".to_string()),
    }
}

struct StagePort {
    position: usize,
    role: String,
    input: String,
    output: String,
}

/// Emit a top-level failure (couldn't even load/parse the definition).
fn emit_check_error(json: bool, target: &str, message: &str) {
    if json {
        let payload = serde_json::json!({
            "valid": false,
            "target": target,
            "errors": [message],
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    } else {
        eprintln!("check failed: {message}");
    }
}

fn render_role_report(
    role_name: &str,
    input: &str,
    output: &str,
    errors: &[String],
    json: bool,
) -> i32 {
    let valid = errors.is_empty();
    if json {
        let payload = serde_json::json!({
            "valid": valid,
            "target": role_name,
            "kind": "role",
            "input": input,
            "output": output,
            "errors": errors,
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
    } else {
        println!("Role: {role_name}");
        println!("  input:  {input}");
        println!("  output: {output}");
        for e in errors {
            println!("  ERROR: {e}");
        }
        if valid {
            println!("check passed");
        } else {
            println!("check failed: {} error(s)", errors.len());
        }
    }
    if valid {
        0
    } else {
        CHECK_EXIT_INVALID
    }
}

fn render_pipeline_report(
    entry: &str,
    ports: &[StagePort],
    boundaries: &[BoundaryReport],
    errors: &[String],
    non_sequential: bool,
    json: bool,
) -> i32 {
    let failed_boundaries: Vec<&BoundaryReport> = boundaries
        .iter()
        .filter(|b| b.skipped.is_none() && b.containment.verdict == ContainmentVerdict::Fail)
        .collect();
    let valid = errors.is_empty() && failed_boundaries.is_empty();

    if json {
        let stages_json: Vec<serde_json::Value> = ports
            .iter()
            .map(|p| {
                serde_json::json!({
                    "position": p.position,
                    "role": p.role,
                    "input": p.input,
                    "output": p.output,
                })
            })
            .collect();
        let boundaries_json: Vec<serde_json::Value> = boundaries
            .iter()
            .map(|b| {
                let status = if let Some(reason) = &b.skipped {
                    serde_json::json!({
                        "from": b.from_role,
                        "to": b.to_role,
                        "status": "skipped",
                        "reason": reason,
                    })
                } else {
                    serde_json::json!({
                        "from": b.from_role,
                        "to": b.to_role,
                        "status": verdict_str(b.containment.verdict),
                        "missing": b.containment.missing,
                        "extra": b.containment.extra,
                        "forbidden": b.containment.forbidden,
                        "type_mismatches": b.containment.type_mismatches
                            .iter()
                            .map(|(f, p, c)| serde_json::json!({"field": f, "producer": p, "consumer": c}))
                            .collect::<Vec<_>>(),
                        "notes": b.containment.notes,
                    })
                };
                status
            })
            .collect();
        let payload = serde_json::json!({
            "valid": valid,
            "target": entry,
            "kind": "pipeline",
            "stages": stages_json,
            "boundaries": boundaries_json,
            "non_sequential": non_sequential,
            "errors": errors,
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap_or_default());
        return if valid { 0 } else { CHECK_EXIT_INVALID };
    }

    // Human-readable.
    println!("Pipeline: {entry} ({} stage{})", ports.len(), plural(ports.len()));
    for p in ports {
        println!(
            "  {}. {:<24} in: {:<22} out: {}",
            p.position, p.role, p.input, p.output
        );
    }

    if !errors.is_empty() {
        println!();
        for e in errors {
            println!("ERROR: {e}");
        }
        println!("\ncheck failed: {} error(s)", errors.len());
        return CHECK_EXIT_INVALID;
    }

    if non_sequential {
        println!(
            "\nnote: pipeline is non-sequential (parallel/switch); cross-stage \
             schema containment not checked"
        );
    }

    let mut warn_count = 0usize;
    let mut skip_count = 0usize;
    for b in boundaries {
        match &b.skipped {
            Some(reason) => {
                skip_count += 1;
                println!(
                    "\nSKIP: stage {} ({}) → stage {} ({})\n  {}",
                    b.from_pos, b.from_role, b.to_pos, b.to_role, reason
                );
            }
            None => match b.containment.verdict {
                ContainmentVerdict::Fail => {
                    println!(
                        "\nFAIL: stage {} ({}) → stage {} ({})",
                        b.from_pos, b.from_role, b.to_pos, b.to_role
                    );
                    if !b.containment.missing.is_empty() {
                        println!("  Missing: {}", b.containment.missing.join(", "));
                    }
                    for (field, prod, cons) in &b.containment.type_mismatches {
                        println!(
                            "  Type mismatch on '{field}': upstream {prod} vs downstream {cons}"
                        );
                    }
                    if !b.containment.forbidden.is_empty() {
                        println!(
                            "  Forbidden (downstream additionalProperties: false): {}",
                            b.containment.forbidden.join(", ")
                        );
                    }
                    if !b.containment.extra.is_empty() {
                        println!("  Extra:   {}", b.containment.extra.join(", "));
                    }
                    println!(
                        "  Suggestion: add a transform stage, or align the schemas so the \
                         upstream output satisfies the downstream input."
                    );
                }
                ContainmentVerdict::Warn => {
                    warn_count += 1;
                    println!(
                        "\nWARN: stage {} ({}) → stage {} ({})",
                        b.from_pos, b.from_role, b.to_pos, b.to_role
                    );
                    for n in &b.containment.notes {
                        println!("  {n}");
                    }
                }
                ContainmentVerdict::Unknown => {
                    for n in &b.containment.notes {
                        println!(
                            "\nnote: stage {} ({}) → stage {} ({}): {}",
                            b.from_pos, b.from_role, b.to_pos, b.to_role, n
                        );
                    }
                }
                ContainmentVerdict::Ok => {}
            },
        }
    }

    let checked = boundaries.len() - skip_count;
    if valid {
        println!(
            "\nOK: {checked} boundar{} checked{}",
            if checked == 1 { "y" } else { "ies" },
            if warn_count > 0 {
                format!(", {warn_count} warning(s)")
            } else {
                String::new()
            }
        );
        println!("check passed");
        0
    } else {
        let n = failed_boundaries.len();
        println!(
            "\ncheck failed: {n} incompatible boundar{}",
            if n == 1 { "y" } else { "ies" }
        );
        CHECK_EXIT_INVALID
    }
}

fn verdict_str(v: ContainmentVerdict) -> &'static str {
    match v {
        ContainmentVerdict::Ok => "ok",
        ContainmentVerdict::Fail => "fail",
        ContainmentVerdict::Warn => "warn",
        ContainmentVerdict::Unknown => "unknown",
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
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
        // Phase 36C: reject any stage override that escalates beyond the parent role.
        preflight_overrides(config, &role, &role.pipeline_all_stages())?;

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
                config_override: s.config_override.clone(),
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
            config_override: None,
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
        // Phase 33D: adjacent-stage shape check for the sequential pipeline.
        crate::config::preflight::validate_pipeline_shape(&config.read(), &stage_tuples)?;
        // Phase 36C: reject any stage override that escalates beyond the parent role.
        preflight_overrides(config, &role, &role.pipeline_sequential().unwrap_or_default())?;
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
            node_index: i,
            cached: m.cached,
            config_overrides_applied: stage
                .config_override
                .as_ref()
                .map(PartialConfig::applied_fields)
                .unwrap_or_default(),
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
        // Phase 36C: reject any stage override that escalates beyond the parent role.
        preflight_overrides(config, &role, &role.pipeline_all_stages())?;

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
                node_index: i,
                cached: false,
                config_overrides_applied: Vec::new(),
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
                config_override: s.config_override.clone(),
            })
            .collect()
    } else {
        vec![PipelineStage {
            role_name: role_name.to_string(),
            model_id: None,
            budget_usd: pipeline_budget_usd.filter(|b| *b > 0.0),
            config_override: None,
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
        // Phase 33D: adjacent-stage shape check for the sequential pipeline.
        crate::config::preflight::validate_pipeline_shape(&config.read(), &stage_tuples)?;
        // Phase 36C: reject any stage override that escalates beyond the parent role.
        preflight_overrides(config, &role, &role.pipeline_sequential().unwrap_or_default())?;
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
            node_index: i,
            cached: m.cached,
            config_overrides_applied: stage
                .config_override
                .as_ref()
                .map(PartialConfig::applied_fields)
                .unwrap_or_default(),
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
            // Server inline stages (`/v1/pipelines/run`) carry no override.
            config_override: None,
        })
        .collect();
    {
        let stage_tuples: Vec<(String, Option<String>)> = pipeline_stages
            .iter()
            .map(|s| (s.role_name.clone(), s.model_id.clone()))
            .collect();
        crate::config::preflight::validate_pipeline_stages(&config.read(), &stage_tuples)?;
        // Phase 33D: adjacent-stage shape check for the sequential pipeline.
        crate::config::preflight::validate_pipeline_shape(&config.read(), &stage_tuples)?;
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
            node_index: i,
            cached: m.cached,
            // Inline server stages carry no override.
            config_overrides_applied: Vec::new(),
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
    // Phase 33D: adjacent-stage shape check (sequential pipelines only).
    preflight_shape(config, nodes)?;

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
                    config_override: s.config_override.clone(),
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
                    node_index,
                    cached: metrics.cached,
                    config_overrides_applied: s
                        .config_override
                        .as_ref()
                        .map(PartialConfig::applied_fields)
                        .unwrap_or_default(),
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
                    stage_budget_usd,
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
                    stage_budget_usd,
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
    node_budget_usd: Option<f64>,
    abort_signal: AbortSignal,
) -> Result<(String, Vec<StageTrace>)> {
    // Each branch sees the same input. We don't propagate `is_last=true`
    // into branches: a branch's output is consumed by the merge, never
    // printed directly. The merged output is what propagates downstream.
    let branch_count = p.branches.len();
    // Phase 22C: split the node's pre-allocated dollar budget evenly across
    // branches. `None` (no enforcement) propagates as `None`. The merge stage
    // runs unbudgeted — the budget is spent on the fan-out itself.
    let branch_budget = split_branch_budget(node_budget_usd, branch_count);
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
            branch_budget,
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
                // Fan-out merge stage runs un-isolated (no per-stage override).
                config_override: None,
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
                node_index,
                cached: metrics.cached,
                config_overrides_applied: Vec::new(),
            });
            return Ok((out, traces));
        }
    };

    // For built-in merges (concatenate / json_array), the parallel node
    // itself doesn't run an extra stage. If this node is the last in the
    // top-level pipeline and we're printing, emit the merged output here.
    // Suppressed under `-o json`, where `run` emits the trace envelope instead.
    if is_last && !json_envelope_mode(config) {
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
    node_budget_usd: Option<f64>,
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
        // Phase 22C: only one switch arm runs, so it inherits the node's full
        // pre-allocated budget rather than a split share.
        node_budget_usd,
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

// ---------------------------------------------------------------------------
// Phase 22: DAG observability & budget
// ---------------------------------------------------------------------------

/// True when the CLI `--pipe` run will wrap its result in a JSON trace envelope
/// (`-o json`, emitted by [`run`]). In that mode individual stages must stay
/// silent — otherwise the last stage prints its own output ahead of the
/// envelope and stdout is no longer a single valid JSON document. Reads the
/// dedicated `pipeline_emits_envelope` flag (set by [`run`]) rather than
/// `output_format`, which is intentionally `None` for stages so the JSON
/// system-prompt suffix never leaks into their prompts.
fn json_envelope_mode(config: &GlobalConfig) -> bool {
    config.read().pipeline_emits_envelope
}

/// Phase 22C: split a parallel node's pre-allocated dollar budget equally
/// across its branches. `None` (no enforcement) passes straight through, as
/// does a zero branch count — there is nothing to divide a budget over.
fn split_branch_budget(node_budget_usd: Option<f64>, branch_count: usize) -> Option<f64> {
    match (node_budget_usd, branch_count) {
        (Some(b), n) if n > 0 => Some(b / n as f64),
        _ => None,
    }
}

/// Phase 22A: derive `(wall_ms, sequential_ms)` from a flat trace list.
///
/// `sequential_ms` is just the sum of every stage's latency — what the run
/// would have cost end-to-end with no concurrency. `wall_ms` models the DAG:
/// top-level nodes run in series, so it sums each node's wall time, and a
/// node's wall time is the slowest branch (fan-out runs concurrently) plus
/// any non-branch latency in that node (a sequential stage, a switch arm, or
/// a custom-merge stage that runs after the branches join).
fn pipeline_timing(traces: &[StageTrace]) -> (u64, u64) {
    use std::collections::BTreeMap;
    let sequential_ms: u64 = traces.iter().map(|t| t.latency_ms).sum();

    // node_index -> (branch -> summed latency, non-branch summed latency)
    let mut nodes: BTreeMap<usize, (BTreeMap<usize, u64>, u64)> = BTreeMap::new();
    for tr in traces {
        let entry = nodes.entry(tr.node_index).or_default();
        match tr.branch {
            Some(b) => *entry.0.entry(b).or_default() += tr.latency_ms,
            None => entry.1 += tr.latency_ms,
        }
    }

    let wall_ms = nodes
        .values()
        .map(|(branches, non_branch)| {
            let slowest_branch = branches.values().copied().max().unwrap_or(0);
            slowest_branch + non_branch
        })
        .sum();

    (wall_ms, sequential_ms)
}

/// Format a latency for the trace tree: sub-second as `Nms`, else `N.Ns`.
fn fmt_latency(ms: u64) -> String {
    if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}

/// Format a duration as fractional seconds (used for the wall/sequential total).
fn fmt_seconds(ms: u64) -> String {
    format!("{:.1}s", ms as f64 / 1000.0)
}

/// One stage's line in the trace tree: `role  model  in→out tok  $cost  lat`.
/// A cache hit (Phase 22D) appends `(cached)`.
fn stage_line(t: &StageTrace) -> String {
    let mut s = format!(
        "{}  {}  {}→{}tok  ${:.4}  {}",
        t.role,
        t.model,
        t.input_tokens,
        t.output_tokens,
        t.cost_usd,
        fmt_latency(t.latency_ms),
    );
    if t.cached {
        s.push_str("  (cached)");
    }
    s
}

/// Phase 22A/22B: render a pipeline DAG's execution trace as an indented tree.
/// Walks `nodes` for structure (branch labels, merge strategy, switch arms)
/// and indexes `traces` by `(node_index, branch)` for the per-stage metrics.
/// Per-branch cost (22B) shows on each branch line, with a subtotal when a
/// branch ran more than one stage; the footer reports total cost plus
/// wall-clock vs sequential time.
fn render_trace_tree(label: &str, nodes: &[PipelineNode], traces: &[StageTrace]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();

    let stage_count: usize = nodes
        .iter()
        .map(|n| n.all_stages().len() + n.merge_role_names().len())
        .sum();
    let parallel_count = nodes
        .iter()
        .filter(|n| matches!(n, PipelineNode::Parallel(_)))
        .count();

    let _ = write!(out, "[pipeline] {label} ({stage_count} stage{}", plural(stage_count));
    if parallel_count > 0 {
        let _ = write!(out, ", {parallel_count} parallel");
    }
    out.push_str(")\n");

    let node_traces = |ni: usize| -> Vec<&StageTrace> {
        traces.iter().filter(|t| t.node_index == ni).collect()
    };

    for (i, node) in nodes.iter().enumerate() {
        let n1 = i + 1;
        match node {
            PipelineNode::Stage(_) => {
                if let Some(t) = node_traces(i).into_iter().find(|t| t.branch.is_none()) {
                    let _ = writeln!(out, "  [{n1}] {}", stage_line(t));
                }
            }
            PipelineNode::Parallel(p) => {
                let bcount = p.branches.len();
                let _ = writeln!(
                    out,
                    "  [{n1}] parallel ({bcount} branch{})",
                    if bcount == 1 { "" } else { "es" }
                );
                let group = node_traces(i);
                for bi in 0..bcount {
                    let bno = bi + 1;
                    let letter = (b'a' + bi as u8) as char;
                    let branch_traces: Vec<&StageTrace> =
                        group.iter().copied().filter(|t| t.branch == Some(bno)).collect();
                    for t in &branch_traces {
                        let _ = writeln!(out, "    [{n1}{letter}] {}", stage_line(t));
                    }
                    // Phase 22B: branch subtotal when a branch ran >1 stage.
                    if branch_traces.len() > 1 {
                        let cost: f64 = branch_traces.iter().map(|t| t.cost_usd).sum();
                        let lat: u64 = branch_traces.iter().map(|t| t.latency_ms).sum();
                        let _ = writeln!(
                            out,
                            "      branch {letter}: ${cost:.4}  {}",
                            fmt_latency(lat)
                        );
                    }
                }
                match &p.merge {
                    MergeStrategy::Concatenate => {
                        let _ = writeln!(out, "    merge: concatenate");
                    }
                    MergeStrategy::JsonArray => {
                        let _ = writeln!(out, "    merge: json_array");
                    }
                    MergeStrategy::CustomRole(r) => {
                        match group.iter().copied().find(|t| t.branch.is_none()) {
                            Some(t) => {
                                let _ = writeln!(out, "    merge: custom_role: {r}  {}", stage_line(t));
                            }
                            None => {
                                let _ = writeln!(out, "    merge: custom_role: {r}");
                            }
                        }
                    }
                }
            }
            PipelineNode::Switch(_) => {
                let group = node_traces(i);
                let executed: Vec<&StageTrace> =
                    group.iter().copied().filter(|t| t.branch.is_none()).collect();
                match executed.first() {
                    Some(first) => {
                        let _ = writeln!(out, "  [{n1}] switch → {}", first.role);
                        for t in &executed {
                            let _ = writeln!(out, "    [{n1}] {}", stage_line(t));
                        }
                    }
                    None => {
                        let _ = writeln!(out, "  [{n1}] switch (no branch taken)");
                    }
                }
            }
        }
    }

    let (wall, seq) = pipeline_timing(traces);
    let total_cost: f64 = traces.iter().map(|t| t.cost_usd).sum();
    let _ = writeln!(
        out,
        "  total: ${total_cost:.4}  {} (wall) vs {} (sequential)",
        fmt_seconds(wall),
        fmt_seconds(seq)
    );
    out
}

/// Phase 23C: the branch-aware label for a stage, matching the trace tree.
/// Fan-out stages carry a numeric branch index; sequential stages do not.
pub fn stage_label(trace: &StageTrace) -> String {
    match trace.branch {
        Some(b) => format!("branch{}: {}", b, trace.role),
        None => trace.role.clone(),
    }
}

/// Phase 23C: build one per-stage run-log record for cost attribution by role.
/// `run_id` is shared across every stage of a single pipeline run; `label` is
/// the pipeline-def name (or "pipeline"); `stage_idx` is 1-based.
pub fn stage_run_log_record(
    run_id: &str,
    label: &str,
    stage_idx: usize,
    trace: &StageTrace,
) -> serde_json::Value {
    serde_json::json!({
        "ts": crate::utils::now(),
        "run_id": run_id,
        "pipeline": label,
        "stage": stage_idx,
        "stage_role": stage_label(trace),
        "model": trace.model,
        "input_tokens": trace.input_tokens,
        "output_tokens": trace.output_tokens,
        "cost_usd": trace.cost_usd,
        "latency_ms": trace.latency_ms,
        "cached": trace.cached,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MergeStrategy, ParallelNode, PipelineNode, RolePipelineStage};

    fn stage(role: &str, model: Option<&str>) -> PipelineNode {
        PipelineNode::Stage(RolePipelineStage {
            role: role.to_string(),
            model: model.map(|m| m.to_string()),
            budget_weight: None,
            config_override: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn tr(
        role: &str,
        model: &str,
        input: u64,
        output: u64,
        cost: f64,
        lat: u64,
        node_index: usize,
        branch: Option<usize>,
    ) -> StageTrace {
        StageTrace {
            role: role.to_string(),
            model: model.to_string(),
            input_tokens: input,
            output_tokens: output,
            cost_usd: cost,
            latency_ms: lat,
            branch,
            node_index,
            cached: false,
            config_overrides_applied: Vec::new(),
        }
    }

    // ---- Phase 36: config isolation ----

    #[test]
    fn clone_and_merge_produces_independent_config() {
        let mut parent = Config::default();
        parent.temperature = Some(0.7);
        let p = PartialConfig {
            working_directory: Some(std::path::PathBuf::from("/tmp/stage")),
            ..Default::default()
        };
        let mut clone = parent.clone();
        clone.apply_partial(&p).unwrap();
        // The clone carries the override...
        assert_eq!(
            clone.working_directory,
            Some(std::path::PathBuf::from("/tmp/stage"))
        );
        // ...and the parent is untouched.
        assert_eq!(parent.working_directory, None);
        assert_eq!(parent.temperature, Some(0.7));
    }

    #[test]
    fn cache_key_changes_with_sampling_override() {
        // Mirrors the fold in `run_stage_inner`: the override fingerprint is
        // woven into the role component of the key, so two sampling values
        // produce distinct cache keys for the same role/model/input.
        let role = "summarize";
        let model = "test-model";
        let input = "hello";
        let key_plain = StageCache::key(role, model, input);
        let p0 = PartialConfig {
            temperature: Some(0.0),
            ..Default::default()
        };
        let p1 = PartialConfig {
            temperature: Some(1.0),
            ..Default::default()
        };
        let key0 = StageCache::key(
            &format!("{role}\u{1e}{}", p0.cache_fingerprint()),
            model,
            input,
        );
        let key1 = StageCache::key(
            &format!("{role}\u{1e}{}", p1.cache_fingerprint()),
            model,
            input,
        );
        assert_ne!(key0, key1);
        assert_ne!(key0, key_plain);
    }

    #[test]
    fn stage_with_override_threads_into_trace_fields() {
        // `applied_fields` is the exact source the StageTrace population uses.
        let p = PartialConfig {
            use_tools: Some("fs_read".into()),
            working_directory: Some(std::path::PathBuf::from("/x")),
            ..Default::default()
        };
        assert_eq!(p.applied_fields(), vec!["use_tools", "working_directory"]);
    }

    // ---- Phase 22C: split_branch_budget ----

    #[test]
    fn split_branch_budget_none_passes_through() {
        assert_eq!(split_branch_budget(None, 3), None);
    }

    #[test]
    fn split_branch_budget_divides_equally() {
        let share = split_branch_budget(Some(0.30), 3).expect("some");
        assert!((share - 0.10).abs() < 1e-9, "got {share}");
    }

    #[test]
    fn split_branch_budget_zero_branches_is_none() {
        // Never divide by zero — a parallel node with no branches is degenerate
        // but must not panic.
        assert_eq!(split_branch_budget(Some(0.30), 0), None);
    }

    // ---- Phase 22A: pipeline_timing ----

    #[test]
    fn pipeline_timing_sequential_sums_all_latency() {
        let traces = vec![
            tr("a", "m", 1, 1, 0.0, 800, 0, None),
            tr("b", "m", 1, 1, 0.0, 1500, 1, None),
        ];
        let (wall, seq) = pipeline_timing(&traces);
        assert_eq!(seq, 2300);
        assert_eq!(wall, 2300, "two sequential stages: wall == sequential");
    }

    #[test]
    fn pipeline_timing_parallel_wall_is_slowest_branch() {
        // extract (800) -> parallel{1200, 600, 700} -> synthesize (1500)
        let traces = vec![
            tr("extract", "deepseek", 500, 200, 0.0001, 800, 0, None),
            tr("security-review", "claude", 200, 300, 0.004, 1200, 1, Some(1)),
            tr("style-review", "deepseek", 200, 150, 0.0001, 600, 1, Some(2)),
            tr("perf-review", "deepseek", 200, 180, 0.0001, 700, 1, Some(3)),
            tr("synthesize", "claude", 630, 200, 0.006, 1500, 2, None),
        ];
        let (wall, seq) = pipeline_timing(&traces);
        assert_eq!(seq, 4800, "800+1200+600+700+1500");
        assert_eq!(wall, 3500, "800 + max(1200,600,700) + 1500");
    }

    #[test]
    fn pipeline_timing_custom_merge_runs_after_branches() {
        let traces = vec![
            tr("a", "m", 10, 10, 0.001, 100, 0, Some(1)),
            tr("b", "m", 10, 10, 0.001, 100, 0, Some(2)),
            tr("merger", "m", 20, 5, 0.002, 300, 0, None),
        ];
        let (wall, seq) = pipeline_timing(&traces);
        assert_eq!(seq, 500);
        assert_eq!(wall, 400, "max(100,100) branches + 300 merge");
    }

    // ---- Phase 22A/22B: render_trace_tree ----

    #[test]
    fn render_trace_tree_shows_nodes_branches_merge_and_totals() {
        let nodes = vec![
            stage("extract", Some("deepseek")),
            PipelineNode::Parallel(ParallelNode {
                branches: vec![stage("security-review", None), stage("style-review", None)],
                merge: MergeStrategy::Concatenate,
            }),
            stage("synthesize", None),
        ];
        let traces = vec![
            tr("extract", "deepseek", 500, 200, 0.0001, 800, 0, None),
            tr("security-review", "claude", 200, 300, 0.004, 1200, 1, Some(1)),
            tr("style-review", "deepseek", 200, 150, 0.0001, 600, 1, Some(2)),
            tr("synthesize", "claude", 630, 200, 0.006, 1500, 2, None),
        ];
        let tree = render_trace_tree("secure-review", &nodes, &traces);

        assert!(tree.contains("[pipeline] secure-review"), "{tree}");
        assert!(tree.contains("[1] extract"), "{tree}");
        assert!(tree.contains("[2] parallel (2 branches)"), "{tree}");
        assert!(tree.contains("[2a] security-review"), "{tree}");
        assert!(tree.contains("[2b] style-review"), "{tree}");
        assert!(tree.contains("merge: concatenate"), "{tree}");
        assert!(tree.contains("[3] synthesize"), "{tree}");
        assert!(tree.contains("500→200tok"), "{tree}");
        assert!(tree.contains("$0.0040"), "{tree}");
        // total cost 0.0102; wall 800+1200+1500=3500; seq 800+1200+600+1500=4100
        assert!(tree.contains("total: $0.0102"), "{tree}");
        assert!(tree.contains("3.5s (wall)"), "{tree}");
        assert!(tree.contains("4.1s (sequential)"), "{tree}");
    }

    #[test]
    fn render_trace_tree_marks_cached_stage() {
        let nodes = vec![stage("extract", None)];
        let mut t = tr("extract", "deepseek", 500, 0, 0.0, 0, 0, None);
        t.cached = true;
        let tree = render_trace_tree("p", &nodes, &[t]);
        assert!(tree.contains("(cached)"), "{tree}");
    }

    #[test]
    fn render_trace_tree_custom_merge_shows_merge_role() {
        let nodes = vec![PipelineNode::Parallel(ParallelNode {
            branches: vec![stage("a", None), stage("b", None)],
            merge: MergeStrategy::CustomRole("merger".to_string()),
        })];
        let traces = vec![
            tr("a", "m", 10, 10, 0.001, 100, 0, Some(1)),
            tr("b", "m", 10, 10, 0.001, 100, 0, Some(2)),
            tr("merger", "m", 20, 5, 0.002, 300, 0, None),
        ];
        let tree = render_trace_tree("p", &nodes, &traces);
        assert!(tree.contains("merge: custom_role: merger"), "{tree}");
        assert!(tree.contains("[1a] a"), "{tree}");
        assert!(tree.contains("[1b] b"), "{tree}");
    }
}
