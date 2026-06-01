# SPEC-004: Cache-Substrate Ecosystem Surfaces

**Version:** 0.1
**Status:** Draft, paired with [`SPEC-003`](SPEC-003-cache-substrate.md)
**Owners:** project lead
**Inputs:** [`ADR-0005`](ADR-0005-cache-substrate-extraction.md), [`SPEC-003`](SPEC-003-cache-substrate.md),
[`SPEC-001`](SPEC-001-trace-format.md), [`SPEC-002`](SPEC-002-test-harness.md),
[`EVAL-0003`](EVAL-0003-tool-call-caching.md), [`EVAL-0004`](EVAL-0004-litellm-cache-parity.md),
root `CLAUDE.md`, `brief` repo `CLAUDE.md`/`README.md`

This spec defines the **API surfaces** every ecosystem component exposes to (or inherits
from) the substrate of [`SPEC-003`](SPEC-003-cache-substrate.md). For each component the
verdict is given; where a component needs **no change**, that is stated and is a valid
result. Companion changes to other repos (`brief`, `llm-functions`, `argc`) are
**documented here, not applied** — this is a docs-only PR.

## Boundary restatement `TODO(ACE-id)`

Every surface below sits relative to the same line: the substrate owns **only** the
wire-level response cache, keyed on the **canonicalized request body**. The structure-aware
`StageCache` `(role, model, input)` key and provider prompt caching (L3) **stay in
Eridian** and are **not** routed through the substrate (`ADR-0005` §1, `SPEC-003` §0). No
surface here may push a structure-aware or knowledge key across that line.

## Eridian (`aichat` fork) — primary surface

**Verdict: changes required, all additive.** Eridian is both the substrate's upstream
client and its richest controller.

### Targeting the substrate

Two routes, non-exclusive (the OpenAI-compat seam, `ADR-0005` §3):

1. **Point `base_url` at the substrate** (preferred). Eridian already speaks
   OpenAI-compatible HTTP; setting the provider `base_url`/`api_base` to the
   `eridian-replay` gateway makes every call flow through it. This is the deployment used
   for promptfoo replay and cross-process mock.
2. **A `CacheBackend::Remote` variant** behind the Phase 38A trait, so "in-process vs
   separate process" is a deployment choice over **one trait** — the in-process
   `DualBackend` (38C) and a remote substrate present the same `CacheBackend` interface to
   `call_chat_completions`/`serve.rs`.

### CLI / flag surface (the three modes)

Mirrors the `SPEC-003` §4 control protocol; extends the 38D `--cache-*` family:

| Flag | Meaning |
|---|---|
| `--cache-mode cache\|cassette\|mock` | select policy |
| `--cassette <name>` | select/record a cassette set |
| `--cache-record` / `--cache-replay` | cassette sub-mode (record grows the set; replay hard-fails on miss) |
| `--cache-ttl`, `--cache-namespace` | 38D controls |
| `--no-transparent-cache` (= `no-cache`), `--cache-no-store` | 38D read/write skips |

Role frontmatter mirror (`SPEC-003` §4): `cache: { ttl, namespace, mode, cassette }`.

### Correlation + determinism headers Eridian injects `TODO(ACE-id)`

On every outbound request to the substrate, Eridian injects:

- `X-Eridian-Session-Id: <turn ULID>` — keystone-trace correlation (`SPEC-003` §6);
- `X-Aichat-Cacheable: 0|1` — determinism signal derived from `temperature`/`seed`/`top_p`
  (`SPEC-003` §2 gate).

These are cheap and correct because Eridian owns the client — the exact objection
`EVAL-001` §3 raised applied only to a third-party MITM.

### What stays in-tree (NOT routed through the substrate) `TODO(ACE-id)`

- **L3 provider prompt caching (37B)** — `cache_control` emission + prefix-stability
  discipline in the request builder. A byte-ordering property *above* the wire; the
  substrate never sees the decision.
- **`StageCache` (`stages` prefix)** — pipeline-stage and `.cache/knowledge` memoization,
  keyed on `(role, model, input)`. Confirmed unchanged; its two callers (`src/pipe.rs`,
  `src/knowledge/compile.rs`) keep working. `StageCache` *shares* `replay-core`'s CAS
  primitive (`SPEC-003` §1) but not keying.

### REPL + batch parity (root `CLAUDE.md` soft constraint)

Both surfaces get the modes: batch via the flags above; REPL/pi via the bridge slash
commands (§pi). The 37D `/cache-stats`, `/cache-clear`, `/transparent-cache on|off`
commands extend with cassette/mock selection.

## `brief` — declarative fixtures field, deferred & optional `TODO(ACE-id)`

**Verdict: spec a minimal, optional, deferred emit target; apply nothing now.** `brief` is
**format-first, synchronous, no `tokio`, no `reqwest`** — it declares and emits, it does
not run (hard constraint, `brief` `CLAUDE.md` decisions 2–4). It must **never** gain
runtime or network code.

