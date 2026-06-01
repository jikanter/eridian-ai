# SPEC-002: Eridian Test Harness

**Version:** 0.1
**Status:** Draft, ready for implementation after Phase 1 ships v0.1 traces
**Owners:** project lead

The test harness is two tracks. They share trace-parsing code but otherwise
operate independently:

1. **Regression matrix** via promptfoo. Volume layer: lots of test cases,
   declarative YAML, prompt × model × role combinatorics.
2. **Control-flow tests** via custom Rust integration tests + wiremock.
   Sparse layer: few high-value tests that exercise retry, fallback,
   tool-denial, and policy logic via deterministic provider failure
   injection.

Neither track replaces the other. They have different cost profiles and
different testing philosophies.

## 1. Repository layout

```text
eridian/
├── tests/
│   ├── regression/                    # promptfoo (Track 1)
│   │   ├── promptfooconfig.yaml
│   │   ├── prompts/
│   │   │   └── *.txt
│   │   ├── helpers/
│   │   │   └── trace.js               # JS helper to parse traces
│   │   └── fixtures/
│   │       └── *.yaml                 # input test cases
│   └── control_flow/                  # Rust + wiremock (Track 2)
│       ├── Cargo.toml
│       ├── src/
│       │   └── lib.rs                 # shared test helpers
│       └── tests/
│           ├── retry.rs
│           ├── fallback.rs
│           ├── tool_denial.rs
│           ├── timeout.rs
│           └── stream_interrupt.rs
└── crates/
    └── eridian-trace/                 # shared trace parsing
        ├── Cargo.toml
        └── src/
            ├── lib.rs
            ├── parse.rs               # streaming + batch JSONL readers
            ├── schema.rs              # event types as Rust structs
            └── assertions.rs          # common assertion helpers
```

## 2. Track 1: promptfoo regression

### Purpose

Catch behavioral regressions: "did this prompt × role × model combination
still produce sensible output after my changes?" High volume, low cost
per test.

### Why promptfoo and not a custom YAML runner

- Matrix testing (prompt × provider × case) is its core value proposition
- Built-in assertion types (`contains`, `regex`, `llm-rubric`) handle
  90% of cases without custom code
- GitHub Action exists for PR-comment integration
- CI/CD output is well-understood
- The `exec` provider lets us target aichat as a subprocess without
  writing a custom integration

### Configuration

Located at `tests/regression/promptfooconfig.yaml`:

```yaml
description: Eridian regression matrix

prompts:
  - file://prompts/general.txt
  - file://prompts/with_rag.txt

providers:
  - id: aichat-default
    config:
      command: |
        aichat \
          --role {{role}} \
          --no-stream \
          --trace-out /tmp/eridian-test-$$.jsonl \
          {{prompt}}
  - id: aichat-claude
    config:
      command: |
        aichat \
          --model anthropic:claude-opus-4-7 \
          --role {{role}} \
          --trace-out /tmp/eridian-test-$$.jsonl \
          {{prompt}}

defaultTest:
  options:
    transform: |
      // Capture trace path for downstream assertions
      const tracePath = `/tmp/eridian-test-${process.pid}.jsonl`;
      return { output, traceFile: tracePath };

tests:
  - description: Rust reviewer identifies unwrap as a panic risk
    vars:
      role: rust-reviewer
      prompt: "Review this code: fn parse(s: &str) -> i32 { s.parse().unwrap() }"
    assert:
      - type: contains-any
        value: ["unwrap", "panic"]
      - type: llm-rubric
        value: "Identifies the unwrap as a panic risk and suggests Result-based handling"
      - type: javascript
        value: |
          const { events } = require('./helpers/trace.js').parseTraceFile(context.traceFile);
          // Happy path should not retry
          const retries = events.filter(e => e.type === 'provider.retry');
          return retries.length === 0;
```

### Trace-aware assertions

The `helpers/trace.js` file:

```javascript
// tests/regression/helpers/trace.js
const fs = require('fs');

exports.parseTraceFile = function(path) {
  const content = fs.readFileSync(path, 'utf-8');
  const events = content
    .trim()
    .split('\n')
    .filter(line => line.length > 0)
    .map(line => {
      try { return JSON.parse(line); }
      catch (e) { return null; } // tolerate trailing partial lines
    })
    .filter(e => e !== null);

  return {
    events,
    eventsOfType: (type) => events.filter(e => e.type === type),
    sessionStart: events.find(e => e.type === 'session.start'),
    sessionEnd: events.find(e => e.type === 'session.end'),
    finalOutput: events.find(e => e.type === 'output.final'),
  };
};
```

