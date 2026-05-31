# Phase 34: Auto-Memory (read surface + lazy-load + write loop)

*2026-05-30T22:29:04Z by Showboat 0.6.1*
<!-- showboat-id: 2813c259-060c-4b6c-b873-48e02f915976 -->

Phase 34A wires a read-only `memory/MEMORY.md` surface. At startup aichat reads the project-local `memory/MEMORY.md` (env override `AICHAT_MEMORY_DIR` for isolation), caps it to 200 lines / 8 KiB, and injects the capped block into the system prompt after the active role's prompt body. The preamble is observable — token-free — via `aichat --info -o json`.

**Setup:** an isolated memory directory with a small MEMORY.md index.

```bash
rm -rf /tmp/p34-demo-mem && mkdir -p /tmp/p34-demo-mem
printf -- '- [Cite sources](feedback_cite_sources.md) — link docs inline in code\n- [Prefer tokio](rust_async.md) — standardize on tokio across the codebase\n' > /tmp/p34-demo-mem/MEMORY.md
cat /tmp/p34-demo-mem/MEMORY.md
```

```output
- [Cite sources](feedback_cite_sources.md) — link docs inline in code
- [Prefer tokio](rust_async.md) — standardize on tokio across the codebase
```

**34A — preamble surfaces in `--info -o json`.** The capped MEMORY.md, framed with a `# Project memory` header, appears under `memory_preamble` without any model call.

```bash
AICHAT_MEMORY_DIR=/tmp/p34-demo-mem ./target/debug/aichat --info -o json | python3 -c 'import sys,json; print(json.load(sys.stdin)["memory_preamble"])'
```

```output
# Project memory
- [Cite sources](feedback_cite_sources.md) — link docs inline in code
- [Prefer tokio](rust_async.md) — standardize on tokio across the codebase
```

**34A — the 200-line / 8-KiB cap.** A MEMORY.md past the cap is truncated and a one-line warning is emitted to stderr nudging the user to split into topic files. Here a 250-line file keeps the first 200 lines; line 201+ are dropped.

```bash
seq 1 250 | sed 's/^/- memory line /' > /tmp/p34-demo-mem/MEMORY.md
AICHAT_MEMORY_DIR=/tmp/p34-demo-mem ./target/debug/aichat --info -o json 2>/tmp/p34-warn.txt >/tmp/p34-info.json
echo '# stderr warning:'
cat /tmp/p34-warn.txt
echo '# last kept line / first dropped line:'
python3 -c 'import json; t=json.load(open("/tmp/p34-info.json"))["memory_preamble"]; print("line 200 present:", "memory line 200" in t); print("line 201 present:", "memory line 201" in t)'
```

```output
# stderr warning:
warning: /tmp/p34-demo-mem/MEMORY.md exceeds the 200-line / 8-KiB memory preamble cap; split it into topic files so context is not dropped
# last kept line / first dropped line:
line 200 present: True
line 201 present: False
```

**34A — absent / empty MEMORY.md is a clean no-op.** No memory directory means no `memory_preamble` key and zero added tokens.

```bash
rm -rf /tmp/p34-demo-mem
AICHAT_MEMORY_DIR=/tmp/p34-demo-mem ./target/debug/aichat --info -o json | python3 -c 'import sys,json; d=json.load(sys.stdin); print("memory_preamble" in d)'
```

```output
False
```