The plausible surface is the **F6 pattern**: brief grows a *format field*, Eridian/the
harness grow the *runtime consumer*, brief never runs anything. An intent author declares,
alongside the existing constraint/deliverable sections, that a role's eval replays a named
cassette set:

```markdown
## Fixtures
- cassette: rust-reviewer-v1 — pinned eval-replay set for the rust-reviewer role
```

or, in frontmatter (brief frontmatter is extensible; `README` allows added fields):

```yaml
cassettes: [rust-reviewer-v1]
```

`brief emit` would compile this into the eval harness's replay config (a promptfoo
`providerconfig` snippet selecting `--mode cassette --cassette rust-reviewer-v1`). brief
itself only **parses and emits a string**; the cassette is recorded/replayed by
`eridian-replay`, invoked by the harness.

**Honest assessment:** for the committed eval-replay story, brief does **not** need this —
the cassette set can be named directly in `promptfooconfig.yaml` (§promptfoo) without any
brief involvement. The `## Fixtures` field is a *convenience* for teams that already author
intent in `.brief.md` and want the cassette reference to live next to the role's intent. It
is therefore specced as **deferred and optional**, and if it is never built, brief stays
untouched. **Companion change (documented, not applied):** adding `## Fixtures` parsing +
a `cassettes:` frontmatter field + emit support is a `brief`-repo task for Jordan to apply
in `../brief` ([github.com/jikanter/brief](https://github.com/jikanter/brief)), never
edited from this PR.

## `llm-functions` / tool-execution boundary — no change to `llm-functions` `TODO(ACE-id)`

**Verdict: `llm-functions` is unchanged; the replay seam is Eridian-side.**

Tool calls are **local subprocesses with no HTTP** (`SPEC-001` `tool.executed`;
`EVAL-001` §2; `EVAL-0003` §1 treats a tool-using turn as an atomic uncacheable black box).
The wire substrate therefore **neither sees nor caches** tool execution. But a cassette
that replays LLM responses while tools run **live and side-effecting** is not deterministic
— the eval would diverge on the tool layer.

**Resolution:** tool-execution replay is an **Eridian-side concern served from the
keystone-trace blob store** — the recorded `tool.executed` stdout (`SPEC-001` §3.4,
content-addressed in `blobs/`) — **not** from the wire substrate. This keeps the substrate
purely wire-level and reuses the trace as the record:

- **Record run:** the keystone trace already captures `tool.executed` stdout/exit-status
  (hashed into the blob store).
- **Replay run:** Eridian's **tool-dispatch layer gains a replay mode** that, instead of
  spawning the subprocess, returns the recorded stdout keyed by `(tool_name, args_hash)`
  from the trace blob store.

So a deterministic eval = wire substrate (LLM responses, cassette) **+** Eridian tool-
replay (tool stdout, trace blobs). Two record stores, two seams, one deterministic run.

`llm-functions` itself stays unchanged (root `CLAUDE.md` *Ask First*: reduced compat with
llm-functions). A **per-tool purity/cacheability declaration** (e.g., "this tool is pure,
its result may be wire-cached") is a **future note, flagged not specced** — it would be a
companion change in
[github.com/jikanter/personal-llm-functions](https://github.com/jikanter/personal-llm-functions),
not applied here.

> **Stop-and-ask flagged (kickoff §6):** the cassette-vs-trace-blob relationship for tool
> replay assumes `tool.executed` records stdout keyed reproducibly by `args_hash`. `SPEC-001`
> §3.4 stores `args_hash` and `stdout_hash` but does not state that `(tool_name, args_hash)`
> is a stable *lookup* key across runs. If that is not guaranteed, the tool-replay key needs
> a schema decision — surfaced here rather than guessed.

## `argc` / `Argcfile.sh` — no contract change; dev tasks only `TODO(ACE-id)`

**Verdict: no change to the `argc` contract; `Argcfile.sh` gains dev tasks.**

`eridian-replay` uses Rust + `clap` (consistent with `aichat` and `brief`), **not** `argc`
— `argc` is for the bash tool layer that `llm-functions` depends on. Touching the `argc`
contract is an *Ask First* item (root `CLAUDE.md`); we do not.

`Argcfile.sh` gains **build/test/demo** tasks for the substrate (e.g.,
`build-replay`, `test-replay`, `demo-replay`), which orchestrate `cargo` and `showboat`.
These are dev-ergonomics wrappers, not contract changes — they add commands, they do not
alter any existing `argc`-defined tool signature.

## `pi` / `pi-extensions` — downstream consumer, inherits for free `TODO(ACE-id)`

**Verdict: light surface; pi inherits caching/replay by topology.**

Per 37D's design note, any tool pointed at the gateway inherits cache/replay for free. The
pi bridge already runs against an in-process `aichat --serve` (`src/repl/pi.rs`); when that
server holds a `ResponseCache`/substrate, pi turns are cached transparently.

Mode selection for pi: via **env or header** at launch — `AICHAT_CACHE_MODE` /
`AICHAT_CASSETTE` env vars consumed by the launcher, or the bridge passing
`x-aichat-cache-mode`/`x-aichat-cassette` headers (`SPEC-003` §4) on its `/v1/state/*`-style
calls. The existing slash-command pattern (37D's `/cache-stats`, `/cache-clear`,
`/transparent-cache on|off`, routed through `bridgeFetch`) extends with
`/cache-mode <mode>` and `/cassette <name>`. No new pi-side architecture.

## `promptfoo` — the eval-replay payload `TODO(ACE-id)`

**Verdict: the concrete realization of the committed eval-replay story.**

promptfoo's provider points at the substrate in **replay** mode against a **committed
cassette set**, giving deterministic, token-free regression/eval. Wiring (extends
`SPEC-002` §2 `exec` provider):

```yaml
# tests/regression/promptfooconfig.yaml
providers:
  - id: aichat-replay
    config:
      command: |
        aichat \
          --role {{role}} \
          --cache-mode cassette --cassette {{cassette}} --cache-replay \
          --trace-out /tmp/eridian-test-$$.jsonl \
          {{prompt}}
```

- A CI run sets `--cache-replay` (hard-fail on miss): zero tokens, no provider, fully
  deterministic. Drift (`SPEC-003` §7) surfaces as a replay miss naming the stale key — the
  prompt changed but the recording did not.
- Cassettes pin the **simulated-user turns** of the multi-turn eval work: each turn's
  request is a canonical key, so a scripted multi-turn conversation replays end-to-end.
- The existing `helpers/trace.js` (`SPEC-002` §2) reads `cache_hit: true` from the trace to
  assert the eval ran from cassette, not from a live call.

Recording the set is a separate, occasional job (`--cache-record` against a live provider),
reviewed and committed per `SPEC-003` §7.

## `wiremock` — coexists; the substrate does NOT replace it `TODO(ACE-id)`

**Verdict: keep `wiremock-rs`; it and the substrate's mock leg occupy different niches.**

- **`wiremock-rs`** runs **in-process** and is compatible with `tokio::time::pause()`
  (`SPEC-002` §3, `EVAL-001` §4.4). It is for **time-mockable Rust control-flow tests** —
  retry backoff, fallback sequencing, stream-interrupt — where the test asserts on aichat's
  *internal* event sequence and must compress 30-second backoffs to zero wall-clock.
- **The substrate's mock leg** is for **cross-process, end-to-end** fault injection: a real
  downstream tool (pi, Claude Code, promptfoo) points at the substrate's `base_url` and
  gets scripted faults. It runs in a real process on real wall-clock; it **cannot**
  time-mock.

The boundary is explicit so no one tries to merge them: in-process + time-control →
`wiremock-rs`; cross-process + real binary under test → substrate mock. (`SPEC-003` §3,
§8.)

## Showboat / Rodney / Inspect `TODO(ACE-id)`

- **Showboat** — every *implementation* PR ships a `demos/<milestone>.md` via `uvx
  showboat` with ≥3 `showboat exec` blocks (happy/error/new-flags), evergreen output so
  `showboat validate` works (root `CLAUDE.md` hard constraint). Required by
  [`PLAN-cache-substrate.md`](PLAN-cache-substrate.md); **this docs PR ships none.**
- **Rodney** — only if a substrate `--serve` admin/playground UI is specced (it is not, in
  v0.1). No surface now.
- **Inspect AI** — a **deferred** cassette consumer: Inspect runs benchmarks against
  Eridian pointed at a committed cassette set in replay mode, same wiring as promptfoo.
  Noted, not built for (consistent with `ECOSYSTEM.md` deferring Inspect).

## Surface summary

| Component | Verdict | Change applied here? |
|---|---|---|
| Eridian (`aichat`) | additive: `base_url`/`CacheBackend::Remote`, `--cache-mode`/`--cassette`, correlation headers; L3 + `StageCache` stay in-tree | no (docs only) |
| `brief` | optional, deferred `## Fixtures`/`cassettes:` emit target; format-only | no — companion change documented for `../brief` |
| `llm-functions` | unchanged; tool-replay is Eridian-side from trace blobs | no |
| `argc` / `Argcfile.sh` | no contract change; dev tasks only | no (docs only) |
| `pi` / `pi-extensions` | inherits for free; env/header mode select + 2 slash commands | no (docs only) |
| `promptfoo` | replay provider against committed cassette set | no (docs only) |
| `wiremock-rs` | coexists; not replaced | no |
| Showboat / Rodney / Inspect | demos per impl PR; Rodney n/a; Inspect deferred | no |
