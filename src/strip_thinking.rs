use anyhow::Result;
use std::io::{self, BufWriter, Read, Write};

const OPEN_TAG: &[u8] = b"<think>";
const CLOSE_TAG: &[u8] = b"</think>";

enum State {
    Normal,
    /// Accumulating potential `<think>` open tag; value is bytes matched so far
    MatchOpen(usize),
    /// Inside a `<think>` block — content is discarded
    Inside,
    /// Accumulating potential `</think>` close tag; value is bytes matched so far
    MatchClose(usize),
    /// Just closed a think block — skip trailing newlines
    AfterClose,
}

/// Streaming filter: reads from `reader`, strips `<think>…</think>` blocks,
/// writes the remainder to `writer`.  Works byte-by-byte so it handles both
/// buffered and streaming stdin.
pub fn strip_thinking_filter(reader: impl Read, writer: impl Write) -> Result<()> {
    let reader = io::BufReader::new(reader);
    let mut out = BufWriter::new(writer);
    let mut state = State::Normal;

    for byte_result in reader.bytes() {
        let b = byte_result?;
        match state {
            State::Normal => {
                if b == OPEN_TAG[0] {
                    state = State::MatchOpen(1);
                } else {
                    out.write_all(&[b])?;
                }
            }
            State::MatchOpen(pos) => {
                if b == OPEN_TAG[pos] {
                    if pos + 1 == OPEN_TAG.len() {
                        state = State::Inside;
                    } else {
                        state = State::MatchOpen(pos + 1);
                    }
                } else {
                    // Partial match failed — flush the buffered bytes
                    out.write_all(&OPEN_TAG[..pos])?;
                    if b == OPEN_TAG[0] {
                        state = State::MatchOpen(1);
                    } else {
                        out.write_all(&[b])?;
                        state = State::Normal;
                    }
                }
            }
            State::Inside => {
                if b == CLOSE_TAG[0] {
                    state = State::MatchClose(1);
                }
                // else: discard
            }
            State::MatchClose(pos) => {
                if b == CLOSE_TAG[pos] {
                    if pos + 1 == CLOSE_TAG.len() {
                        state = State::AfterClose;
                    } else {
                        state = State::MatchClose(pos + 1);
                    }
                } else if b == CLOSE_TAG[0] {
                    state = State::MatchClose(1);
                } else {
                    state = State::Inside;
                }
            }
            State::AfterClose => {
                if b == b'\n' || b == b'\r' {
                    // Skip newlines immediately after </think>
                } else if b == OPEN_TAG[0] {
                    state = State::MatchOpen(1);
                } else {
                    out.write_all(&[b])?;
                    state = State::Normal;
                }
            }
        }
    }

    // If we ended mid-match on an open tag, flush the partial bytes
    if let State::MatchOpen(pos) = state {
        out.write_all(&OPEN_TAG[..pos])?;
    }

    out.flush()?;
    Ok(())
}

/// Strip `<think>…</think>` blocks from an in-memory string.
/// Uses the byte-level filter so it matches blocks at any position.
pub fn strip_thinking_str(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    strip_thinking_filter(input.as_bytes(), &mut out)
        .expect("in-memory I/O cannot fail");
    String::from_utf8(out).expect("filter preserves UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(input: &str) -> String {
        let mut output = Vec::new();
        strip_thinking_filter(input.as_bytes(), &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn test_basic_strip() {
        assert_eq!(filter("<think>reasoning</think>answer"), "answer");
    }

    #[test]
    fn test_multiline_think() {
        assert_eq!(
            filter("<think>\nstep 1\nstep 2\n</think>\nanswer"),
            "answer"
        );
    }

    #[test]
    fn test_no_think_block() {
        assert_eq!(filter("just plain text"), "just plain text");
    }

    #[test]
    fn test_multiple_think_blocks() {
        assert_eq!(
            filter("<think>a</think>\nfirst\n<think>b</think>\nsecond"),
            "first\nsecond"
        );
    }

    #[test]
    fn test_empty_think_block() {
        assert_eq!(filter("<think></think>answer"), "answer");
    }

    #[test]
    fn test_partial_open_tag_not_stripped() {
        assert_eq!(filter("<thi some text"), "<thi some text");
    }

    #[test]
    fn test_think_at_end() {
        assert_eq!(filter("text<think>hidden</think>"), "text");
    }

    #[test]
    fn test_angle_brackets_in_text() {
        assert_eq!(filter("a < b && c > d"), "a < b && c > d");
    }

    #[test]
    fn test_trailing_newlines_stripped() {
        assert_eq!(filter("<think>r</think>\n\n\nanswer"), "answer");
    }

    #[test]
    fn test_passthrough_empty() {
        assert_eq!(filter(""), "");
    }

    #[test]
    fn test_unclosed_think_discards_rest() {
        assert_eq!(filter("before<think>rest of text"), "before");
    }

    #[test]
    fn test_close_tag_without_open_passthrough() {
        assert_eq!(filter("text</think>more"), "text</think>more");
    }

    #[test]
    fn test_nested_open_inside_think() {
        // Inner <think> is just consumed text inside the block
        assert_eq!(
            filter("<think>outer<think>inner</think>after"),
            "after"
        );
    }
}
