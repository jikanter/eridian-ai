# Phase 12: Discoverability & Previews : Overview - Epic 3

**Status (2026-05-04):** Shipped alongside Phase 14. Preview hits stderr so stdout stays pipeable; `--list-roles --verbose` and `--find-role` share a single renderer.

| Item | Description | Status |
|---|---|---|
| 12A | Resolved prompt preview (`--dry-run` with `extends`/`include` expanded, variables interpolated) | **Done** |
| 12B | Pipeline visualization in `--dry-run` (numbered stages with model per row) | **Done** |
| 12C | Port signatures in `--list-roles` (`--verbose` shows `in: ... out: ...` plus capabilities and tools) | **Done** |
| 12D | Composition summary after `.role <name>` in REPL (one line: extends, includes, tools, ports, capabilities, pipeline) | **Done** |

**Files touched:**
- `src/main.rs` — `emit_dry_run_preview` (stderr block in `start_directive`
  before `call_react`); `render_role_list` shared by `--list-roles` and
  `--find-role`; honors `-o json`.
- `src/repl/mod.rs` — `print_role_composition_summary` called from `.role
  <name>`; imports `RoleLike` so `use_tools` resolves.
- `src/cli.rs` — `--verbose` flag.

**Design notes:**
- The preview emits to **stderr**, never stdout. Existing tooling that pipes
  `aichat --dry-run` into another process keeps working unchanged; humans
  still see the preview in their terminal.
- The implicit `%%` temp role (created by `--prompt` or bare `aichat
  "text"`) intentionally suppresses the preview — there is no metadata
  worth showing for an unnamed inline prompt.
- The composition summary stays silent when a role has no metadata to
  surface (no extends/include/tools/capabilities, plain `any → text`
  ports). Avoids noise for trivial roles.

**12A/12B Design — Resolved Preview:**

`--dry-run` already exists but shows the raw prompt. Enhance it to render the *fully resolved* state:

```bash
$ aichat -r code-reviewer --dry-run "review this"

--- Resolved Role: code-reviewer ---
  extends: base-analyst
  includes: [json-output, safety-checks]
  model: claude:claude-sonnet-4-6
  tools: 3 (web_search, fs_cat, execute_command)
  input_schema: { type: "string" }
  output_schema: { properties: { issues: [...], severity: [...] } }

--- Pipeline ---
  1. extract (deepseek:deepseek-chat)
  2. review (claude:claude-sonnet-4-6)
  3. format (deepseek:deepseek-chat)

--- Assembled Prompt (847 tokens) ---
  [system] You are a code review assistant...
  [user] review this

--- Estimated Cost ---
  $0.003 (3 stages, ~2400 tokens total)
```

Zero tokens spent. This is the "terraform plan" moment — the most beloved command in that ecosystem because it eliminates the fear of "what will this actually do?"

**Files:** `src/main.rs` (enhance `--dry-run` path), `src/config/role.rs` (add `resolve_full()` that expands extends/include/variables).

**12C Design — Port Signatures:**

```bash
$ aichat --list-roles --verbose
  code-reviewer    in: text      out: json{issues, severity}    3 tools   extends: base-analyst
  summarizer       in: text      out: text                      0 tools
  classifier       in: json{...} out: json{label, confidence}   0 tools   pipeline: 2 stages
```

Derived from existing `input_schema`/`output_schema`. A one-line human-readable summary of JSON Schema top-level properties. When no schema is defined, shows `in: any, out: text`.

**Files:** `src/config/role.rs` (add `port_signature()` method), `src/main.rs` or `src/config/mod.rs` (render in list output).
