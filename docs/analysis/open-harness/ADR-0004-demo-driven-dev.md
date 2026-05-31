# ADR-0004: Every PR ships with a Showboat demo

**Status:** Accepted, 2026-04-25
**Decider:** project lead

## Context

This workstream is being implemented primarily by coding agents (Claude Code).
Two failure modes are likely:

1. **Tests pass, feature doesn't actually work.** The agent writes code and
   tests that satisfy themselves but don't exercise the real-world path
   end-to-end. Common with subprocess-spawning code, file I/O, and external
   process integration — exactly the surface area Eridian's trace work
   touches.
2. **Reviewer can't tell if it works without running it themselves.** The PR
   description says "added trace emission" but the reviewer has to clone,
   build, and exercise it manually to confirm.

Simon Willison's [Showboat](https://github.com/simonw/showboat)
([blog post, 2026-02-10](https://simonwillison.net/2026/Feb/10/showboat-and-rodney/))
is a small Go CLI explicitly designed for coding agents to construct
Markdown demonstration documents. The agent runs `showboat init demo.md
"Title"`, then `showboat note`, `showboat exec`, and `showboat image`
commands to assemble a document that captures both *what was run* and *what
the actual output was*. The result is a Markdown file that lives in the
repo, gets reviewed alongside the code, and demonstrates the feature
working end-to-end.

Showboat is paired with [Rodney](https://github.com/simonw/rodney), a CLI
browser automation tool built on `go-rod`, for capturing screenshots and
exercising web UIs as part of the demo flow.

## Decision

**Every PR landing trace-emission work or test-harness work ships with a
Showboat-generated `demos/<feature>.md` file** that demonstrates the feature
working end-to-end. The demo is not optional, not "nice to have," and not
something added later. It's part of the PR.

For Eridian-specific scenario demos (retry behavior under failure injection,
fallback transitions, tool denial, redaction), Showboat is the canonical
format.

**Rodney is used only for `aichat --serve` web playground / arena demos.**
For pure CLI features (the bulk of Phase 1 and Phase 2 work), Rodney is
not used.

## What a good demo looks like

For trace-emission work:

- A `demo.md` showing a real `aichat` invocation with `--trace-out` set,
  followed by the resulting JSONL contents (via `cat` and `jq`), followed
  by a resolution of one blob via `aichat trace show <id>`. Reviewer sees
  the feature behaving as the spec describes.

For control-flow tests (Phase 2):

- A `demo.md` showing `cargo test` running the wiremock-driven retry test,
  followed by the trace file the test produced, followed by the assertions
  that fired against it. Reviewer sees the test infrastructure exercising
  real Eridian behavior, not mocked-out unit tests.

For redaction:

- A `demo.md` running an aichat invocation against a fixture with embedded
  fake API keys, then `cat`ing the trace and `grep`ping for the keys to
  show they were stripped. Confirms redaction worked, not just that the
  unit test passed.

## Workflow

1. Implement the feature.
2. Write tests for it. Run them green.
3. **Then** write the Showboat demo:
   ```
   uvx showboat init demos/<feature>.md "<feature>"
   uvx showboat note demos/<feature>.md "<context>"
   uvx showboat exec demos/<feature>.md bash "<command exercising the feature>"
   ```
4. Open the demo in a Markdown previewer to sanity-check it. The agent
   doing the work cannot see the rendered output; the human reviewer can.
5. Commit demo alongside code and tests. PR description references the
   demo path.

## Consequences

### Positive

- **Reviewer signal-to-noise improves dramatically.** A reviewer can scan
  the rendered demo in seconds and tell whether the feature works. This
  matters enormously when reviewing agent-produced PRs at volume.
- **Catches "tests pass, feature doesn't" failures.** Showboat's `exec`
  captures actual command output. If the feature is broken in the way it
  meets reality (file paths, env vars, subprocess invocation), the demo
  fails visibly rather than silently passing CI.
- **Builds an institutional library of "this is how this feature behaves."**
  Future contributors can read past demos to understand the system's
  actual behavior, not just its specified behavior.
- **Catches agents cheating themselves.** If an agent's mental model of
  what the feature does diverges from what the binary actually does, the
  demo step surfaces it before review.

### Negative

- **One extra step per PR.** Real cost, modest. Demos for the kinds of
  features we're building are typically 5–10 lines of Showboat commands.
- **Demos can be cheated.** Per the Showboat docs, agents can sometimes
  edit the Markdown file directly rather than using Showboat commands,
  producing fake output. Mitigation: code review explicitly checks that
  the demo was produced by Showboat (check for the timestamp marker, the
  rendered output blocks). Long-term, `showboat verify` re-runs commands
  to detect tampering, though its design is not yet final per the
  Showboat author.
- **Showboat is a third-party Go CLI.** We depend on `simonw/showboat`
  remaining maintained. If it disappears, we'd need to either fork or
  reimplement the trivial `init/note/exec/image` API. Acceptable risk
  given how small the tool is.

### Risks accepted

- **Demos drift from reality over time.** A demo committed at PR time
  reflects behavior at that moment; if the feature changes later, the
  demo isn't automatically updated. Mitigation: a demo is a snapshot,
  not a regression test. Real regression coverage comes from the test
  harness in `SPEC-002`. Demos are for human review at PR time.
- **Demo files accumulate.** Over many PRs, `demos/` grows. We accept
  this; they're cheap to keep, and grouping them by feature
  (`demos/trace-emission/`, `demos/redaction/`) keeps it navigable.

## Out of scope

- **Automated demo regeneration.** We could imagine a CI job that re-runs
  every demo on every commit. Not doing it. The cost is high (LLM API
  calls for any demo that touches a real model), the value is low (real
  regression coverage is the test harness), and it's a distraction.
- **Showboat for Phase 3 work.** The discipline applies to Phase 1 and
  Phase 2. Whether to extend it to training-pipeline work, eval setup,
  etc. is a future decision.

## Sources

- Showboat: <https://github.com/simonw/showboat>
- Rodney: <https://github.com/simonw/rodney>
- "Introducing Showboat and Rodney, so agents can demo what they've built"
  (Simon Willison, 2026-02-10):
  <https://simonwillison.net/2026/Feb/10/showboat-and-rodney/>
- Showboat issue on agent self-cheating:
  <https://github.com/simonw/showboat/issues/12>
