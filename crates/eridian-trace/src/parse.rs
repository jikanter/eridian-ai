//! Streaming + batch JSONL readers.
//!
//! SPEC-001 §3 crash-safety guarantee: a trace may end with a torn final line
//! (the writer was killed mid-append). The streaming parser therefore tolerates
//! a trailing partial line — it parses every complete line and silently drops a
//! final fragment that fails to parse. A malformed line *in the middle* of the
//! stream is a real error and surfaces as [`ParseError`].

use crate::schema::{Trace, TraceEvent};
use std::fmt;
use std::fs;
use std::io::BufRead;
use std::path::Path;

/// Errors the reader can surface. Defined by hand to keep the crate at zero new
/// dependencies (no `thiserror`).
#[derive(Debug)]
pub enum ParseError {
    /// I/O error reading the trace file.
    Io(std::io::Error),
    /// A complete, non-final line failed to parse as a SPEC-001 envelope.
    Malformed { line_no: usize, source: serde_json::Error },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Io(e) => write!(f, "trace I/O error: {e}"),
            ParseError::Malformed { line_no, source } => {
                write!(f, "malformed trace line {line_no}: {source}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

impl From<std::io::Error> for ParseError {
    fn from(e: std::io::Error) -> Self {
        ParseError::Io(e)
    }
}

/// Parse a whole trace file into memory.
pub fn parse_trace_file(_path: &Path) -> Result<Trace, ParseError> {
    Ok(Trace::default()) // stub
}

/// Parse a trace from any buffered reader, tolerating a torn final line.
///
/// A line missing its terminating newline can only be the last line (EOF was
/// reached mid-write). If such a tail fails to parse it is silently dropped —
/// the SPEC-001 §3 crash-safety contract. A parse failure on a newline-
/// terminated line is a genuine [`ParseError::Malformed`].
pub fn parse_trace_stream<R: BufRead>(mut reader: R) -> Result<Trace, ParseError> {
    let mut events = Vec::new();
    let mut line_no = 0usize;
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf)?;
        if n == 0 {
            break; // EOF
        }
        line_no += 1;
        let had_newline = buf.ends_with('\n');
        let line = buf.trim_end_matches(['\n', '\r']);
        if line.is_empty() {
            continue; // blank line — nothing to parse
        }
        match serde_json::from_str::<TraceEvent>(line) {
            Ok(ev) => events.push(ev),
            Err(source) => {
                if had_newline {
                    return Err(ParseError::Malformed { line_no, source });
                }
                // Torn final fragment: tolerate per the crash-safety guarantee.
                break;
            }
        }
    }
    Ok(Trace { events })
}

// Silence unused-import warnings while `parse_trace_file` is still a stub.
#[allow(unused)]
fn _uses(_: fs::Metadata) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const START: &str = r#"{"schema_version":"0.1","session_id":"01HSESS","parent_session_id":null,"seq":0,"ts_ns":1000,"type":"session.start","data":{"role":"rust-reviewer"}}"#;
    const RETRY: &str = r#"{"schema_version":"0.1","session_id":"01HSESS","parent_session_id":null,"seq":1,"ts_ns":2000,"type":"provider.retry","data":{"attempt":2,"trigger":"http_5xx"}}"#;

    #[test]
    fn parses_complete_lines_and_tolerates_torn_final_line() {
        // Two complete lines, then a torn final fragment (writer killed mid-append).
        let raw = format!("{START}\n{RETRY}\n{{\"schema_version\":\"0.1\",\"sess");
        let trace = parse_trace_stream(Cursor::new(raw)).expect("partial tail is not an error");

        assert_eq!(trace.events.len(), 2, "torn final line must be dropped, not counted");
        assert_eq!(trace.events[0].event_type, "session.start");
        assert_eq!(trace.events[0].seq, 0);
        assert_eq!(trace.events[1].event_type, "provider.retry");
        assert_eq!(trace.events[1].data["trigger"], "http_5xx");
    }
}