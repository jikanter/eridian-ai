# Phase 54: CLI UX Hardening — Overview - Epic 12 (Developer Experience)

The batch CLI has strong primitives (`-o` single output axis, `--dry-run`, `--explain-context`,
semantic exit codes) but a weak **shape**: ~90 flags live in one flat namespace with no grouping,
no man page, and several CLI/GNU table-stakes affordances missing. This phase brings the surface up
to [clig.dev](https://clig.dev) / GNU conventions **without** adding output formats or breaking the
existing flag contract (`aichat -r summarize < in.txt` and all `bin/` wrappers keep working — see
[`architecture.md` "What to Preserve"](../architecture/architecture.md)).

**Scope discipline.** Batch CLI only. The legacy Reedline REPL is frozen (pi owns interactive — see
[`features/repl-pi.md`](../features/repl-pi.md)); no REPL UX investment here. Shell completions are
**out of scope** (handled externally via oh-my-zsh). No new `-o` formats, no `--agent-mode` (both
killed — see [`anti-roadmap.md`](anti-roadmap.md)).

**Discipline (CLAUDE.md hard rules).** TDD for every item: failing test first → implement →
showboat demo. bats integration tests **alongside** unit tests for each sub-phase. Zero new default
dependencies expected — `clap_mangen` is the only candidate add and is build-time only; confirm
before pulling it in.

| Item | Description | Compat | Status |
|---|---|---|---|
| 54A | Grouped `--help` (clap `help_heading`) + generated man page (`clap_mangen`) | additive | **Done** |
| 54B | Standard flags: `--color=auto\|always\|never`, `-q/--quiet`, global `--verbose` | additive | **Done** |
| 54C | Non-interactive safety: `--no-input` guard + destructive-op confirm + `--yes` | additive | **Done** |
| 54D | "Did you mean?" suggestions for unknown role / model / session / agent | additive | **Done** |
| 54E | `--config-path` / `--config-get KEY` introspection (flag form) | additive | **Done** |
| 54F | Noun-verb subcommand layer; existing flags become hidden deprecated aliases | **Ask-First** | -- |
| 54G | Cross-surface syntax map doc (CLI `--` / legacy REPL `.` / pi `/`) | doc only | -- |

**Sequencing.** 54A–54E are independent, low-risk, shippable in any order (54A first = highest
discoverability ROI). 54F is gated on explicit owner approval (backward-compat = Ask-First) and
should land **after** 54A so the grouped-help machinery is reused by the subcommand surface. 54G is
documentation, do last so it reflects the shipped surface.

---

## 54A — Grouped help + man page

**What.** Today `aichat --help` prints ~90 flags as one undifferentiated wall. Annotate each clap
arg with `#[arg(help_heading = "...")]` into ~14 sections (Core, Input, Output, Execution,
Discovery, Roles, Knowledge, Memory, RAG, MCP, Server, Session, Setup). Generate `aichat.1` from the
existing clap definitions via `clap_mangen`, exposed as a hidden `--man` flag
(`aichat --man > man/aichat.1`) so packaging regenerates it from the live flags — no tracked,
drift-prone roff artifact, no version-baking from debug builds.

**Acceptance.**
- `aichat --help` shows the grouped section headers; every existing flag appears under exactly one
  heading; no flag dropped or renamed.
- `man/aichat.1` builds from clap defs (no hand-maintained duplicate) and renders with `man`.

**Test.** bats: `aichat --help | grep -q '<Heading>'` for each section; assert a representative flag
under each. Snapshot the heading set so a new ungrouped flag fails CI. Unit: man generation produces
non-empty roff for a known flag.

**Showboat.** Evergreen demo: side-by-side flat-vs-grouped help excerpt; `man aichat` open to the
Knowledge section. Deterministic (help text is static) → safe for `showboat validate`.

---

## 54B — Standard flags

**What.** Three GNU/clig.dev table-stakes flags currently missing:
- `--color=auto|always|never` (default `auto`). Layer over existing `NO_COLOR` + TTY detection —
  explicit flag overrides env, enables color through a pager (`| less -R`). Precedence:
  `--color` > `NO_COLOR` > TTY auto.
- `-q/--quiet` — suppress spinner, `--cost` line, and non-essential stderr; primary stdout
  unaffected.
- Global `--verbose` — promote verbosity from role-scoped to a global log level (logs → stderr).
  Keep the existing role `--verbose` behavior as a subset.

**Acceptance.** `--color=never` strips ANSI even on a TTY; `--color=always` keeps ANSI when piped.
`-q` removes spinner/cost; exit code and stdout payload byte-identical to non-quiet run. `--verbose`
emits diagnostics to stderr only, never stdout.

**Test.** bats: pipe `--color=always` into `cat -v`, assert escape codes present; `--color=never`
on forced-TTY, assert absent. `-q` run vs normal run → assert stdout identical, stderr shrinks.
Unit: color-decision function truth table (flag × NO_COLOR × is_tty).

**Shipped.** `--color` unified behind `no_color()`/`decide_no_color`; `-q` behind
`spinner_suppressed`/`should_show_cost`; `--verbose` overloaded to force `effective_log_level` →
Debug (overrides `AICHAT_LOG_LEVEL`) and route to stderr, keeping its legacy role-list detail.
Behavioral suppression (spinner/cost) and `--verbose` stderr emission are runtime/TTY-bound and
covered by unit truth tables; bats pins the deterministic CLI surface.

**Known follow-up.** `--verbose` log lines can be dropped on `process::exit` (buffered logger
writes not flushed on the fast-exit paths). Tracked for a later flush-on-exit fix; does not affect
the level-resolution logic.

**Showboat.** Demo the color truth table and a quiet-mode diff. Deterministic.

---

## 54C — Non-interactive safety

**What.** Interactive blocking points (agent variable prompts, `--execute` confirm, REPL `.delete`)
currently hang or mis-behave when no TTY (CI, pipelines). Add:
- `--no-input` — when input would be required and stdin is not a TTY, **fail loud** with a clear
  message + usage exit code, never hang. Auto-enabled when stdin is non-TTY (flag forces it on).
- Destructive-op confirmation for delete paths (role/session/RAG/agent), modeled on the existing
  `--execute` confirm flow. Bypass with `--yes` (alias `--force`) for scripts.

**Acceptance.** Delete without `--yes` on a TTY prompts; with `--yes` proceeds silently. Any op
needing input under non-TTV stdin exits with the usage code and a one-line "needs input, none on
stdin" message — no hang. SECURITY.md alignment: no destructive action without confirm-or-`--yes`.

**Test.** bats: `printf '' | aichat <op-needing-input>` (no TTY) → asserts non-zero usage exit + no
hang (bounded timeout, poll output file — no `|| true` masking). Delete-with-`--yes` removes target;
delete-without on non-TTY refuses. Unit: TTY/`--no-input`/`--yes` decision matrix.

**Showboat.** Demo the refuse-on-no-TTY path and the `--yes` bypass. Deterministic (no API call).

**Shipped.** Primitive in place: `--no-input`, `--yes` (alias `--force`), an `IS_STDIN_TERMINAL`
static, and pure `can_prompt` / `resolve_confirm` (`Proceed`/`Prompt`/`Refuse`). Wired into the
deterministic destructive batch op `--migrate-sessions` (removes legacy `.yaml`): confirms when
interactive, refuses with the usage exit code (2) under non-TTY/`--no-input`, bypassed by `--yes`.
Other `inquire` prompt sites (role/macro creation, agent variables) already error rather than hang
on a non-TTY; routing them through `resolve_confirm` for uniform messaging is a follow-on.

---

## 54D — "Did you mean?" suggestions

**What.** Unknown `-r`/`-m`/`-s`/`-a` value → Levenshtein-nearest candidate from the relevant list.
`aichat -r summarise` → `error: unknown role 'summarise'. Did you mean 'summarize'?` Reuse existing
list-enumeration code (`--list-roles` etc.) as the candidate source.

**Acceptance.** Single nearest match within edit-distance threshold is suggested; below threshold,
no noisy guess. Suggestion to stderr; exit code unchanged (still a usage error). Works for role,
model, session, agent.

**Test.** bats: known-typo input asserts the "Did you mean" line names the right candidate;
far-off input asserts no suggestion line. Unit: distance/threshold function over a fixed candidate
set.

**Showboat.** Demo typo → suggestion for each of the four kinds. Deterministic.

**Shipped.** Pure `nearest_match` (Levenshtein, threshold `max(2, len/3)`) + `did_you_mean` helper,
wired into the unknown-role (`Role::resolve`), unknown-agent (`Agent::init`), and unknown-model
(`Model::retrieve_model`) errors. Session is N/A by design — `-s NAME` creates a session on demand
rather than erroring, so there is no unknown-session to suggest against.

---

## 54E — Config introspection

**What.** Config is only reachable via `.edit config` (REPL) or manual file open — a batch-only tool
should expose it batch-style. Add `aichat config path` (print resolved config dir + file, honoring
`AICHAT_CONFIG_DIR`/`XDG_CONFIG_HOME` precedence) and `aichat config get KEY` (print one resolved
value, `-o json` aware). Read-only; no `config set` (file edit stays the authoring path).

**Acceptance.** `config path` prints the same dir `--info` resolves to. `config get model` matches
the active model. `-o json` emits a structured object. Unknown key → usage error (+ 54D suggestion
where cheap).

**Test.** bats: set `AICHAT_CONFIG_DIR` to a temp dir, assert `config path` echoes it; `config get`
a seeded key returns the seeded value; `-o json` parses. Unit: key resolver against a fixture config.

**Showboat.** Demo `config path` + `config get` (text and json) against a temp config. Deterministic.

**Shipped.** Delivered as **flags** `--config-path` / `--config-get KEY` (not a `config`
subcommand — that is 54F's Ask-First surface; a subcommand would also clash with the trailing-text
positional). `--config-path` is a pure static early-exit (no init, no model). `--config-get` reuses
`Config::sysinfo_items()` (the exact `--info` key/value set), takes the light `info_flag` init (no
client/network/model), is `-o json` aware, and suggests via [`did_you_mean`](#54d) on unknown keys.

**Testing note.** All Phase 54 bats are self-contained / CI-safe — isolated `AICHAT_CONFIG_DIR`
and light-init paths, no dependency on a running model or ambient config. See
[`feedback_ci_safe_tests`].

---

## 54F — Noun-verb subcommand layer *(Ask-First gate)*

**What.** The flag namespace hides ~40 verbs as flags — 11 `--knowledge-*`, 7 `--memory-*`, 6
`--list-*`, role/session/mcp clusters. Introduce an **additive** noun-verb surface; existing flags
become **hidden deprecated aliases** dispatching to the same code (no removal, no break):

```
aichat knowledge compile|search|list|stat|show|reflect|curate
aichat memory    reflect|curate|load
aichat role      list|fork|explain|find
aichat session   list|convert|migrate|empty
aichat mcp       serve|call|list-tools|validate
aichat list      models|roles|agents|sessions|macros|rags
```

**Compat (the gate).** `aichat -r <role> < input`, `--prompt`, `-m`, `-o`, `--each`, and every
`bin/` wrapper MUST behave identically. Old flags keep working (hidden in `--help`, emit a one-line
deprecation note to stderr only under `--verbose`). **This sub-phase requires explicit owner
approval before any code — backward-compat changes are Ask-First per CLAUDE.md.**

**Acceptance.** Each new subcommand produces byte-identical stdout to its legacy flag for a fixed
input. Legacy flags still resolve. `aichat --help` shows subcommands, not the 40 hidden flags.
Tab-completion (oh-my-zsh, external) sees subcommands.

**Test.** bats: matrix asserting `aichat knowledge search Q` == `aichat --knowledge-search Q` (and
each pair) on identical stdout/exit. Regression: a legacy invocation from a `bin/` wrapper still
passes. Unit: alias→canonical dispatch table is total (every legacy flag maps).

**Showboat.** Demo old-flag and new-subcommand producing the same output for knowledge + list.
Deterministic (offline roles / `--dry-run`).

---

## 54G — Cross-surface syntax map

**What.** Three command syntaxes coexist by design: batch CLI `--flag`, legacy REPL `.command`, pi
`/command`. Users crossing surfaces get lost. One doc in `docs/features/` mapping equivalent
operations across all three (e.g. `--role` ↔ `.role` ↔ `/role`), stating which surface owns what and
why (pi owns interactive; aichat owns batch).

**Acceptance.** Table covers every operation available on ≥2 surfaces; links from
[`features/repl-pi.md`](../features/repl-pi.md) and the CLI help footer. No behavior change.

**Test.** bats/doc-lint: assert the doc references each legacy REPL command and its CLI counterpart
exists (catch drift when a flag is renamed).

**Showboat.** Render the cross-surface table as an evergreen note.

---

## Out of scope (explicit)

- Shell completions (bash/zsh/fish) — handled externally (oh-my-zsh).
- New `-o` output formats, `--agent-mode`, `--machine-readable` — killed; `-o compact` covers.
- Legacy Reedline REPL UX — frozen; pi owns interactive.
- `config set` (mutating config from CLI) — file edit remains the authoring path.