Used inside `javascript` assertions to assert on the trace file aichat
just wrote.

### CI integration

`.github/workflows/regression.yml` runs promptfoo via the official
GitHub Action on PRs that touch `prompts/`, `roles/`, or any aichat
behavior code:

```yaml
- uses: promptfoo/promptfoo-action@v1
  with:
    working-directory: tests/regression
    fail-on-threshold: 95   # 95% pass rate required
    config: promptfooconfig.yaml
```

### What promptfoo cannot test

The exec provider is a black box from promptfoo's perspective: prompt in,
text out. It cannot:

- Inject deterministic provider failures
- Assert on intra-call event sequences as test *primitives* (we work
  around this with `javascript` assertions reading the trace, but the
  ergonomics are bad and time-mocking is impossible)
- Test code paths that don't produce visible output (e.g., did the
  whitelist deny correctly, while the model still produced a sensible
  fallback response?)

These cases live in Track 2.

## 3. Track 2: Rust integration tests + wiremock

### Purpose

Test aichat's internal control flow under deterministic provider behavior:
retry triggers, exponential backoff, fallback transitions, tool whitelist
enforcement, stream interruption recovery.

### Why a custom harness instead of more promptfoo

Promptfoo's exec provider is the wrong abstraction here. We need:

- A mock HTTP server we fully control (response sequencing, latency,
  malformed bodies, mid-stream disconnects)
- The ability to assert on internal events as first-class primitives,
  not by reading a trace file from JS
- Time mocking, so tests with 30-second backoffs don't take 30 seconds
  of wall clock to run
- Direct access to aichat as a Rust binary so we can capture its trace
  output and parse it with the same `eridian-trace` crate the rest of
  Eridian uses

### Stack

