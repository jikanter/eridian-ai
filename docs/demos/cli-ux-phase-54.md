# Phase 54: CLI UX hardening

*2026-06-21T18:06:51Z by Showboat 0.6.1*
<!-- showboat-id: 15831879-97c2-4e8f-b2e1-8f75d6308ace -->

Phase 54 brings the batch CLI up to clig.dev / GNU conventions without breaking the existing flag contract. Every check below is deterministic and needs no model — config-dependent steps use an isolated, throwaway config dir.

## 54A — grouped help: the ~90 flags now sit under 14 named sections instead of one wall.

```bash
target/debug/aichat --help | grep -E '^[A-Z][A-Za-z ]*:$'
```

```output
Arguments:
Options:
Core:
Sessions:
RAG:
Server:
REPL:
MCP:
Execution:
Output:
Input:
Setup:
Discovery:
Roles:
Knowledge:
Memory:
```

54A also ships a man page generated from the same clap definitions (no hand-maintained copy): `aichat --man > man/aichat.1`.

```bash
target/debug/aichat --man | grep -E '^\.TH aichat'
```

```output
.TH aichat 1  "aichat 0.7.4-eridian-DEBUG" 
```

## 54B — --color overrides TTY detection. On the (model-free) error path, --color=always keeps ANSI through a pipe; never/auto stay plain. Count of ESC sequences:

```bash
d=$(mktemp -d); printf "compress_threshold: 1\n" > "$d/config.yaml"; esc=$(printf "\033["); for w in always never auto; do n=$(AICHAT_CONFIG_DIR="$d" target/debug/aichat --config-get badkey --color=$w 2>&1 1>/dev/null | grep -cF "$esc"); echo "--color=$w -> $n ANSI seq"; done; rm -rf "$d"
```

```output
--color=always -> 1 ANSI seq
--color=never -> 0 ANSI seq
--color=auto -> 0 ANSI seq
```

-q/--quiet and a global --verbose round out the standard flags (both under Output):

```bash
target/debug/aichat --help | awk '/^[A-Z][A-Za-z ]*:$/{s=$0} s=="Output:"{print}' | grep -E -- '--quiet|--verbose|--color'
```

```output
      --color <WHEN>
  -q, --quiet
      --verbose
```

## 54C — destructive ops refuse without confirmation when stdin is not a TTY (no hang), and exit with the usage code (2). --migrate-sessions, with a legacy .yaml present but no --yes:

```bash
d=$(mktemp -d); printf "compress_threshold: 1\n" > "$d/config.yaml"; mkdir -p "$d/sessions"; printf "dummy: true\n" > "$d/sessions/foo.yaml"; AICHAT_SESSIONS_DIR="$d/sessions" AICHAT_CONFIG_DIR="$d" target/debug/aichat --migrate-sessions; echo "exit=$?"; echo "yaml preserved: $([ -f "$d/sessions/foo.yaml" ] && echo yes)"; rm -rf "$d"
```

```output
Refusing to migrate 1 legacy YAML session(s) without confirmation (stdin is not a terminal or --no-input is set). Re-run with --yes to proceed.
exit=2
yaml preserved: yes
```

## 54D — unknown role/agent/model names suggest the nearest real candidate (Levenshtein). A one-char typo of a real role:

```bash
d=$(mktemp -d); printf "compress_threshold: 1\n" > "$d/config.yaml"; mkdir -p "$d/roles"; printf "hello\n" > "$d/roles/summarize.md"; AICHAT_CONFIG_DIR="$d" target/debug/aichat --explain-role summarise 2>&1 | tail -1; rm -rf "$d"
```

```output
    Unknown role `summarise`. Did you mean `summarize`?
```

## 54E — read-only config introspection. --config-path resolves the file statically (no model); --config-get reads one value (and -o json), suggesting near keys on a typo:

```bash
d=$(mktemp -d); printf "compress_threshold: 1234\n" > "$d/config.yaml"; echo "path: $(AICHAT_CONFIG_DIR=$d target/debug/aichat --config-path)"; echo "get:  $(AICHAT_CONFIG_DIR=$d target/debug/aichat --config-get compress_threshold)"; echo "json: $(AICHAT_CONFIG_DIR=$d target/debug/aichat --config-get compress_threshold -o json | tr -d "\n ")"; AICHAT_CONFIG_DIR=$d target/debug/aichat --config-get compres_threshold 2>&1 | tail -1; rm -rf "$d"
```

```output
path: /var/folders/fy/24gf3kvn5y50x_44lbb7pzxr0000gn/T/tmp.tOLQAXPUia/config.yaml
get:  1234
json: {"compress_threshold":"1234"}
Error: Unknown config key `compres_threshold`. Did you mean `compress_threshold`?
```

Every step above ran without a model, server, or ambient config — each config-dependent block used an isolated, throwaway `AICHAT_CONFIG_DIR`, so this demo is reproducible in CI. The noun-verb subcommand layer (54F) is held pending review; see the cross-surface command map in docs/features/cross-surface-commands.md.
