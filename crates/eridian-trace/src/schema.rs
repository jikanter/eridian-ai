//! SPEC-001 §2 envelope as Rust structs.
//!
//! The reader is intentionally lossy about `data`: it keeps the type-specific
//! payload as an untyped [`serde_json::Value`] so the crate parses *every*
//! event variant (SPEC-002 §7 criterion 3) without re-encoding the 17 typed
//! payloads that live in the producer (`src/utils/trace_spec/event.rs`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One trace line: the SPEC-001 §2 envelope with an untyped `data` payload.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TraceEvent {
    pub schema_version: String,
    pub session_id: String,
    #[serde(default)]
    pub parent_session_id: Option<String>,
    pub seq: u64,
    pub ts_ns: u64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: Value,
}

/// A parsed trace: events in on-disk order.
#[derive(Debug, Default, Clone)]
pub struct Trace {
    pub events: Vec<TraceEvent>,
}

impl Trace {
    /// All events whose dotted `type` equals `t`, in order.
    pub fn events_of_type(&self, _t: &str) -> Vec<&TraceEvent> {
        Vec::new() // stub
    }

    /// The single `session.start` event, if present.
    pub fn session_start(&self) -> Option<&TraceEvent> {
        None // stub
    }

    /// The single `session.end` event, if present.
    pub fn session_end(&self) -> Option<&TraceEvent> {
        None // stub
    }

    /// The single `output.final` event, if present.
    pub fn final_output(&self) -> Option<&TraceEvent> {
        None // stub
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(seq: u64, ty: &str) -> TraceEvent {
        TraceEvent {
            schema_version: "0.1".into(),
            session_id: "01HSESS".into(),
            parent_session_id: None,
            seq,
            ts_ns: seq * 1000,
            event_type: ty.into(),
            data: json!({}),
        }
    }

    fn trace() -> Trace {
        Trace {
            events: vec![
                ev(0, "session.start"),
                ev(1, "provider.retry"),
                ev(2, "provider.retry"),
                ev(3, "output.final"),
                ev(4, "session.end"),
            ],
        }
    }

    #[test]
    fn events_of_type_filters_in_order() {
        let t = trace();
        let retries = t.events_of_type("provider.retry");
        assert_eq!(retries.len(), 2);
        assert_eq!(retries[0].seq, 1);
        assert_eq!(retries[1].seq, 2);
        assert!(t.events_of_type("tool.denied").is_empty());
    }

    #[test]
    fn lifecycle_accessors_find_singletons() {
        let t = trace();
        assert_eq!(t.session_start().unwrap().seq, 0);
        assert_eq!(t.session_end().unwrap().seq, 4);
        assert_eq!(t.final_output().unwrap().seq, 3);
    }

    #[test]
    fn lifecycle_accessors_return_none_when_absent() {
        let t = Trace { events: vec![ev(0, "provider.request")] };
        assert!(t.session_start().is_none());
        assert!(t.session_end().is_none());
        assert!(t.final_output().is_none());
    }
}
