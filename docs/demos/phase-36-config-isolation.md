# Phase 36: Pipeline Stage Config Isolation

*2026-06-01T18:57:43Z by Showboat 0.6.1*
<!-- showboat-id: 105b5601-bc8a-45a5-b7bb-93dfa42c3964 -->

Pipeline stages historically shared one `Config` for the whole run — the model field was saved/restored per stage, but tool permissions, sampling, MCP bindings, and working directory were not. Phase 36 adds an opt-in, per-stage `config_override:` that runs the stage against a *clone* of the global config; mutations never leak across stages. A preflight guard ensures an override can only **narrow** permissions, never escalate beyond the parent role.

- **36A** — `config_override:` YAML on a stage; `PartialConfig` type; `working_directory` on `Config`.
- **36B** — clone-and-merge at the stage boundary (config-scoped: cwd + MCP); role-scoped fields (tools, sampling) applied to the stage role; per-command `cmd.current_dir`; cache-key fold.
- **36C** — preflight escalation guard (use_tools subset, MCP disable-only, cwd descendant).
- **36D** — `config_overrides_applied` in `StageTrace` for `-o json` audit.

## The new surface (src/config/mod.rs)

```bash
grep -E "pub struct PartialConfig|pub fn (applied_fields|is_empty|apply_to_role|apply_partial|is_path_descendant)" src/config/mod.rs
```

```output
pub struct PartialConfig {
    pub fn applied_fields(&self) -> Vec<String> {
    pub fn is_empty(&self) -> bool {
    pub fn apply_to_role(&self, role: &mut Role) {
pub fn is_path_descendant(parent: &Path, child: &Path) -> bool {
    pub fn apply_partial(&mut self, p: &PartialConfig) -> Result<()> {
```
