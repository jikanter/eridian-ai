# ADR-0001: Trace format is the keystone artifact

**Status:** Accepted, 2026-04-25
**Decider:** project lead

## Context

Eridian needs a testing story, an evaluation story, and (eventually) a
training-data extraction story. Each of these has an obvious tool that already
exists:

- promptfoo for assertion-based regression testing
- Inspect AI for formal evals
- HuggingFace `trl` / axolotl / a custom pipeline for training data

A naive approach is to pick one of these tools and use its native data model
as Eridian's source of truth — for example, write everything as Inspect AI
`Task` definitions, or store all runs in promptfoo's JSON output format.

This is wrong. Each tool's data model is shaped by its purpose:

- promptfoo's output format is shaped around prompt/output pairs and
  assertion results. It has no first-class concept of "retry attempt" or
  "fallback provider invocation."
- Inspect's log format is rich but Python-centric, structured around
  `Solver` / `Scorer` semantics that don't match aichat's runtime.
- Training datasets are shaped for a specific trainer's input format and
  drop most metadata.

If Eridian commits to any one of these as canonical, we either lose
information that other consumers need or we layer extensions onto a foreign
schema and end up with a Frankenstein.

## Decision

**Eridian emits a structured event log in a native, aichat-shaped JSONL
format.** This is the source of truth. All downstream consumers — promptfoo
assertions, Inspect AI evals, training data extraction, observability
projections — read this format. Nothing else writes it.

The format is specified in `SPEC-001-trace-format.md`. It is versioned via
`schema_version` in every event, and breaking changes bump the version
explicitly.

## Consequences

### Positive

- **No tool lock-in.** Swapping promptfoo for a different test runner is a
  consumer-side rewrite, not a data-migration project.
- **Faithful representation of aichat internals.** Retry attempts, fallback
  decisions, RAG retrieval scores, tool-whitelist denials all become
  first-class events with the granularity needed to assert on them.
- **One instrumentation effort.** Aichat's codebase gets traced once, not
  three times for three different consumer formats.
- **Future consumers get the data for free.** When (not if) we want to feed
  traces into an observability platform, generate DPO pairs, or build a
  marimo explorer, the data is already there in a format we control.

### Negative

- **Schema design is real upfront work.** Getting the events right matters
  because changing them later is a breaking-change migration. We accept this
  cost; see `SPEC-001` for the v0.1 design and the explicit list of "open
  questions to answer before v1.0."
- **Each consumer needs an adapter.** Promptfoo can't read our format
  natively; we write a small JS helper. Inspect can't read it either; we
  write a small Python adapter when the time comes. This is a real
  cost — it's just a smaller cost than writing into someone else's format and
  perpetually fighting it.
- **Storage cost.** A blob store of full prompts and tool outputs accumulates.
  We accept this; deduplication via content-addressing keeps it manageable,
  and disk is cheap relative to the value of the corpus.

### Risks accepted

- **The schema may be wrong in ways we don't see yet.** Mitigation: ship v0.1
  fast, get real consumers (promptfoo regression tests, control-flow tests)
  reading from it within the same quarter, let them tell us what's missing.
  Bump to v0.2 deliberately.
- **The "downstream consumers will materialize" framing is partially
  speculative.** Today there is one confirmed consumer (the test harness)
  and several speculative ones (training, eval, observability). If only the
  test harness ever materializes, we have over-engineered relative to using
  promptfoo's native format. Mitigation: the format itself is designed to be
  cheap if only one consumer reads it — JSONL is a universal format, the
  blob store is just files, and the writer is a few hundred lines of Rust.

## Considered alternatives

### Alt 1: Use promptfoo's output format as canonical

Rejected because promptfoo treats every test target as a black box. There's
no place in its schema for retry attempts, fallback transitions, or
intra-call events. Adopting it as canonical would force us to either lose
those events or bolt on extensions that promptfoo's UI and tooling don't
understand.

### Alt 2: Use Inspect AI's log format as canonical

Rejected because Inspect's format is Python-centric and structured around
its `Sample` / `Solver` / `Scorer` model. Eridian is Rust, aichat invocations
don't naturally split into solver/scorer phases, and committing to Inspect's
format would couple our event schema to Python data classes we don't control.
Inspect remains a useful *consumer* (deferred to Phase 3) but not the schema
authority.

### Alt 3: Use OpenTelemetry GenAI semantic conventions as canonical

Rejected because OTel's GenAI conventions are aimed at observability of
production systems, not at testing internal control flow. They lack
first-class concepts for what we need to assert on (retry attempts as
events, fixture-injected failures, role/tool-whitelist application). We'd
end up extending heavily and fighting their conventions. Better to design
an aichat-native schema and emit OTel as a *projection* if observability
tooling ever requires it.

### Alt 4: No structured trace; tests rely on stdout/stderr scraping

Rejected because the retry layer (one of our explicit testing requirements)
emits no observable signal in stdout under normal operation. Asserting "did
retry fire?" requires either parsing internal log lines (fragile, format
churns under refactor) or having a structured event for it. Once we accept
we need structured events, we may as well design the format deliberately.

## Sources and prior art

- Hamel Husain on Inspect AI's design philosophy:
  <https://hamel.dev/notes/llm/evals/inspect.html>
- LangChain on harness-and-memory coupling:
  <https://www.langchain.com/blog/your-harness-your-memory>
- The promptfoo and Inspect AI repositories themselves (their native formats
  inspected directly).
- Conversations with the project lead establishing the "make for AI" frame:
  the trace is the source artifact, downstream tools are build rules.
