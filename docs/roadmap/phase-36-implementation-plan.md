# Phase 36: Pipeline Stage Config Isolation — Implementation Plan

**Author's note (2026-05-30):** Implementation plan derived from
[`phase-36-overview.md`](phase-36-overview.md), with codebase reality corrections
(§0) and the three scope decisions taken on 2026-05-30 (§0.0). Read §0 first.

---

## §0.0 Scope decisions (resolved 2026-05-30)

| Decision                          | Choice                                                                                                                                                                                                                                                                                                                                                                                              |
|-----------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **`working_directory`**           | **Implement as a new feature this phase.** Add `working_directory: Option<PathBuf>` to `Config`, apply it at the tool-spawn point in `src/function.rs::run_command_with_stderr_timeout` via `Command::current_dir` (per-command, **not** the process-global `std::env::set_current_dir` — that would race across concurrent fan-out branches). Include the path-descendant escalation guard in 36C. |
| **`config_override.mcp_servers`** | **Disable-only this phase.** Support only the empty list (`mcp_servers: []` → clear `Config.mcp_servers`). Reject a non-empty list at preflight with "not supported; use `[]` to disable". Full re-selection is a later follow-up.                                                                                                                                                                  |
| **Stage cache key**               | **Fold the override into the key.** Hash the applied `PartialConfig` into `StageCache::key` so a sampling override (temperature/top_p) correctly invalidates a cached stage output.                                                                                                                                                                                                                 |

---

## Summary of the phase

Pipeline stages today share one `Config` instance for the whole run. The model
field is saved/restored per stage (`run_stage` at [`src/pipe.rs:274`](../../src/pipe.rs)),
so the *LLM context* is isolated stage-to-stage — but tool permissions, sampling
params, MCP bindings, and (newly) working directory are not. Phase 36 closes that
gap with an **opt-in, per-stage `config_override:`** that clones the global
`Config`, applies a narrow set of overrides, runs the stage against the clone,
and drops the clone afterward — so mutations never leak across stages. A
**preflight escalation guard** ensures an override can only *narrow* permissions,
never grant ones the parent role lacks. Per-stage telemetry records which fields
were overridden.

| Item | What | Where |
|---|---|---|
| 36A | `config_override:` YAML surface on a stage; `PartialConfig` type; `working_directory` on `Config` | `role.rs`, `mod.rs`, `pipe.rs`, `function.rs` |
| 36B | Clone-and-merge at the stage boundary; cwd applied at tool spawn; cache-key fold | `pipe.rs::run_stage`, `mod.rs::Config::apply_partial`, `function.rs` |
| 36C | Permission-escalation guard at preflight (use_tools subset, mcp disable-only, cwd descendant) | `preflight.rs`, `mod.rs::is_path_descendant` |
| 36D | `config_overrides_applied` in `StageTrace` | `pipe.rs` |

Design tenets (unchanged): extend the existing model-restore pattern (no new
lifecycle hook); opt-in (no-override stages behave exactly as today); permissions
downward-only; telemetry is part of the audit story; one `PartialConfig` type,
`#[non_exhaustive]`.

---

## §0 Reality check — verified facts

Verified against source (all confirmed via grep/Read this session):

- `GlobalConfig = Arc<RwLock<Config>>` (`mod.rs:491`). `read()`/`write()` guards.
- `Config` derives `#[derive(Debug, Clone, Deserialize)]` (`mod.rs:185`) — **Clone
  yes (clone-and-merge works), Serialize NO** (so the new field needs no
  serialize handling; `--info` reads via accessors, not by serializing `Config`).
- `Config` fields: `use_tools: Option<String>` (`:218`, comma-separated, sentinel
  `"all"`), `mcp_servers: IndexMap<String, McpServerConfig>` (`:267`),
  `temperature: Option<f64>` (`:191`), `top_p: Option<f64>` (`:192`).
