# PLAN: Phase 2 — Test Harness

**Status:** Ready to start *after* Phase 1 lands a stable v0.1 trace
**Depends on:** SPEC-002, Phase 1 PR-12 (acceptance), pre-flight PF-1 (tokio time audit)

The test harness is two tracks (regression matrix via promptfoo,
control-flow tests via Rust + wiremock). The plan below sequences them
serially: Track 2 first because it's the higher-risk integration work,
Track 1 second because it's mostly configuration once the trace surface
is stable.

## Pre-flight

### PF-1 (cont'd): Verify tokio time abstractions are in place

If the audit from Phase 1 PF-1 found `std::thread::sleep` or wall-clock
arithmetic in the retry path, that refactor must land before TH-3.
Tests with 30+ second backoffs are not acceptable.

### PF-3: Verify `--api-base` override works for all providers

Many of the tests in `SPEC-002` rely on pointing aichat at a wiremock
endpoint via `OPENAI_API_BASE` / `ANTHROPIC_API_BASE`. Confirm the
override is wired for each provider in the test matrix. Likely already
works (aichat supports custom OpenAI-compatible endpoints), but verify
before TH-3.

## Track 2 first: control-flow tests

### TH-1: Test crate skeleton

**Goal:** the `tests/control_flow/` Cargo crate, with shared helpers but
no actual tests yet.

**Scope:**

- `tests/control_flow/Cargo.toml` with deps on `wiremock`, `tokio`,
  `tempfile`, `assert_cmd`, `eridian-trace`, `serde_json`
- `tests/control_flow/src/lib.rs` — shared helpers:
  - `spawn_aichat(env_overrides, args, trace_dir) -> Output`
  - `success_response_body() -> serde_json::Value` (Anthropic-shaped)
  - `error_response_body(code, message) -> serde_json::Value`
- A trivial smoke test that spawns aichat with a mock server, gets a
  successful response, and parses the trace. Validates the harness works
  end-to-end before any real test logic.

**Out of scope:** the five specific tests from `SPEC-002` §3. Those come
in TH-3 through TH-7.

**Acceptance:**

- `cargo test -p eridian-control-flow-tests` is green with the smoke
  test
- A Showboat demo at `demos/control-flow-harness.md` shows: running the
  smoke test, the trace it produces, and an explanation of how the
  helpers chain together

**Estimated size:** medium PR (~400 LOC).

### TH-2: Time mocking integration

**Goal:** verify `tokio::time::pause()` works with aichat's retry path.
This is a focused PR because it can fail in subtle ways and we want to
catch those before piling tests on top.

**Scope:**

- A single test that:
  1. Configures aichat with a 30-second backoff
  2. `tokio::time::pause()` before invoking
  3. Mocks the provider to fail twice, then succeed
  4. Asserts the test completes in < 1 second wall-clock
  5. Asserts the trace shows two retries with correct backoff_ms

**If this test cannot be made to work**, that's the signal that PF-1
wasn't sufficient — there's a deeper issue with aichat's time model.
Stop and file an issue rather than working around it.

**Acceptance:**

- Test passes in well under 1 second
- A Showboat demo at `demos/time-mocking.md` runs the test with `--nocapture`
  showing the timestamps in the trace

**Estimated size:** small PR (~150 LOC), but high investigation risk.
Time-box at one day; if it's not working, escalate.

### TH-3: Test 1 — `retry_fires_on_502_then_succeeds`

The first real test. Implements the example in `SPEC-002` §3 verbatim.

**Acceptance:**

- Test passes
- Trace assertions verify exactly 2 retries, exponential backoff,
  successful final output
- Showboat demo at `demos/test-retry-happy-path.md`

**Estimated size:** small PR (~100 LOC).

### TH-4: Test 2 — `retry_exhausts_then_fails`

**Scope:** Mock provider returns 502 indefinitely. Configure aichat with
max_retries=3. Assert on `error` event with `kind: exhausted_retries`,
nonzero exit status.

**Acceptance:**

- Test passes
- Showboat demo at `demos/test-retry-exhausted.md`

**Estimated size:** small PR (~100 LOC).

### TH-5: Test 3 — `fallback_kicks_in_after_retry_exhaustion`

**Scope:** Two mock servers (primary fails, fallback succeeds). Configure
aichat with provider fallback. Assert on `provider.fallback` event,
successful output, fallback's `provider.request` follows.

**Acceptance:**

- Test passes
- Showboat demo at `demos/test-fallback.md`

**Estimated size:** small PR (~150 LOC).

### TH-6: Test 4 — `tool_denial_when_not_in_whitelist`

**Scope:** Mock provider returns a response containing a tool call for a
tool not in the role's whitelist. Assert `tool.denied` event, model is
re-prompted with denial info, eventual successful output.