**Surfaces covered.** The Rust loader injects memory for role/agent/prompt turns (`aichat "..."`, `-r`, `-a`, the legacy REPL, and the server's role path) at `Input::build_messages`. Pi's *native* agent turns — which build their own system prompt independent of any aichat role — are covered by the matching `before_agent_start` hook in the bundled pi extension (`assets/pi-extensions/aichat-bridge.js`), capped to the same budget.

---

## 34B — topic-file lazy loading

`MEMORY.md` is the always-loaded index; the topic files it links to are loaded only on demand. A `memory:<reference>` path resolves against the index links and topic filenames; `--memory-load` prints the resolved (capped) topic.

```bash
D=/tmp/p34b-demo-mem; rm -rf "$D"; mkdir -p "$D"
printf '# Memory Index\n- [Cite sources](feedback_cite_sources.md) — link docs inline\n' > "$D/MEMORY.md"
printf 'Always cite sources inline when answering.\n' > "$D/feedback_cite_sources.md"
AICHAT_MEMORY_DIR=$D ./target/debug/aichat --memory-load cite_sources
```

```output
Always cite sources inline when answering.
```

An unresolvable reference errors with a non-zero exit, so a typo never silently loads nothing:

```bash
AICHAT_MEMORY_DIR=$D ./target/debug/aichat --memory-load nope; echo "exit=$?"
```

```output
Error: memory: no topic resolves for reference 'nope'
exit=1
```

---

## 34C — session-exit Reflector with mandatory secret redaction

Before the transcript ever reaches the Reflector, recognized credentials are rewritten to `[REDACTED:<class>]`. There is no flag to disable it. Here `AICHAT_MEMORY_REFLECT_ECHO=1` makes the Reflector echo the redacted transcript as the candidate body — no model call, so the redaction is visible directly.

```bash
D=/tmp/p34c-demo-mem; rm -rf "$D"; mkdir -p "$D"
printf '# Memory Index\n' > "$D/MEMORY.md"
printf 'user: export OPENAI_API_KEY=sk-test-12345 then run the deploy\nuser: also my pref is tokio for async\n' > "$D/transcript.txt"
AICHAT_MEMORY_DIR=$D AICHAT_MEMORY_REFLECT_ECHO=1 ./target/debug/aichat --memory-reflect --memory-transcript "$D/transcript.txt"
```

```output
{
  "candidates": [
    {
      "topic": "user_export_openai",
      "body": "user: export OPENAI_API_KEY=[REDACTED:generic_secret] then run the deploy\nuser: also my pref is tokio for async\n",
      "turns_referenced": []
    }
  ]
}
```

The live path (no echo env) routes the redacted transcript through a role whose name ends `-memory-reflector`, which returns the same candidate-set schema. At REPL exit the loop is opt-in via `--memory-reflect-on-exit` (or `AICHAT_MEMORY_REFLECT_ON_EXIT=1`).

---

## 34D — Curator gate

The Curator gates every candidate (`[a]ccept [s]kip [e]dit [r]eject-all`). `--memory-auto-curate` accepts all without prompting — for non-interactive runs. Accept writes the topic file atomically with stamped frontmatter and appends an index line to `MEMORY.md`.

```bash
D=/tmp/p34d-demo-mem; rm -rf "$D"; mkdir -p "$D"
printf '# Memory Index\n' > "$D/MEMORY.md"
echo '{"candidates":[{"topic":"rust_async","body":"Prefer tokio::spawn for new async code.","turns_referenced":[3]}]}' > "$D/cands.json"
AICHAT_MEMORY_DIR=$D ./target/debug/aichat --memory-curate --memory-candidates "$D/cands.json" --memory-auto-curate
echo '--- MEMORY.md ---'; cat "$D/MEMORY.md"
echo '--- rust_async.md ---'; cat "$D/rust_async.md"
```

```output
memory: wrote /tmp/p34d-demo-mem/rust_async.md
memory: curation complete — 1 file(s) written.
--- MEMORY.md ---
# Memory Index
- [rust_async](rust_async.md) — Prefer tokio::spawn for new async code.
--- rust_async.md ---
---
created: 2026-05-31T04:18:19.731088+00:00
curator: auto
turns_referenced: [3]
---

Prefer tokio::spawn for new async code.
```

The loop closes: a candidate the Curator accepted is immediately lazy-loadable by its topic reference (34B).

```bash
AICHAT_MEMORY_DIR=$D ./target/debug/aichat --memory-load rust_async
```

```output
---
created: 2026-05-31T04:18:19.731088+00:00
curator: auto
turns_referenced: [3]
---

Prefer tokio::spawn for new async code.
```
