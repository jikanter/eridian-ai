# Session storage cutover: aichat YAML → pi v3 JSONL

*2026-06-10T17:36:02Z by Showboat 0.6.1*
<!-- showboat-id: 7a6a344c-1a9d-4da4-ac0c-52085a712525 -->

aichat's native session format is now pi's v3 JSONL tree (`<name>.jsonl`). The old YAML format is deprecated and read-only: aichat still loads it, warns once, and rewrites it as JSONL on the next save. A batch-mode session and a pi-REPL session are now the same kind of file.

This demo runs fully offline (no model calls). All session files live in a throwaway directory pointed at by `AICHAT_SESSIONS_DIR`. Volatile fields (random session id, timestamps) are stripped with `jq` so the demo stays reproducible.

## 1. A legacy YAML session

Start with a pre-cutover session on disk: aichat YAML, with metadata (model, role, sampling) and a two-turn conversation.

```bash
mkdir -p sessions
cat > sessions/work.yaml <<'YAML'
model: openai:gpt-4o-mini
temperature: 0.5
role_name: coder
messages:
  - role: user
    content: list the rust files
  - role: assistant
    content: src/main.rs and src/lib.rs
YAML
ls sessions
```

```output
work.yaml
```

## 2. One-shot migration

`aichat --migrate-sessions` converts every legacy `.yaml` under the sessions directory to `.jsonl` in place, removing the YAML once the JSONL is written. It recurses into the auto-named `_/` subdir. Progress goes to stderr; the exit code is non-zero if any file fails.

```bash
AICHAT_SESSIONS_DIR="$PWD/sessions" '/Volumes/ExternalData/admin/Developer/Projects/aichat/target/debug/aichat' --migrate-sessions 2>&1 | sed "s#$PWD/##g"
echo '--- sessions dir after ---'
ls sessions
```

```output
migrated sessions/work.yaml -> sessions/work.jsonl
Migrated 1 session(s); 0 failed.
--- sessions dir after ---
work.jsonl
```

## 3. The JSONL header carries aichat metadata

Pi's tree format has no slot for aichat's session metadata (model, role, sampling, compression boundary). aichat stores it in an `aichat` key on the header line — pi ignores unknown header keys, so the file stays pi-loadable while aichat recovers every field on load. Below, the volatile `id`/`timestamp` are dropped to keep the demo evergreen.

```bash
head -1 sessions/work.jsonl | jq '{type, version, aichat}'
```

```output
{
  "type": "session",
  "version": 3,
  "aichat": {
    "model": "openai:gpt-4o-mini",
    "temperature": 0.5,
    "roleName": "coder",
    "compressedEntries": 0
  }
}
```

## 4. The conversation as a pi message tree

Each turn is a `message` entry chained by `parentId`. Showing only the stable role + content (entry ids/timestamps are random per save and stripped here).

```bash
tail -n +2 sessions/work.jsonl | jq -c '{role: .message.role, content: .message.content}'
```

```output
{"role":"user","content":"list the rust files"}
{"role":"assistant","content":[{"type":"text","text":"src/main.rs and src/lib.rs"}]}
```

## 5. Round-trip is lossless

The `aichat` header block plus two pi-ignored side fields — `aichatOutput` (raw structured tool output) and `aichatUrl` (image URL) — let aichat re-import a JSONL session with zero loss, while a plain pi loader simply ignores them. The round-trip is covered by unit tests:

```bash
cargo test --bin aichat pi_export_tests::round_trip 2>/dev/null | grep -E 'round_trip|test result'
```

```output
test config::session::pi_export_tests::round_trip_preserves_text_messages_and_metadata ... ok
test config::session::pi_export_tests::round_trip_preserves_compression_boundary ... ok
test config::session::pi_export_tests::round_trip_preserves_multimodal_image_message ... ok
test config::session::pi_export_tests::round_trip_preserves_compression_boundary_across_tool_calls ... ok
test config::session::pi_export_tests::round_trip_preserves_tool_calls_and_structured_output ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 775 filtered out; finished in 0.02s
```

## Summary

- **Native format:** pi v3 JSONL (`<name>.jsonl`). YAML is deprecated, read-only.
- **Loading:** auto-detected by content (first line `"type":"session"` → JSONL; else legacy YAML + one-time deprecation warning).
- **Migration:** `aichat --migrate-sessions` (bulk, recursive) or lazy (legacy YAML rewrites as JSONL on next save).
- **Lossless:** metadata in the `aichat` header block; structured tool outputs and image URLs in pi-ignored side fields.
- **Listing / delete / completion** transparently span both formats during the transition.

A batch session (`aichat -s work ...`) and a pi-REPL session are now the same file. Pi REPL session-store location (segregated aichat store vs. device-wide `~/.pi`) is a separate concern — see `docs/features/repl-pi.md`. Full format reference: `docs/features/session-format.md`.
