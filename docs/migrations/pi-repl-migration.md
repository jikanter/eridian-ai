# Migrating to the pi REPL

Pi is the default REPL surface after the Phase 4 cutover. This is the
playbook for existing aichat users adapting to the change. The migration
itself is essentially a one-step install: `aichat` with no input now
launches pi when it's on PATH. The built-in Reedline REPL stays
available behind `--legacy-repl` (or `AICHAT_REPL=legacy`) indefinitely
so the two surfaces can be tested side-by-side. There is no scheduled
removal date for the legacy REPL.

The user guide is in [`docs/repl-pi.md`](../repl-pi.md). This file covers
the *move* — what to do with what you already have.

---

## Step 1 — Install pi

```bash
curl -fsSL https://pi.dev/install.sh | sh
# or:
npm install -g @earendil-works/pi-coding-agent
```

Confirm: `pi --version` (require ≥ 0.74.0).

## Step 2 — Try it side-by-side

Both REPLs are first-class. The legacy REPL is unchanged.

```bash
aichat                  # default: pi (or built-in, with a note, if pi missing)
aichat --legacy-repl    # force the built-in REPL
aichat --pi-repl        # strict: error if pi isn't on PATH
```

Try a few of your usual flows in the pi launch:

- `/role <name>` to switch roles
- `/agent <name>` to bind an agent
- `/macro <name>` to run a macro
- `/info` (or `/info role`) to confirm context

If something is missing, see the mapping table in
[`docs/repl-pi.md`](../repl-pi.md#slash-command-mapping). Several
lower-priority dot-commands are not yet bridged; they fall under the
follow-up phase.

## Step 3 — Carry over a session you care about

aichat sessions remain the source of truth for **batch mode** (`aichat -s
<name>`). They are not auto-discovered by pi's `/resume`. Convert the
ones you want to keep working with interactively:

```bash
# By name, against your configured sessions directory:
aichat --convert-session my-session --to pi \
  --out ~/.pi/agent/sessions/imported-my-session.jsonl

# By path:
aichat --convert-session ~/.config/aichat/sessions/my-session.yaml \
  --to pi --out ~/.pi/agent/sessions/imported.jsonl
```

`~/.pi/agent/sessions/` is the directory pi reads. After conversion the
session appears in pi's `/resume` selector on next launch.

Things to know about the converter:

- It's one-way (aichat → pi). aichat will continue to read the original
  YAML file unchanged.
- System messages are dropped; pi composes the system prompt at session
  start from the model + extension config.
- Compressed history is flattened in front of live messages — no
  `CompactionEntry` is emitted yet.
- Tool calls split correctly into one assistant entry + one toolResult
  per call.
- Token usage/cost numbers are zeroed (aichat never stored them per
  message).

## Step 4 — Decide what stays in aichat config

These continue to live in your aichat config directory and are surfaced
through the bridge — you don't need to move them:

- Roles (`<config>/roles/*.md`)
- Macros (`<config>/macros/*.yaml`)
- Agents (under `<functions_dir>/agents/<name>/`)
- RAGs (`<config>/rags/*`)
- MCP servers (`config.yaml` `mcp_servers:` and `~/.config/mcp/mcp.json`)

Pi-specific things you may want:

- Themes: `~/.pi/agent/themes/`
- Skills, prompt templates, extra extensions: under `~/.pi/agent/`
- Per-project pi settings: `<project>/.pi/` (the launcher writes the
  bundled bridge under `<project>/.pi/extensions/`)

## Step 5 — Pick your default (optional)

After the Phase 4 cutover, pi is the default. No action is needed unless
you'd rather make legacy the persistent choice on your machine:

```bash
# ~/.zshrc or ~/.bashrc
export AICHAT_REPL=legacy
```

Per-invocation overrides:

```bash
aichat --pi-repl       # strict pi (error if missing)
aichat --legacy-repl   # built-in REPL, silently
```

## Things that don't move

- **Batch mode** (`aichat "<prompt>"`, `aichat -f file`, `aichat -r role`,
  `aichat -a agent`, `aichat --macro`, `aichat --pipe`, `aichat --serve`,
  `aichat --mcp`) is unaffected. The pi REPL only replaces the
  interactive surface.
- **Existing aichat sessions** keep working in batch mode after
  conversion to pi for REPL use — converting doesn't move or delete the
  original.
- **The `--serve` mode's endpoint surface** is unchanged for external
  clients. The `/v1/state/*` routes the bridge uses only activate when
  `AICHAT_BRIDGE_TOKEN` is set at server boot — i.e., only during a
  `--pi-repl` launch.

## Reverting

There is no destructive step in this migration. To go back to the
built-in REPL:

```bash
# Per-invocation:
aichat --legacy-repl

# Persistent:
export AICHAT_REPL=legacy   # in your shell rc
```

The original session YAML files are untouched by the converter. Pi's
imported copies live under `~/.pi/agent/sessions/` and can be deleted at
will.