- Setters exist: `set_use_tools(Option<String>)` (`:1146`),
  `set_temperature(Option<f64>)` (`:1132`), `set_top_p(Option<f64>)` (`:1139`),
  `set_max_output_tokens(Option<isize>)` (`:1210`). **`max_output_tokens` is
  `isize`, not `usize`.** Each setter dispatches `match … { Some(role_like) =>
  role_like.set_…, None => self.field = … }` — **verify on a fresh clone the
  `None` arm fires** (no active role_like) so `apply_partial` writes the plain
  `Config` field (unit test asserts this).
- `Role.use_tools: Option<String>` (`role.rs:149`), `Role.role_mcp_servers:
  Vec<String>` (`role.rs:189`).
- **No per-config working directory exists today.** Only `std::env::current_dir()`
  *reads* (main.rs, repl/pi.rs, utils/variables.rs `__cwd__`), and session
  headers record a `cwd` string. There is a `Config.working_directory_cache:
  Option<PathBuf>` field at `mod.rs:305` with `#[serde(skip)]` — **TODO: confirm
  what it caches before naming the new field** (avoid collision/confusion; the
  new override field is `working_directory`, distinct from `working_directory_cache`).
- Tool spawn point: `Function::run_command_with_stderr_timeout(config:
  &GlobalConfig, bin, args, envs)` at `function.rs:600` already takes `config` and
  reads `config.read().tool_timeout`. It builds a `tokio::process::Command` and
  spawns. **This is where `working_directory` gets applied** (`cmd.current_dir`).

### Overview sketch corrections (for the record)

The overview's `PartialConfig` sketch had wrong types: `use_tools` is
`Option<String>` not `Vec<String>`; `mcp_servers` is a map not a name-`Vec`;
`max_output_tokens` is `isize` not `usize`. The revised type (§0.3) uses the real
types.

### §0.3 `PartialConfig` (final)

```rust
// src/config/mod.rs
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]          // typo'd override keys fail loudly
#[non_exhaustive]                      // future fields are non-breaking
pub struct PartialConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_tools: Option<String>,            // comma-separated; "" narrows to none
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<String>>,     // only [] (disable all) supported now
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<isize>,
}

impl PartialConfig {
    /// 36D: ordered names of the fields set to `Some(_)` — feeds `StageTrace`
    /// and the cache-key fold. Order fixed for deterministic output.
    pub fn applied_fields(&self) -> Vec<String> { /* push in fixed field order */ }
    pub fn is_empty(&self) -> bool { self.applied_fields().is_empty() }
}
```

`#[derive(Deserialize)]` only — `PartialConfig` is never serialized (it rides
inside `RolePipelineStage`, whose `Serialize` is used by `--info`; give
`PartialConfig` a manual no-op or derive `Serialize` too if `--info -o json`
must echo it — **decide during 36A; `--info` echoing the override is the 36A
acceptance test, so it likely needs `Serialize`**).

YAML authoring shape:

```yaml
pipeline:
  - role: research-agent
    model: claude-haiku-4-5
    config_override:
      use_tools: "fs_read,grep,list_files"   # narrow to read-only
      mcp_servers: []                         # disable MCP for this stage
      working_directory: ./research-scratch   # must stay within parent's tree
      temperature: 0.0
  - role: implementation-agent
    model: claude-sonnet-4-6
    config_override:
      use_tools: "fs_read,fs_write,run_command"
```

---

## §1 36A — `config_override:` parses; `working_directory` lands on `Config`

**`src/config/mod.rs`:**
1. Define `PartialConfig` (§0.3) + `applied_fields`/`is_empty`.
2. Add `working_directory: Option<PathBuf>` to `Config` (after the existing fields;
   `#[serde(default)]`). Add `working_directory: None` to the manual `Default`/
   constructor (around `mod.rs:400-456`). Distinct from `working_directory_cache`.