**Acceptance:**

- Test passes
- Showboat demo at `demos/test-tool-denial.md`

**Estimated size:** small-medium PR (~200 LOC). Higher complexity
because tool-call response shaping is provider-specific.

### TH-7: Test 5 — `stream_interrupt_triggers_retry`

**Scope:** Mock provider sends a partial streaming response, then closes
the connection mid-stream. Assert `provider.retry` with
`trigger: stream_interrupted`, eventual successful retry.

**Acceptance:**

- Test passes
- Showboat demo at `demos/test-stream-interrupt.md`

**Estimated size:** medium PR (~250 LOC). Most complex of the five
because wiremock's streaming-response support requires careful setup.

## Track 1: regression matrix via promptfoo

### TH-8: promptfoo config skeleton + 5 baseline tests

**Goal:** stand up the regression-test infrastructure with enough cases
to verify the wiring works.

**Scope:**

- `tests/regression/promptfooconfig.yaml` per `SPEC-002` §2
- `tests/regression/helpers/trace.js` per `SPEC-002` §2
- Five test cases, one per major built-in role
- A `Justfile` or `Makefile` target `just test:regression` that runs
  promptfoo with the right working directory

**Acceptance:**

- `just test:regression` runs locally and produces output
- The five baseline tests pass
- A Showboat demo at `demos/regression-baseline.md` shows running the
  tests and reading the output

**Estimated size:** medium PR (~300 LOC of YAML/JS plus 5 test cases).

### TH-9: Expand to 20+ regression tests

**Goal:** cover the breadth required by `SPEC-002` §7 acceptance criterion 1.

**Scope:**

- Bring the test count to 20+ across the major roles, prompt patterns,
  and model families
- At least one test per role exercises a trace-aware assertion (not
  just output-text assertions)

**Acceptance:**

- `tests/regression/` has 20+ test cases, organized by role
- All pass on CI
- A Showboat demo at `demos/regression-expanded.md`

**Estimated size:** medium PR. Most of the work is writing test cases
rather than infrastructure.

### TH-10: GitHub Action for PR comments

**Scope:**

- `.github/workflows/regression.yml` per `SPEC-002` §2
- Configured to run only on changes to relevant paths
- 95% pass-rate threshold

**Acceptance:**

- A test PR triggers the workflow and posts a comment
- A Showboat demo at `demos/regression-ci.md` includes a screenshot of
  the PR comment (this is a Rodney use case — the PR comment is a
  GitHub web view)

**Estimated size:** small PR (~100 LOC of YAML).

## Phase 2 close-out

### TH-11: Phase 2 acceptance suite

**Goal:** verify all five acceptance criteria from `SPEC-002` §7.

**Scope:**

- Verification script that confirms the test counts, the five
  control-flow tests, the eridian-trace coverage, the GitHub Action,
  and the overview Showboat demo
- A `demos/test-harness-overview.md` Showboat demo per `SPEC-002` §7
  criterion 5
- A Phase 2 retrospective in `docs/architecture/RETRO-phase2.md`

**Acceptance:**

- All five `SPEC-002` §7 criteria pass
- Schema feedback gathered during Phase 2 is filed as issues for v0.2
  consideration

**Estimated size:** small PR (~150 LOC + the demo).

## Sequencing summary

```text
PF-1 (verified), PF-3 (audit)
   ↓
TH-1 (test crate skeleton)
   ↓
TH-2 (time mocking)
   ↓
TH-3 → TH-4 → TH-5 → TH-6 → TH-7  (the five control-flow tests)
   ↓
TH-8 (promptfoo skeleton)
   ↓
TH-9 (expand)
   ↓
TH-10 (CI integration)
   ↓
TH-11 (acceptance + retro)
```

Track 2 (TH-1 through TH-7) is sequenced first because it's the higher-risk
integration work and likely to surface schema gaps that send us back to
v0.2. Track 1 (TH-8 through TH-10) is mostly configuration once the trace
surface is proven stable.

## Schema feedback discipline

Phase 2 is the first time real consumers exercise `SPEC-001`. The
expected outcome: at least one schema gap is found.

When a gap is found:

1. **Don't extend the schema unilaterally.** File an issue describing the
   missing event or field, the use case, and the proposed addition.
2. **Tag the issue `schema-v0.2`**. Batch these for a deliberate v0.2
   bump rather than letting v0.1 drift.
3. **Work around the gap in the test for now** if possible (often a
   custom JS assertion in promptfoo can read the event stream and
   compute the missing signal).

If multiple gaps accumulate fast, that's the signal to pause Phase 2 and
ship a v0.2 of the trace before continuing. Don't build a test corpus on
top of a schema you know is wrong.
