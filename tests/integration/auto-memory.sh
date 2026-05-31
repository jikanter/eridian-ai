#!/usr/bin/env bats

# Phase 34A: Auto-memory read surface.
#   34A  read `memory/MEMORY.md` at startup, cap at 200 lines / 8 KiB, inject
#        the capped content into the system prompt and surface it in
#        `aichat --info -o json` as `memory_preamble`.
#
# All checks are offline: `--info` reads config and the memory file but makes
# no model request. The memory directory is isolated per-test via the
# AICHAT_MEMORY_DIR override so the repo's own `memory/MEMORY.md` never leaks
# into the assertions.

AICHAT_BIN="${AICHAT_BIN:-./target/debug/aichat}"

setup() {
  MEM_DIR="$(mktemp -d)"
  export AICHAT_MEMORY_DIR="$MEM_DIR"
}

teardown() {
  rm -rf "$MEM_DIR"
  unset AICHAT_MEMORY_DIR
}

# ----- 34A: presence in --info -o json -----

@test "auto-memory: MEMORY.md content surfaces in --info -o json" {
  cat > "$MEM_DIR/MEMORY.md" <<EOF
- [Cite sources](feedback_cite_sources.md) — link docs inline
EOF
  run "$AICHAT_BIN" --info -o json
  [ "$status" -eq 0 ]
  [[ "$output" == *"memory_preamble"* ]]
  [[ "$output" == *"Cite sources"* ]]
}

@test "auto-memory: no memory_preamble key when MEMORY.md is absent" {
  run "$AICHAT_BIN" --info -o json
  [ "$status" -eq 0 ]
  [[ "$output" != *"memory_preamble"* ]]
}

@test "auto-memory: empty MEMORY.md yields no preamble" {
  : > "$MEM_DIR/MEMORY.md"
  run "$AICHAT_BIN" --info -o json
  [ "$status" -eq 0 ]
  [[ "$output" != *"memory_preamble"* ]]
}

# ----- 34A: truncation warning -----

@test "auto-memory: truncation warning fires past the 200-line cap" {
  # 250 numbered lines — exceeds the 200-line cap.
  for i in $(seq 1 250); do
    echo "- memory line $i" >> "$MEM_DIR/MEMORY.md"
  done
  run "$AICHAT_BIN" --info -o json
  [ "$status" -eq 0 ]
  # Warning lands on stderr; bats `run` folds stderr into $output.
  [[ "$output" == *"memory preamble cap"* ]]
  [[ "$output" == *"split"* ]]
  # The first 200 lines survive the cap; 201..250 are dropped.
  [[ "$output" == *"memory line 200"* ]]
  [[ "$output" != *"memory line 201"* ]]
  [[ "$output" != *"memory line 250"* ]]
}

@test "auto-memory: under-cap file fires no warning" {
  printf -- '- one\n- two\n- three\n' > "$MEM_DIR/MEMORY.md"
  run "$AICHAT_BIN" --info -o json
  [ "$status" -eq 0 ]
  [[ "$output" != *"memory preamble cap"* ]]
}

# ----- 34B: topic-file lazy loading -----

@test "34B: --memory-load resolves a topic by reference and prints its content" {
  cat > "$MEM_DIR/MEMORY.md" <<EOF
# Memory Index
- [Cite sources](feedback_cite_sources.md) — link docs inline
EOF
  cat > "$MEM_DIR/feedback_cite_sources.md" <<EOF
Always cite sources inline when answering.
EOF
  run "$AICHAT_BIN" --memory-load cite_sources
  [ "$status" -eq 0 ]
  [[ "$output" == *"Always cite sources inline"* ]]
}

@test "34B: --memory-load errors on an unresolvable reference" {
  printf '# Memory Index\n' > "$MEM_DIR/MEMORY.md"
  run "$AICHAT_BIN" --memory-load nope
  [ "$status" -ne 0 ]
}

# ----- 34C: Reflector secret redaction -----

@test "34C: redaction replaces a secret before the Reflector sees it" {
  # AICHAT_MEMORY_REFLECT_ECHO makes the Reflector echo the (redacted)
  # transcript as the candidate body — no LLM call, fully offline.
  printf '# Memory Index\n' > "$MEM_DIR/MEMORY.md"
  TRANSCRIPT="$MEM_DIR/transcript.txt"
  printf 'user: export OPENAI_API_KEY=sk-test-12345 then run it\n' > "$TRANSCRIPT"
  AICHAT_MEMORY_REFLECT_ECHO=1 run "$AICHAT_BIN" --memory-reflect --memory-transcript "$TRANSCRIPT"
  [ "$status" -eq 0 ]
  [[ "$output" == *"[REDACTED:"* ]]
  [[ "$output" != *"sk-test-12345"* ]]
}

# ----- 34D: curator gate -----

@test "34D: accept writes the topic file and appends to MEMORY.md" {
  printf '# Memory Index\n' > "$MEM_DIR/MEMORY.md"
  CANDS="$MEM_DIR/cands.json"
  cat > "$CANDS" <<EOF
{"candidates":[{"topic":"tokio_pref","body":"User prefers tokio.","turns_referenced":[3]}]}
EOF
  run bash -c "printf 'a\n' | '$AICHAT_BIN' --memory-curate --memory-candidates '$CANDS'"
  [ "$status" -eq 0 ]
  [ -f "$MEM_DIR/tokio_pref.md" ]
  grep -q "User prefers tokio." "$MEM_DIR/tokio_pref.md"
  grep -q "(tokio_pref.md)" "$MEM_DIR/MEMORY.md"
}

@test "34D: reject-all aborts cleanly with exit 0 and writes nothing" {
  printf '# Memory Index\n' > "$MEM_DIR/MEMORY.md"
  CANDS="$MEM_DIR/cands.json"
  cat > "$CANDS" <<EOF
{"candidates":[
  {"topic":"a","body":"one"},
  {"topic":"b","body":"two"},
  {"topic":"c","body":"three"}
]}
EOF
  run bash -c "printf 'r\n' | '$AICHAT_BIN' --memory-curate --memory-candidates '$CANDS'"
  [ "$status" -eq 0 ]
  [ ! -f "$MEM_DIR/a.md" ]
  [ ! -f "$MEM_DIR/b.md" ]
  [ ! -f "$MEM_DIR/c.md" ]
}

@test "34D: --memory-auto-curate accepts every candidate without prompting" {
  printf '# Memory Index\n' > "$MEM_DIR/MEMORY.md"
  CANDS="$MEM_DIR/cands.json"
  cat > "$CANDS" <<EOF
{"candidates":[
  {"topic":"one","body":"first"},
  {"topic":"two","body":"second"}
]}
EOF
  run "$AICHAT_BIN" --memory-curate --memory-candidates "$CANDS" --memory-auto-curate
  [ "$status" -eq 0 ]
  [ -f "$MEM_DIR/one.md" ]
  [ -f "$MEM_DIR/two.md" ]
}

@test "34D + 34B: an accepted candidate becomes lazy-loadable by reference" {
  printf '# Memory Index\n' > "$MEM_DIR/MEMORY.md"
  CANDS="$MEM_DIR/cands.json"
  cat > "$CANDS" <<EOF
{"candidates":[{"topic":"rust_async","body":"Prefer tokio::spawn for new code."}]}
EOF
  "$AICHAT_BIN" --memory-curate --memory-candidates "$CANDS" --memory-auto-curate
  run "$AICHAT_BIN" --memory-load rust_async
  [ "$status" -eq 0 ]
  [[ "$output" == *"Prefer tokio::spawn"* ]]
}
