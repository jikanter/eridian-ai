//! Phase 25 (Epic 9): Knowledge Compilation.
//!
//! Compiles input files (markdown, code, notes) into a typed, tagged,
//! provenance-grounded atomic fact store — no embeddings in the primary path.
//! See `docs/analysis/epic-9.md` for the design rationale (FADER, AEVS,
//! Karpathy's compiled-KB pattern).
//
// Several helpers here (edge/graph APIs on `KnowledgeStore`, `FactId::from_raw`,
// `SourceAnchor::span_len`, `check_fact`) are exercised by `cfg(test)` and
// will become load-bearing once Phase 26 (query / graph walk) and Phase 27
// (evolution loop) land. Scoping the dead-code allow to this module keeps
// the production build clean without writing throwaway consumers for items
// that will be called naturally in a few phases.
#![allow(dead_code)]

pub mod cli;
pub mod compile;
pub mod edp;
pub mod restore;
pub mod store;
pub mod tags;

// External API: just the CLI entry points. Internal types (EDP, Tag,
// KnowledgeStore, etc.) are accessed through `knowledge::<module>::<Type>`
// by intra-crate consumers (Phase 26, Phase 27) once those land.
pub use cli::{run_compile, run_list, run_show, run_stat};
