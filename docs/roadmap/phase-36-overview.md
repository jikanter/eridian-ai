# Phase 36: Pipeline Stage Config Isolation : Overview - Epic 7

**Status (2026-06-01):** **Done (36A–D).** Shipped. This phase closes the incidental divergence identified in Theme 9 of [`260524_anthropic_memory_divergence.md`](https://github.com/jikanter/aichat-private/): pipeline stages share the global `Config` (tools, env, working directory) where Anthropic's sub-agent pattern isolates each. Extends the model-state-restore pattern already in [`src/pipe.rs`](../../src/pipe.rs) (`run_stage` → `run_stage_inner`) to also clone-and-restore *config* state across stages, mirroring how [`src/config/resolver.rs`](../../src/config/resolver.rs) describes macros running in isolated config clones. User docs: [`docs/features/pipeline-isolation.md`](../features/pipeline-isolation.md).

**Implementation note (delta from design draft):** override fields split by scope rather than all landing on the cloned `Config`. The pipeline path reads tool selection and sampling from the *stage's resolved role*, not the global config, so `use_tools`/`temperature`/`top_p`/`max_output_tokens` are applied via `PartialConfig::apply_to_role(&mut Role)` in `run_stage_inner`; only `working_directory`/`mcp_servers` go through `Config::apply_partial` on the cloned config in `run_stage`. `mcp_servers` is disable-only (`[]`) this release — non-empty re-selection is rejected at preflight. The escalation guard runs at execution preflight *and* under `--check` (the offline-deterministic surface; `--dry-run` short-circuits before the execution preflight, so the guard was added to `check_pipeline` too).

| Item | Description | Status |
|---|---|---|
| 36A | Add `config_override: Option<PartialConfig>` field to `PipelineStage` ([`src/pipe.rs`](../../src/pipe.rs)) and to the YAML stage declaration in `RolePipelineStage` ([`src/config/role.rs`](../../src/config/role.rs)); `PartialConfig` + `working_directory` on `Config` ([`src/config/mod.rs`](../../src/config/mod.rs)). | **Done** |
| 36B | Clone-and-merge at the stage boundary — config-scoped fields (`working_directory`, `mcp_servers`) on the cloned `Config` via `Config::apply_partial`; role-scoped fields (`use_tools`, sampling) on the stage's resolved role via `PartialConfig::apply_to_role`; per-command `cmd.current_dir` at spawn; cache-key fold. | **Done** |
| 36C | Preflight escalation guard — `use_tools` subset, `mcp_servers` disable-only, `working_directory` descendant. Runs at execution preflight and under `--check`. Extends [`src/config/preflight.rs`](../../src/config/preflight.rs). | **Done** |
| 36D | Telemetry — `config_overrides_applied: Vec<String>` on [`StageTrace`](../../src/pipe.rs), surfaced in `-o json` per stage. | **Done** |

## Background

The convergence-doc claim that aichat's Pipeline-as-Role is "isolating each stage's config" overstates what `src/pipe.rs` does today. A closer pass on the actual code (per divergence Theme 9, `[ts-ai-00014]`):

- `PipelineStage` ([`src/pipe.rs:18`](../../src/pipe.rs)) carries `role_name` and `model_id` per stage.
- `run_stage` ([`src/pipe.rs:164`](../../src/pipe.rs)) restores `model` after each stage runs (the existing model-state restore around `run_stage_inner` at line 210) — so the *LLM context* is isolated stage-to-stage.
- The global `Config` is *not* cloned between stages. Tool permissions, environment variables, working directory, and MCP-pool state all share one `GlobalConfig` instance across the whole pipeline.

[`src/config/resolver.rs:170-185`](../../src/config/resolver.rs) explicitly documents the contrast: "Macros run REPL commands in an isolated config clone — they have no role shape and cannot participate in stage chaining." The macro pattern is the right model; pipeline stages need to adopt it.

This matters because Anthropic's sub-agent isolation is *both* — separate context window *and* separate config, including separate tool permissions. A research sub-agent and an implementation sub-agent should not be able to read each other's MCP credentials or modify each other's working directory. aichat's pipeline cannot enforce this today.

The fix is narrow: extend the existing model-state restore pattern to also clone-and-restore the `Config` itself, gated by an opt-in `config_override:` field per stage. This is closing an incidental divergence, not opening a new feature.

## Design tenets

1. **Extend the existing pattern, don't add a new one.** The model-state restore around `run_stage_inner` (lines 188-222) is the existing isolation primitive. 36B layers config clone-and-restore onto it; no new lifecycle hook.
2. **Opt-in, not implicit.** Stages without a `config_override:` field behave exactly as today (shared `Config`). Backward compatibility is total; this is purely additive.
3. **Permissions are downward-only.** A stage can *narrow* tool permissions (the research stage drops the `write_file` tool); a stage cannot *grant* permissions the parent role lacks. 36C enforces this at preflight.
4. **Telemetry is part of the audit story.** Per `CLAUDE.md` ("This system is designed to run as optimally on local models as frontier models") and the divergence playbook's Theme 6 (telemetry asymmetry), `-o json` consumers see which overrides fired per stage. Silent isolation is half a feature.
5. **No `PartialConfig` proliferation.** `PartialConfig` is one type, defined once. Other phases (e.g. a future macro-as-pipeline-stage feature) reuse it without re-inventing.

## 36A Design — `config_override:` YAML surface

Authoring shape — extends the existing `RolePipelineStage` declaration in role frontmatter:

```yaml
pipeline:
  - role: research-agent
    model: claude-haiku-4-5
    config_override:
      use_tools:
        # Narrow to read-only tools for the research stage
        - read_file
        - grep
        - list_files
      working_directory: ./research-scratch
      mcp_servers: []                       # disable all MCP for this stage

  - role: implementation-agent
    model: claude-sonnet-4-6
    config_override:
      use_tools:
        - read_file
        - write_file
        - run_command
      # Inherits parent role's working_directory and MCP servers
```

`PartialConfig` covers the fields a stage may legitimately override. Initial set (mirrors the smallest sufficient subset; expanded only when use cases justify):

```rust
// Sketch — actual definition lives in src/config/mod.rs
pub struct PartialConfig {
    pub use_tools: Option<Vec<String>>,
    pub working_directory: Option<PathBuf>,
    pub mcp_servers: Option<Vec<String>>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_output_tokens: Option<usize>,
    // Deliberately NOT covered (would require deeper refactor):
    // - clients (model providers are stage-level via `model_id`)
    // - rag (RAG state is conversation-scoped, not stage-scoped)
    // - sessions (sessions are pipeline-scoped, not stage-scoped)
}
```

**Files:** [`src/pipe.rs`](../../src/pipe.rs) (add `config_override` to `PipelineStage` and `PipelineStageDef`), [`src/config/role.rs`](../../src/config/role.rs) (parse `config_override:` from YAML), [`src/config/mod.rs`](../../src/config/mod.rs) (define `PartialConfig`).

## 36B Design — Clone-and-merge at stage boundary

The existing `run_stage` at [`src/pipe.rs:164`](../../src/pipe.rs) already restores the `model` field after each `run_stage_inner` call (the restore happens between lines 210 and 222 — `run_stage_inner` is called with a model override; the parent loop restores the original model afterward). 36B extends this with a config clone:

```rust
// Sketch — actual change lives in src/pipe.rs::run_stage
async fn run_stage(
    config: &GlobalConfig,
    stage: &PipelineStage,
    // ... existing args ...
) -> Result<...> {
    // Existing: model-state restore
    let original_model = config.read().model.clone();

    // NEW (36B): config clone-and-merge if override declared
    let stage_config: GlobalConfig = if let Some(override_cfg) = &stage.config_override {
        let mut cloned = config.read().clone();           // mirrors macro pattern
        cloned.apply_partial(override_cfg)?;              // PartialConfig → Config merge
        Arc::new(RwLock::new(cloned))
    } else {
        config.clone()                                     // no override — pass-through, shared
    };

    let result = run_stage_inner(&stage_config, /* ... */).await;

    // Existing: model restore (now a no-op when stage_config is a clone, since
    // the clone is dropped; only matters when stage_config == config)
    if !stage.has_override() {
        config.write().model = original_model;
    }

    result
}
```

The model-state restore at line 220-222 stays in place for the no-override path (backward compatibility for stages that don't declare `config_override:`). For overridden stages, the entire `stage_config` is dropped after `run_stage_inner` returns — there's nothing to restore because the parent `config` was never mutated.

**Files:** [`src/pipe.rs`](../../src/pipe.rs) (`run_stage`), [`src/config/mod.rs`](../../src/config/mod.rs) (new `Config::apply_partial(&self, PartialConfig)` method).

## 36C Design — Permission escalation guard

A stage that declares `use_tools: [run_command, write_file]` when the parent role only grants `[read_file, grep]` is escalating — that's exactly the cross-stage privilege contamination Theme 9 calls out. Preflight rejects:

```
$ aichat -r my-pipeline-role
error: pipeline stage 2 ('research-agent') declares config_override with
       use_tools=[run_command], but parent role 'my-pipeline-role' does not
       grant 'run_command'. Override may only narrow tool permissions,
       never escalate them.

       hint: add 'run_command' to the parent role's use_tools, or remove
             it from the stage override.
```

The check runs in [`src/config/preflight.rs`](../../src/config/preflight.rs) alongside the existing pipeline-stage validation. It applies to:

| Override field | Escalation rule |
|---|---|
| `use_tools` | Stage list must be a subset of parent role's `use_tools` |
| `mcp_servers` | Stage list must be a subset of parent role's `mcp_servers` |
| `working_directory` | Must not contain `..` segments that escape the parent's working directory tree |
| `temperature` / `top_p` / `max_output_tokens` | No escalation check — these are tuning knobs, not permission grants |

The `working_directory` rule is the trickiest. The implementation canonicalises both paths (parent and override) and asserts the override is a prefix or descendant. Symlink resolution is deliberately *out of scope* for this phase — a malicious role author could symlink their way around the check, but that's the existing trust model (roles are trusted code; we are protecting against accidents, not adversaries).

**Files:** [`src/config/preflight.rs`](../../src/config/preflight.rs) (extend `validate_pipeline_stages`), [`src/config/mod.rs`](../../src/config/mod.rs) (helper for `is_path_descendant`).

## 36D Design — Telemetry in `StageTrace`

The existing `StageTrace` at [`src/pipe.rs:30`](../../src/pipe.rs) carries per-stage execution metadata. 36D adds:

```rust
pub struct StageTrace {
    // ... existing fields ...
    pub config_overrides_applied: Vec<String>,  // NEW: e.g. ["use_tools", "working_directory"]
}
```

The field is populated by `run_stage` after the clone-and-merge in 36B, listing the names of `PartialConfig` fields that were `Some(_)`. `-o json` output includes the field; `-o text` (the human-readable default) omits it (matches the existing `StageTrace` rendering convention — JSON gets the detail, text gets the summary).

This aligns with Theme 6 (`[dom-ai-00022]`) of the divergence playbook: "audit-by-default not yet enforced at session.compress / strip_thinking" — and now, "not yet enforced at pipeline stage config overrides." Closing this telemetry gap brings pipeline isolation in line with aichat's stated posture.

**Files:** [`src/pipe.rs`](../../src/pipe.rs) (extend `StageTrace`, populate in `run_stage`), [`src/serve.rs`](../../src/serve.rs) (no changes — `StageTrace` serialization is already structural).

## Open questions

### 1. Inheritance for stages without `config_override:`

**Question:** A stage without `config_override:` shares the global `Config` today. Should it instead get an *empty* clone (so mutations from prior stages don't leak forward)?

**Recommendation: keep shared `Config` for no-override stages.** Three reasons. (a) Backward compatibility — flipping to clone-by-default changes behaviour for every existing pipeline. (b) Performance — every stage paying the clone cost when most stages don't need isolation is a tax on the common path. (c) Mental model — opt-in isolation matches Unix `chroot` semantics: by default you inherit; by declaration you isolate. A future opt-out (`isolation: shared|cloned` at the pipeline level) could flip the default for pipelines that want belt-and-suspenders, but the default stays as-is.

### 2. Composability with macro isolation

**Question:** A macro stage already runs in an isolated config clone ([`src/config/resolver.rs:170-185`](../../src/config/resolver.rs)). Does this phase change macro behaviour?

**Recommendation: no. Macro isolation is orthogonal.** Macros are not LLM stages — they are REPL-command sequences. The `pipeline_stage_admissible` check at [`src/config/resolver.rs:178-185`](../../src/config/resolver.rs) explicitly rejects macros from pipelines. This phase touches only role-and-agent stages, which were *not* isolated before; macros were already isolated and stay so.

### 3. `PartialConfig` field set

**Question:** Initial `PartialConfig` covers 6 fields. Should more be added (e.g. `keep_thinking`, `clear_at_least`)?

**Recommendation: ship the initial 6; add more under demand-driven pressure.** Premature surface expansion is the worst kind of API debt. The 6 fields cover the cases Theme 9 explicitly calls out (tool permissions, working directory, MCP servers, sampling params). Future fields (per-stage `keep_thinking` etc.) wait for a documented use case. The `PartialConfig` struct is `#[non_exhaustive]` to make additions non-breaking.

### 4. Deferred — `dependencies.md` / `success-metrics.md` updates

This phase does **not** update [`docs/roadmap/dependencies.md`](dependencies.md) or [`docs/roadmap/success-metrics.md`](success-metrics.md). Tracked as a follow-up doc PR with Phases 34 and 35.

## Testing

Per project guideline ("*Always* add integration tests via bats in addition to unit tests"), the implementation PR must add:

- **`tests/regression/pipeline-isolation.sh`** — bats regression test covering:
  - 36A: a role with valid `config_override:` parses without warning; `aichat --info -r <role> -o json` shows the stage's overrides in the pipeline section.
  - 36B: a 2-stage pipeline where stage 1 mutates a config field (e.g. via a tool that changes working dir); stage 2 sees the *original* parent config, not stage 1's mutation. (Test via an instrumented role that echoes the current working directory.)
  - 36C/escalation-rejection: a stage declaring `use_tools: [run_command]` when the parent role's `use_tools: [read_file]` fails preflight with the documented error message and a non-zero exit code (per [`src/utils/exit_code.rs`](../../src/utils/exit_code.rs)).
  - 36C/narrowing-allowed: a stage declaring `use_tools: [read_file]` when the parent role's `use_tools: [read_file, write_file]` passes preflight.
  - 36C/working-dir-escape: a stage declaring `working_directory: ../../etc` fails preflight.
  - 36D: `aichat -r <pipeline-role> -o json` emits a `stages[].config_overrides_applied` field in the JSON output for each stage that declared overrides, and the field is empty (or absent) for stages without overrides.
  - Backward compat: a 2-stage pipeline with no `config_override:` declarations runs identically to before this phase landed (same `StageTrace` output minus the new field).

- **Rust unit tests** in `src/pipe.rs`:
  - `tests::clone_and_merge_produces_independent_config` — clone two configs from the same parent, mutate each, assert the parent and the other clone are unchanged.
  - `tests::run_stage_with_override_drops_clone_on_completion` — assert `Arc::strong_count` returns to 1 for the parent after the stage exits.
  - `tests::stage_trace_records_applied_overrides` — populate a stage with overrides, assert the trace lists exactly the overridden field names.

- **Rust unit tests** in `src/config/preflight.rs`:
  - `tests::escalation_use_tools_rejected`
  - `tests::escalation_mcp_servers_rejected`
  - `tests::narrowing_use_tools_accepted`
  - `tests::working_directory_must_be_descendant`

## Sequencing

- **36A and 36B must land together** (one PR). 36A without 36B is a YAML field that's silently ignored; 36B without 36A has no consumer.
- **36C should land in a follow-up PR** after 36A+B are exercised end-to-end without the guard. This validates the override mechanism before the rejection logic is layered on (avoids the failure mode where the test for 36B fails because 36C falsely rejects).
- **36D can land in the same PR as 36C** — the telemetry field is independent of the guard, and shipping them together keeps the audit story consistent.

## Files (consolidated)

- [`src/pipe.rs`](../../src/pipe.rs) — `PipelineStage` field, `StageTrace` field, `run_stage` clone-and-merge logic
- [`src/config/role.rs`](../../src/config/role.rs) — YAML parsing for `config_override:`
- [`src/config/mod.rs`](../../src/config/mod.rs) — `PartialConfig` type, `Config::apply_partial` method, `is_path_descendant` helper
- [`src/config/preflight.rs`](../../src/config/preflight.rs) — escalation guard in `validate_pipeline_stages`

## References

- Theme 9 (`[ts-ai-00014]`) of the divergence playbook (analysis source for this phase)
- [`src/pipe.rs:18-222`](../../src/pipe.rs) — current `PipelineStage` definition and `run_stage` model-restore pattern
- [`src/config/resolver.rs:170-185`](../../src/config/resolver.rs) — macro-isolation pattern this phase adopts for pipeline stages
- [Phase 21 overview](phase-21-overview.md) — DAG primitives this phase extends within Epic 7
- [Phase 22 overview](phase-22-overview.md) — sibling phase under Epic 7 (DAG observability); shares the `StageTrace` substrate touched in 36D