- **`wiremock-rs`** ([LukeMathWalker/wiremock-rs](https://github.com/LukeMathWalker/wiremock-rs))
  for the mock provider. Async, supports response sequencing, plays well
  with `tokio::test`.
- **`tokio` test runtime** with `tokio::time::pause()` for time mocking.
  Requires aichat's retry-backoff logic to use `tokio::time::sleep`, not
  `std::thread::sleep`. Audit before Phase 2 starts; if needed, file an
  Eridian issue to fix the time abstraction.
- **`eridian-trace`** crate for parsing the JSONL the test produced.
- **`tempfile`** for per-test trace directories.
- **`assert_cmd`** for invoking aichat as a subprocess and capturing its
  output / exit status.

### Test layout

Each test in `tests/control_flow/tests/` follows this pattern:

```rust
use eridian_trace::{parse_trace_file, EventType};
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path};

#[tokio::test]
async fn retry_fires_on_502_then_succeeds() {
    // 1. Spin up mock provider
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(502))
        .up_to_n_times(2)
        .mount(&mock).await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_response()))
        .mount(&mock).await;

    // 2. Set up trace output
    let tracedir = tempfile::tempdir().unwrap();
    let trace_path = tracedir.path().join("trace.jsonl");

    // 3. Run aichat against the mock
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aichat"))
        .env("ANTHROPIC_API_BASE", mock.uri())
        .env("AICHAT_TRACE_DIR", tracedir.path())
        .env("AICHAT_FIXTURE_ID", "retry_fires_on_502_then_succeeds")
        .arg("--trace-out")
        .arg(&trace_path)
        .arg("hello")
        .output()
        .expect("aichat invocation failed");

    assert!(output.status.success(), "aichat exited non-zero");

    // 4. Parse trace and assert on events
    let trace = parse_trace_file(&trace_path).unwrap();

    let retries = trace.events_of_type("provider.retry");
    assert_eq!(retries.len(), 2);
    assert_eq!(retries[0].data["trigger"], "http_5xx");
    assert!(
        retries[0].data["backoff_ms"].as_u64().unwrap()
        < retries[1].data["backoff_ms"].as_u64().unwrap(),
        "expected exponential backoff"
    );

    let final_output = trace.events_of_type("output.final");
    assert_eq!(final_output.len(), 1);
}
```

### Phase 2 minimum test set

These five tests gate Phase 2 acceptance:

1. `retry_fires_on_502_then_succeeds` — happy retry path.
2. `retry_exhausts_then_fails` — max-retries reached, no fallback
   configured. Asserts on `error` event with `kind: exhausted_retries`.
3. `fallback_kicks_in_after_retry_exhaustion` — same as above but with
   fallback configured. Asserts on `provider.fallback` event.
4. `tool_denial_when_not_in_whitelist` — model requests a tool not in
   the role's whitelist. Asserts on `tool.denied` event with reason.
5. `stream_interrupt_triggers_retry` — mock disconnects mid-stream.
   Asserts on `provider.retry` with `trigger: stream_interrupted`.

### Time mocking dependency

Several of these tests would take 30+ seconds of wall-clock time without
mocking. `tokio::time::pause()` only works if aichat's retry code uses
`tokio::time::sleep`. **Pre-Phase-2 audit task:** verify aichat's retry
implementation uses tokio time abstractions throughout. If it uses
`std::thread::sleep` or wall-clock arithmetic, file an issue to refactor
before Phase 2 starts. This is a known dependency, not a surprise.

## 4. Shared crate: `eridian-trace`

A small Rust crate that lives in `crates/eridian-trace/`. Used by:

- The Track 2 integration tests.
- The `aichat trace show` command.
- The future marimo trace explorer (deferred).
- The future Snakemake/DVC training pipelines (deferred).

### API sketch

```rust
// crates/eridian-trace/src/lib.rs

pub use schema::*;
pub use parse::{parse_trace_file, parse_trace_stream, ParseError};

pub mod schema;
pub mod parse;
pub mod assertions;

// schema.rs
#[derive(Debug, Deserialize, Serialize)]
pub struct TraceEvent {
    pub schema_version: String,
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub seq: u64,
    pub ts_ns: u64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: serde_json::Value,
}

#[derive(Debug)]
pub struct Trace {
    pub events: Vec<TraceEvent>,
}

impl Trace {
    pub fn events_of_type(&self, t: &str) -> Vec<&TraceEvent> { ... }
    pub fn session_start(&self) -> Option<&TraceEvent> { ... }
    pub fn session_end(&self) -> Option<&TraceEvent> { ... }
    pub fn final_output(&self) -> Option<&TraceEvent> { ... }
}

// parse.rs
pub fn parse_trace_file(path: &Path) -> Result<Trace, ParseError>;
pub fn parse_trace_stream<R: BufRead>(reader: R) -> Result<Trace, ParseError>;
```

The streaming parser tolerates trailing partial lines (per
`SPEC-001` §3, crash safety guarantee).

## 5. CI gating

- **Track 1 (promptfoo).** Runs on PRs that touch `prompts/`, `roles/`,
  `tests/regression/`, or any aichat code. Requires a 95% pass rate.
- **Track 2 (control-flow).** Runs on every PR. All tests must pass.
- **`eridian-trace` unit tests.** Run on every PR. Includes a
  property-based test (via `proptest`) that fuzzes consumer behavior under
  partial reads and malformed lines, to keep the streaming-safety
  invariants from regressing.

## 6. What this spec deliberately omits

- **Inspect AI eval harness.** Deferred to Phase 3. Will live in
  `tests/eval/` when added.
- **Performance benchmarks.** A small Criterion bench suite for
  trace-emission throughput is desirable but separate; will live in
  `benches/`.
- **End-to-end demos.** Per `ADR-0004`, every PR ships a Showboat demo,
  but those live in `demos/`, not `tests/`.

## 7. Acceptance criteria for SPEC-002 v0.1

The harness is complete when:

1. `tests/regression/` contains at least 20 promptfoo test cases covering
   the major roles in Eridian's default config.
2. `tests/control_flow/tests/` contains the five tests listed in §3 and
   they pass on a clean CI run.
3. The `eridian-trace` crate parses every event type in `SPEC-001` and
   has unit-test coverage for the malformed-input edge cases.
4. The promptfoo GitHub Action posts comments on PRs with regression
   results.
5. A Showboat demo at `demos/test-harness-overview.md` walks through
   running both tracks against a known-good aichat build, with output
   visible.