**`src/config/role.rs`:**
3. Add `config_override: Option<PartialConfig>` to `RolePipelineStage`
   (`role.rs:234`), `#[serde(default, skip_serializing_if = "Option::is_none")]`.
   The leaf-stage parse at `role.rs:508`
   (`serde_json::from_value::<RolePipelineStage>`) picks it up automatically —
   **no `parse_pipeline_node` change needed.** Import `PartialConfig`.

**`src/pipe.rs`:**
4. Add `config_override: Option<PartialConfig>` to internal `PipelineStage`
   (`pipe.rs:18`). Thread it through **every** construction site (missing one
   silently drops the override):
   - `run_stage` per-attempt rebuild `attempt_stage` (`:306`) — copy forward, else
     retries/fallbacks lose isolation.
   - `parse_stages` (`:758`, CLI `--stage`) — always `None`.
   - `run_node` Stage arm (`:1827`) — from `RolePipelineStage`.
   - `run_parallel` `CustomRole` merge stage (`:1952`) — `None` (document: merge
     runs un-isolated).
   - `invoke_role` sequential builder (`:1385`) + non-pipeline single-stage
     fallback (`:1395`).
   - `invoke_role_streaming` sequential builder (`:1585`).
   - `run_inline_pipeline` (`:1688`, server inline stages) — `None`.
   Consider `#[derive(Default)]` on `PipelineStage` + `..Default::default()` to
   make the additive field painless across ~8 sites.

