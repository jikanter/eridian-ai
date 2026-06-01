#!/usr/bin/env bats

# Phase 36: Pipeline Stage Config Isolation.
#
# Offline-deterministic coverage via `--check` (the Phase 15C preflight
# validator) — no LLM call, no network. Runtime trace assertions for
# `config_overrides_applied` (36D) and working-directory isolation (36B) live
# in the Rust unit tests (`src/pipe.rs`, `src/config/{mod,preflight}.rs`), which
# don't need a live model.
#
#   36A — a role with a valid `config_override:` parses and `--check`s clean.
#   36C — escalation (use_tools / mcp re-select / cwd escape) fails `--check`
#         with a non-zero exit; narrowing passes.
#   backward-compat — a no-override pipeline `--check`s exactly as before.

load common.bash

ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"

setup() {
  mkdir -p "$ROLES_DIR"
  printf -- '---\n---\nA: __INPUT__\n' > "$ROLES_DIR/iso-stage-a.md"
  printf -- '---\n---\nB: __INPUT__\n' > "$ROLES_DIR/iso-stage-b.md"
}

teardown() {
  rm -f "$ROLES_DIR/iso-stage-a.md" \
        "$ROLES_DIR/iso-stage-b.md" \
        "$ROLES_DIR/iso-narrow.md" \
        "$ROLES_DIR/iso-escalate.md" \
        "$ROLES_DIR/iso-cwd-escape.md" \
        "$ROLES_DIR/iso-mcp-reselect.md" \
        "$ROLES_DIR/iso-plain.md" \
        "$ROLES_DIR/iso-bogus-key.md"
}

# ---- 36A / 36C narrowing: valid override passes --check ----

@test "config-isolation: narrowing use_tools passes --check" {
  cat > "$ROLES_DIR/iso-narrow.md" <<'EOF'
---
use_tools: "fs_read,grep,run_command"
pipeline:
  - role: iso-stage-a
    config_override:
      use_tools: "fs_read,grep"
      temperature: 0.0
  - role: iso-stage-b
---
EOF
  run "$AICHAT_BIN" -r iso-narrow --check </dev/null
  [ "$status" -eq 0 ]
  [[ "$output" == *"check passed"* ]]
}

@test "config-isolation: --check -o json reports valid for override role" {
  cat > "$ROLES_DIR/iso-narrow.md" <<'EOF'
---
use_tools: "fs_read,grep"
pipeline:
  - role: iso-stage-a
    config_override:
      use_tools: "fs_read"
  - role: iso-stage-b
---
EOF
  run "$AICHAT_BIN" -r iso-narrow --check -o json </dev/null
  [ "$status" -eq 0 ]
  [[ "$output" == *'"valid": true'* ]]
}

# ---- 36C: use_tools escalation rejected ----

@test "config-isolation: use_tools escalation fails --check" {
  cat > "$ROLES_DIR/iso-escalate.md" <<'EOF'
---
use_tools: "fs_read"
pipeline:
  - role: iso-stage-a
    config_override:
      use_tools: "run_command"
  - role: iso-stage-b
---
EOF
  run "$AICHAT_BIN" -r iso-escalate --check </dev/null
  [ "$status" -ne 0 ]
  [[ "$output" == *"run_command"* ]]
  [[ "$output" == *"narrow"* ]]
}

# ---- 36C: working_directory escape rejected ----

@test "config-isolation: working_directory escape fails --check" {
  cat > "$ROLES_DIR/iso-cwd-escape.md" <<'EOF'
---
use_tools: "fs_read"
pipeline:
  - role: iso-stage-a
    config_override:
      working_directory: "../../etc"
  - role: iso-stage-b
---
EOF
  run "$AICHAT_BIN" -r iso-cwd-escape --check </dev/null
  [ "$status" -ne 0 ]
  [[ "$output" == *"escapes"* ]]
}

# ---- 36C: mcp_servers re-selection rejected (disable-only this release) ----

@test "config-isolation: non-empty mcp_servers override fails --check" {
  cat > "$ROLES_DIR/iso-mcp-reselect.md" <<'EOF'
---
use_tools: "fs_read"
pipeline:
  - role: iso-stage-a
    config_override:
      mcp_servers:
        - some-server
  - role: iso-stage-b
---
EOF
  run "$AICHAT_BIN" -r iso-mcp-reselect --check </dev/null
  [ "$status" -ne 0 ]
  [[ "$output" == *"not supported"* ]]
}

# ---- deny_unknown_fields ----
#
# `PartialConfig` is `#[serde(deny_unknown_fields)]`, so a typo'd key fails to
# deserialize — verified at the type level by the Rust unit test
# `partial_config_rejects_unknown_key`. Note: the role *frontmatter* parser
# (`role.rs`) skips an unparseable pipeline node with a `warn!` rather than
# hard-failing (a pre-existing, pipeline-wide behavior), so the stage is dropped
# rather than the whole role rejected. That leniency is intentionally left
# unchanged by Phase 36; the strictness guarantee lives at the type boundary.

# ---- backward compatibility: no-override pipeline unchanged ----

@test "config-isolation: no-override pipeline still --checks clean" {
  cat > "$ROLES_DIR/iso-plain.md" <<'EOF'
---
pipeline:
  - role: iso-stage-a
  - role: iso-stage-b
---
EOF
  run "$AICHAT_BIN" -r iso-plain --check </dev/null
  [ "$status" -eq 0 ]
  [[ "$output" == *"check passed"* ]]
}
