//! SPEC-001 structured trace emitter (Phase 42 — Observability Keystone).
//!
//! This is the new keystone trace contract that supersedes the ad-hoc
//! `utils::trace` JSONL (Phases 8F/8G) in Phase 42D. It is built as a
//! separate module so the two can coexist until the cutover.
//!
//! Grounding docs: `docs/analysis/caching/SPEC-001-trace-format.md`,
//! `ADR-0002` (streaming-safe), `ADR-0003` (async writer thread).
//!
//! Phase 42A delivers the schema + emitter as a self-contained, fully-tested
//! unit. The producer call sites (context/provider/tool/output events) and the
//! `--trace`/`AICHAT_TRACE` surface land in 42B–42D, which is why several
//! public items are not yet referenced from the request path.
#![allow(dead_code)]

pub mod blob;
pub mod event;
pub mod redact;
pub mod ulid;
pub mod writer;
