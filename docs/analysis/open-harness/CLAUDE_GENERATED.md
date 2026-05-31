# Eridian — Claude Code Context

Eridian is a fork of [aichat](https://github.com/sigoden/aichat). This document
orients coding agents to the **trace emission** workstream, which is the foundation
for Eridian's testing, evaluation, and training data extraction strategy.

## Reading order

If you are new to this workstream, read these in order:

1. `docs/architecture/ECOSYSTEM.md` — the future-state map of how Eridian, its tests,
   evals, and training pipelines fit together. Read first; gives you the *why*.
2. `docs/architecture/ADR-0001-trace-as-keystone.md` — why the trace format is the
   load-bearing decision and why we are not adopting an existing tool's format as
   the source of truth.
3. `docs/architecture/ADR-0002-streaming-safe.md` — why traces must be streaming-safe
   and the engineering discipline that requires.
4. `docs/architecture/ADR-0003-async-writer-thread.md` — why trace writes happen on
   a dedicated OS thread and the Rust concurrency primitives used.
5. `docs/specs/SPEC-001-trace-format.md` — **the** spec. Schema, event types, file
   layout, redaction, versioning. This is the contract.
6. `docs/specs/SPEC-002-test-harness.md` — how promptfoo (regression) and a custom
   Rust harness with wiremock (control-flow) consume the trace format.
7. `docs/implementation/PLAN-trace-emission.md` — phased plan for the trace
   emission work. Start here when implementing.
8. `docs/implementation/PLAN-test-harness.md` — phased plan for the test harness,
   sequenced after the trace work lands.

## Working principles

- **The schema is the contract.** Everything downstream — promptfoo assertions,
  control-flow tests, Inspect AI evals, training data extraction — depends on
  `SPEC-001` being stable. Schema changes are deliberate and bump
  `schema_version`.
- **Streaming-safe is non-negotiable.** Every event is self-contained, atomically
  written, and survives `tail -f`. See `ADR-0002`.
- **Trace emission is on the hot path of every aichat invocation.** It must not
  block user-visible work and must not panic on disk pressure. See `ADR-0003`.
- **Default to defaults.** Tracing is on by default. Redaction defaults are sane.
  Output paths follow XDG conventions. Users get value with zero config.
- **Don't over-build.** Specs call out explicit Phase 1 vs deferred work. The DPO
  pipeline, Inspect integration, OpenTelemetry projection, and marimo trace
  explorer are not Phase 1.

## Supervision workflow — Showboat

Every implementation milestone ships a [Showboat](https://github.com/simonw/showboat)
demo as part of its acceptance criteria. This is non-negotiable: passing tests are
necessary but not sufficient — we want a human-readable Markdown artifact that
exercises the new feature with real commands, real outputs, and (where relevant)
screenshots.

### Why this discipline

Tests prove correctness. Showboat demos prove *the thing actually exists in the
shape we asked for*. Reading test code is hard; reading a `demo.md` that runs the
feature end-to-end is easy. This is especially valuable when the implementer is a
coding agent: it converts "the agent says it's done" into "here is the agent
demonstrating it, with command outputs you can verify."

### Standard agent invocation

When Claude Code is asked to implement a milestone, the prompt should end with
something like:

> Once tests pass, use Showboat to create a `demos/<milestone-name>.md` that
> exercises the new functionality. Run `uvx showboat --help` first to see the
> commands. Include at least three `showboat exec` invocations covering the happy
> path, an error path, and any new CLI flags.

For milestones touching the web UI (e.g., `aichat --serve` playground),
[Rodney](https://github.com/simonw/rodney) is the browser automation companion:

> Use Rodney inside Showboat exec blocks to capture screenshots and DOM state from
> the playground. `uvx rodney --help` for commands.

### Catching agent cheating

Showboat's biggest failure mode is the agent editing the Markdown file directly
instead of using the CLI. Reviewers should spot-check by running
`showboat extract demos/<milestone-name>.md` and comparing the recovered command
sequence to the file contents. If the agent invented outputs, the extracted
commands will not regenerate the file when re-run.

## What "done" looks like for Phase 1 (trace emission)

- `aichat` emits a JSONL trace per turn that conforms to `SPEC-001` v0.1.
- A blob store at `~/.local/state/aichat/traces/blobs/` holds payloads
  content-addressed by SHA-256.
- A bounded MPSC channel with a dedicated OS thread handles writes; aichat's
  request path never blocks on disk.
- `--trace-out`, `--no-trace`, and `AICHAT_TRACE*` env vars are wired.
- A small redaction layer with a default rule set runs before events hit disk.
- `aichat trace show <session_id>` resolves blobs for human inspection.
- `demos/phase-1-trace-emission.md` exists, generated via Showboat, exercising
  every CLI flag and showing real trace output.

Phase 2 (test harness) and beyond are sequenced once Phase 1 lands. See
`PLAN-test-harness.md`.

## Out of scope for now

Training pipelines, Inspect AI integration, the marimo explorer, DPO pair
generation, OpenTelemetry projection, web UI test coverage via Rodney. All of
these are real future work but not gating Phase 1.

## When stuck

If a design question is not answered by the specs or ADRs, **stop and ask**. The
schema is too load-bearing for guesswork. Open an issue or surface the question
in your PR description rather than inventing.
