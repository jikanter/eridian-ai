//! `eridian-trace` — streaming-safe reader + assertion helpers for the SPEC-001
//! Eridian trace format.
//!
//! This crate is the single shared consumer of the trace contract. It backs:
//! - the Phase 43 control-flow test harness (SPEC-002 Track 2),
//! - the `aichat trace show` command,
//! - the Phase 44 trace projections (OTel) and training-pair extraction.
//!
//! It never *emits* traces (that is the producer in `src/utils/trace_spec/`);
//! it only reads them.

pub mod parse;
pub mod schema;

pub use parse::{parse_trace_file, parse_trace_stream, ParseError};
pub use schema::{Trace, TraceEvent};