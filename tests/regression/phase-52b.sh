#!/usr/bin/env bats

# Phase 52B (Facet taxonomy): the --dry-run preview surfaces an entity's facet
# families, tagged owned vs referenced, via Entity::facets(). No LLM call —
# preview emits to stderr before any API request.
load common.bash

ROLES_DIR="${AICHAT_ROLES_DIR:-$HOME/Library/Application Support/aichat/roles}"

setup() {
  mkdir -p "$ROLES_DIR"
  # Owns Shape (schemas), references Act (use_tools) — a mixed entity exercising
  # the backing-gates-ownership rule (§5.2): a file-role can own declarative
  # facets but only reference executable ones.
  cat > "$ROLES_DIR/p52b-mixed.md" <<'EOF'
---
description: Phase 52B facet fixture
use_tools: fs_read
input_schema:
  type: object
  properties:
    code: { type: string }
output_schema:
  type: object
  properties:
    issues: { type: array }
---
You review code.
EOF
  # A bare role with no facets — the line must be omitted, not blank.
  cat > "$ROLES_DIR/p52b-bare.md" <<'EOF'
---
description: Phase 52B bare fixture
---
Say hi.
EOF
}

teardown() {
  rm -f "$ROLES_DIR/p52b-mixed.md" "$ROLES_DIR/p52b-bare.md"
}

@test "phase-52b: --dry-run preview lists facets tagged owned/referenced" {
  err_file="$(mktemp)"
  "$AICHAT_BIN" -r p52b-mixed --dry-run '{"code":"x=1"}' >/dev/null 2>"$err_file"
  status=$?
  [ "$status" -eq 0 ]
  grep -q "Resolved Role: p52b-mixed" "$err_file"
  grep -q "facets: Act(ref), Shape(owned)" "$err_file"
  rm -f "$err_file"
}

@test "phase-52b: --dry-run preview omits the facets line when there are none" {
  err_file="$(mktemp)"
  "$AICHAT_BIN" -r p52b-bare --dry-run "hello" >/dev/null 2>"$err_file"
  ! grep -q "facets:" "$err_file"
  rm -f "$err_file"
}