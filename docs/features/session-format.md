# Session format: pi JSONL (native)

aichat's native session format is pi's **v3 JSONL session-tree** format
(`<name>.jsonl`). The previous YAML format (`<name>.yaml`) is **deprecated**
and read-only: aichat still loads it, warns once, and rewrites it as JSONL on
the next save.

Storing sessions in pi's own format means a session created in batch mode
(`aichat -s work ...`) and one created in the pi REPL are the same kind of
file — no conversion step to resume a batch session interactively, and pi's
`/resume` sees them directly.

## What changed

| | Before | Now |
|---|---|---|
| On-disk format | `<name>.yaml` (aichat YAML) | `<name>.jsonl` (pi v3 tree) |
| New sessions save as | `.yaml` | `.jsonl` |
| Loading | YAML only | `.jsonl` **or** legacy `.yaml` (auto-detected) |
| Listing / delete / completion | `.yaml` | both, de-duplicated |

Detection is by content, not extension: the first non-empty line of a JSONL
session is a JSON object with `"type": "session"`. Anything else is parsed as
legacy YAML.

## Lossless round-trip

Pi's tree format has no slot for aichat's session metadata (model, role,
sampling params, agent variables/instructions, the compression boundary). aichat
stores that metadata in an `aichat` key in the session **header** line. Pi
ignores unknown header keys, so the file stays fully pi-loadable while aichat
recovers every field on load.

Two more mirrors keep the conversation itself lossless, where pi's text-only
content blocks would otherwise flatten data:

- `aichatOutput` on each `toolResult` entry — the raw (possibly structured)
  tool output `Value`, not just its text rendering.
- `aichatUrl` on each image block — the original aichat image URL.

A pi-native session that lacks these mirrors still imports fine: metadata
defaults, tool outputs fall back to their text content.

System-role messages are still dropped on export — pi composes the system
prompt from the model + extension config at session start, so re-seeding it
would double up.

## Migrating existing sessions

Convert every legacy YAML session in your sessions directory to JSONL in one
shot:

```bash
aichat --migrate-sessions
```

It recurses into the auto-named `_/` subdirectory, writes a `.jsonl` beside
each `.yaml`, and removes the `.yaml` once the `.jsonl` is written. Progress
and a final count go to stderr; the exit code is non-zero if any file failed.

Sessions you don't migrate convert lazily: the first time aichat loads a legacy
`.yaml` it warns and, on the next save, writes `.jsonl` (the `.yaml` is left in
place until you migrate or delete it).

The sessions directory is resolved from `$AICHAT_SESSIONS_DIR`, else
`<config dir>/sessions`. Agent sessions (under each agent's data dir) are not
swept by `--migrate-sessions`; they convert lazily on next save.

## Inspecting a single session

`--convert-session` still emits pi JSONL for one named or path-given YAML
session, to stdout or `--out PATH`, without touching the original:

```bash
aichat --convert-session my-session --to pi | jq          # inspect
aichat --convert-session my-session --to pi --out out.jsonl
```

## Relationship to the pi REPL session store

Where pi's REPL writes sessions (the segregated aichat-owned store vs. the
device-wide `~/.pi/agent/sessions/`) is a separate concern documented in
[`repl-pi.md`](repl-pi.md#sessions-pi-owns-the-format). This page is about the
on-disk **format** of aichat's own sessions, which is now the same JSONL format
pi uses.