**Acceptance (36A):** a role with valid `config_override:` parses without warning;
`aichat --info -r <role> -o json` shows the override in the pipeline section. (Inert
until 36B — that's why 36A+36B ship in one PR.)

---

## §2 36B — clone-and-merge; cwd at spawn; cache-key fold

**`src/config/mod.rs` — `Config::apply_partial`:**
```rust
impl Config {
    pub fn apply_partial(&mut self, p: &PartialConfig) -> Result<()> {
        if let Some(t) = p.use_tools.clone()  { self.set_use_tools(Some(t)); }
        if let Some(t) = p.temperature        { self.set_temperature(Some(t)); }
        if let Some(t) = p.top_p              { self.set_top_p(Some(t)); }
        if let Some(t) = p.max_output_tokens  { self.set_max_output_tokens(Some(t)); }
        if let Some(d) = p.working_directory.clone() { self.working_directory = Some(d); }
        if let Some(servers) = &p.mcp_servers {
            if servers.is_empty() { self.mcp_servers.clear(); }
            else { bail!("config_override.mcp_servers: only [] (disable all) \
                          is supported in this release"); }
        }
        Ok(())
    }
}
```

**`src/pipe.rs` — `run_stage`** (clone seam at the top, before the candidates loop):
```rust
let stage_config: GlobalConfig = match &stage.config_override {
    Some(p) if !p.is_empty() => {
        let mut cloned = config.read().clone();      // mirrors macro isolation
        cloned.apply_partial(p)?;
        Arc::new(RwLock::new(cloned))
    }
    _ => config.clone(),                              // shared handle, as today
};
// use &stage_config everywhere run_stage currently uses `config`.
```
- Existing `saved_model_id`/`set_model` restore stays; on the override path it
  operates on the throwaway clone (harmless), on the shared path it's byte-for-byte
  today's behavior. **Backward compat: no-override stage → `config.clone()` arm.**
- `Arc`/`RwLock`: `GlobalConfig = Arc<RwLock<Config>>` confirmed. Drop the
  `read()` guard before `Arc::new(RwLock::new(cloned))` (don't hold it across).

**`src/function.rs` — apply cwd at spawn** (`run_command_with_stderr_timeout`,
after building `cmd`, before `spawn`):
```rust
if let Some(dir) = config.read().working_directory.clone() {
    cmd.current_dir(dir);
}
```
Per-command and isolated: each fan-out branch holds its own cloned config, so
concurrent branches with different cwds don't interfere. No process-global state.

**Cache key fold** (`pipe.rs` ~`:573`, `StageCache::key(role, model, text)`): extend
to include the override fingerprint, e.g. append
`stage.config_override.as_ref().map(|p| p.applied_fields().join(","))` **and the
values** (a sampling change must change the key) — simplest correct approach: hash
a small `serde_json`/`format!` of the `PartialConfig` into the key input. Add a
unit test that two temperatures yield different keys.

---

## §3 36C — escalation guard (follow-up PR)

Extend preflight in [`src/config/preflight.rs`](../../src/config/preflight.rs).
`validate_pipeline_stages` (`:52`) takes `&[(String, Option<String>)]` (no override,
no parent handle). Add a sibling **`validate_pipeline_overrides(parent_role,
nodes)`** called from the same preflight blocks that already call
`validate_pipeline_stages` (in `run`, `invoke_role`, `invoke_role_streaming`,
`run_inline_pipeline`). It needs the **parent (pipeline-owning) role**'s permission
set plus each stage's override.

| Field | Rule |
|---|---|
| `use_tools` | Override set ⊆ parent set. Split comma-strings, trim, compare as sets. Parent `"all"` ⇒ any override OK (narrowing). Override `"all"` when parent ≠ `"all"` ⇒ **reject**. Empty override ⇒ narrow to none, OK. |
| `mcp_servers` | `[]` OK (disable). Non-empty ⇒ reject ("not supported; use `[]`"). |
| `working_directory` | Canonicalize override against the parent cwd (parent's `working_directory` or, if `None`, `std::env::current_dir()`); assert the result is a **descendant** of the parent tree. `..`-escape ⇒ reject. Symlink resolution out of scope (trust model: roles are trusted code; guarding accidents, not adversaries). Helper `is_path_descendant(parent, child)` in `mod.rs`. |
| `temperature`/`top_p`/`max_output_tokens` | No check — tuning knobs. |

Error format (config error → exit code 3 via `bail!`→`classify_error`):
```
error: pipeline stage 1 ('research-agent') config_override grants use_tools
       [run_command] not held by parent role 'my-pipeline-role'
       (parent grants: [fs_read, grep]). Overrides may only narrow tool
       permissions, never escalate them.
  hint: add 'run_command' to the parent role's use_tools, or remove it from
        the stage override.
```
**Parent role caveat:** only meaningful for frontmatter/`--pipe-def` pipelines
(they have an owning role). CLI `--stage` pipelines carry no override and no parent
permission set — guard is a no-op there. Document it.

---

## §4 36D — telemetry in `StageTrace`

`src/pipe.rs`, add to `StageTrace` (`:40`):
```rust
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub config_overrides_applied: Vec<String>,   // e.g. ["use_tools","temperature"]
```
`skip_serializing_if = "Vec::is_empty"` keeps it out of JSON for the no-override
case (matches `cached`/`branch` terseness). Populate at every `StageTrace { … }`
literal (~6: `invoke_role` ×2, `invoke_role_streaming` ×2, `run_node` Stage arm,
`run_parallel` custom-merge, `run_inline_pipeline`) from
`stage.config_override.as_ref().map(PartialConfig::applied_fields).unwrap_or_default()`.
`#[serde(default)]` keeps `StageTrace::default()` and the `Default` derive working.
`-o text`/trace tree omit it (no `render_trace_tree` change); `-o json` emits it. No
`serve.rs` change.

---

## §5 Test plan (TDD — red tests first, then implement, then showboat)

Per `[[feedback_tdd]]`: failing tests first.

### Rust unit tests
**`src/config/mod.rs`** (`apply_partial`/`PartialConfig`):
- `apply_partial_only_touches_some_fields`
- `apply_partial_narrows_use_tools`
- `apply_partial_sets_working_directory`
- `apply_partial_empty_mcp_disables` / `apply_partial_nonempty_mcp_rejected`
- `partial_applied_fields_lists_set_fields` (fixed order)
- `is_path_descendant_*` (descendant ok; `..`-escape rejected; equal-path ok)

**`src/pipe.rs`:**
- `clone_and_merge_produces_independent_config`
- `run_stage_with_override_drops_clone_on_completion` (Arc strong_count → 1; may
  need a test seam at the clone helper if `run_stage` needs a live client)
- `stage_trace_records_applied_overrides`
- `cache_key_changes_with_sampling_override`

**`src/config/preflight.rs`:**
- `escalation_use_tools_rejected` / `narrowing_use_tools_accepted`
- `escalation_use_tools_all_rejected` / `parent_all_allows_any_override`
- `mcp_disable_allowed_nonempty_rejected`
- `working_directory_must_be_descendant` / `working_directory_escape_rejected`

### bats — `tests/regression/pipeline-isolation.sh`
(offline per `[[reference_offline_pipeline_demos]]`: trivial echo roles + `--dry-run`)
- 36A parse + `--info -r <role> -o json` shows overrides.
- 36B isolation: stage 1 narrows tools; `-o json` shows stage 2 has parent's tools.
- 36B cwd isolation: instrumented role echoes effective cwd; stage 2 sees parent cwd.
- 36C escalation-rejected: parent `fs_read`, stage `run_command` → preflight fail, exit 3.
- 36C narrowing-allowed: subset → passes.
- 36C cwd-escape: `working_directory: ../../etc` → preflight fail.
- 36D json: `stages[].config_overrides_applied` present for overridden stages, absent/empty otherwise.
- Backward compat: 2-stage no-override pipeline identical to pre-Phase-36.

### showboat
Evergreen `showboat note` (per `CLAUDE.md`) offline: a 2-stage pipeline whose
stage 1 narrows `use_tools`, shown via `--check` + `-o json` trace (deterministic).
**Do NOT run `showboat verify` this session** — `[[feedback_no_showboat_verify]]`.

---

## §6 Sequencing
1. **PR 1 — 36A + 36B** (overview rule: 36A alone is an ignored field). Includes
   `PartialConfig`, `working_directory` on `Config`, `apply_partial`, `run_stage`
   clone seam, cwd-at-spawn, cache-key fold, field threading, unit tests, and the
   36A/36B/backward-compat bats. **No guard yet.**
2. **PR 2 — 36C + 36D** (telemetry independent of guard; ship together for a
   consistent audit story). `validate_pipeline_overrides`, `is_path_descendant`,
   `StageTrace` field + population, escalation/cwd unit tests, 36C/36D bats +
   showboat.
3. **Doc PR (deferred, with Phases 34/35):** `dependencies.md`/`success-metrics.md`.

---

## §7 Verify-during-impl checklist
1. What `Config.working_directory_cache` (`mod.rs:305`) caches — ensure the new
   `working_directory` field doesn't shadow/duplicate it.
2. Setter `None`-arm fires on a fresh clone (no active `role_like`) so
   `apply_partial` mutates the plain `Config`.
3. Whether `--info -o json` requires `PartialConfig: Serialize` (likely yes for the
   36A acceptance test).
4. `RwLock` flavor (parking_lot vs tokio) for the `read()`-then-`Arc::new` seam.

---

## §8 Files touched
| File | Change |
|---|---|
| `src/config/mod.rs` | `PartialConfig` + helpers; `working_directory` on `Config`; `Config::apply_partial`; `is_path_descendant` |
| `src/config/role.rs` | `config_override` on `RolePipelineStage` |
| `src/pipe.rs` | `config_override` on `PipelineStage` (thread ~8 sites); clone-and-merge in `run_stage`; cache-key fold; `config_overrides_applied` on `StageTrace` + population |
| `src/function.rs` | apply `working_directory` via `cmd.current_dir` in `run_command_with_stderr_timeout` |
| `src/config/preflight.rs` | `validate_pipeline_overrides` escalation guard; call from existing preflight blocks |
| `tests/regression/pipeline-isolation.sh` | new bats regression suite |
| `docs/features/…` | user-facing doc for `config_override:` |

**Not touched:** `src/serve.rs` (StageTrace serialization is structural);
`src/config/resolver.rs` (macro isolation orthogonal).
