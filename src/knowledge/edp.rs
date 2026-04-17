//! Phase 25A: Entity-Description Pair (EDP) — the atomic unit of the
//! knowledge store. See `docs/analysis/epic-9.md` Feature 1.
//!
//! Each EDP pairs a short entity anchor with a factual description, plus
//! typed tags, a deterministic source anchor (for AEVS restore-check, Phase
//! 25C), and outbound edges (for graph walk expansion, Phase 26B).
//!
//! `FactId` is a content hash of `(entity, description, path, byte_start)`
//! so identical facts converge to the same id — free dedup across repeated
//! compilation runs (Phase 25B's sample augmentation step).

use crate::utils::sha256;
use serde::{Deserialize, Serialize};

use super::tags::Tag;

/// Opaque, deterministic fact identifier. Content-addressable:
/// `(entity, description, path, byte_start)` → same id. Rendered as
/// `fact-<hex>` in logs and CLI output.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FactId(String);

impl FactId {
    /// Compute the deterministic id from the fact's identifying fields. The
    /// underlying hash is full SHA-256 so intra-KB collisions are practically
    /// impossible; the display form truncates for readability.
    pub fn compute(entity: &str, description: &str, path: &str, byte_start: usize) -> Self {
        let input = format!("{entity}\0{description}\0{path}\0{byte_start}");
        Self(format!("fact-{}", &sha256(&input)[..16]))
    }

    /// Wrap a pre-existing id string (used by JSONL load).
    pub fn from_raw<S: Into<String>>(raw: S) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FactId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Deterministic pointer back to the source region the fact was extracted
/// from. `content_hash` captures the state of the file at compile time so
/// Phase 25C's restore-check can refuse to match against a file that has
/// drifted since compilation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAnchor {
    pub path: String,
    /// Byte offsets into the source file, half-open `[start, end)`.
    pub byte_range: (usize, usize),
    /// 1-indexed inclusive line range, for human-facing display.
    pub line_range: (usize, usize),
    /// SHA-256 of the source file contents at compile time.
    pub content_hash: String,
}

impl SourceAnchor {
    pub fn span_len(&self) -> usize {
        self.byte_range.1.saturating_sub(self.byte_range.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Markdown / wiki link target resolved to another fact in the store.
    MarkdownLink,
    /// Both facts were extracted from the same source file.
    SharedFile,
    /// Both facts name the same canonical entity (post-normalization).
    SharedEntity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeRef {
    pub to: FactId,
    pub kind: EdgeKind,
}

/// The atomic fact unit. Everything the KB stores is one of these; everything
/// the query layer retrieves is one or more of these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityDescriptionPair {
    pub id: FactId,
    pub entity: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    pub provenance: SourceAnchor,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<EdgeRef>,
}

impl EntityDescriptionPair {
    /// Build an EDP, computing `id` from the identifying fields. Callers that
    /// need to preserve an existing id (e.g. loading from JSONL) should
    /// construct the struct literal directly.
    pub fn new(
        entity: impl Into<String>,
        description: impl Into<String>,
        tags: Vec<Tag>,
        provenance: SourceAnchor,
        edges: Vec<EdgeRef>,
    ) -> Self {
        let entity = entity.into();
        let description = description.into();
        let id = FactId::compute(
            &entity,
            &description,
            &provenance.path,
            provenance.byte_range.0,
        );
        Self {
            id,
            entity,
            description,
            tags,
            provenance,
            edges,
        }
    }

    /// Serialize as a single JSON line for `facts.jsonl` (Phase 25D).
    pub fn to_jsonl_line(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_jsonl_line(line: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(line)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::tags::Tag;

    fn anchor(path: &str, start: usize, end: usize) -> SourceAnchor {
        SourceAnchor {
            path: path.into(),
            byte_range: (start, end),
            line_range: (1, 3),
            content_hash: "deadbeef".into(),
        }
    }

    #[test]
    fn fact_id_is_deterministic() {
        let a = FactId::compute("Alan Turing", "Pioneer of modern computing.", "a.md", 0);
        let b = FactId::compute("Alan Turing", "Pioneer of modern computing.", "a.md", 0);
        assert_eq!(a, b);
        assert!(a.as_str().starts_with("fact-"));
    }

    #[test]
    fn fact_id_differs_on_any_field_change() {
        let base = FactId::compute("X", "Y", "p.md", 0);
        assert_ne!(base, FactId::compute("X2", "Y", "p.md", 0));
        assert_ne!(base, FactId::compute("X", "Y2", "p.md", 0));
        assert_ne!(base, FactId::compute("X", "Y", "q.md", 0));
        assert_ne!(base, FactId::compute("X", "Y", "p.md", 10));
    }

    #[test]
    fn fact_id_delimiter_prevents_concatenation_collision() {
        // Without the NUL delimiter, ("ab", "c", ...) would hash-collide with
        // ("a", "bc", ...). The NUL guards against that.
        let a = FactId::compute("ab", "c", "p.md", 0);
        let b = FactId::compute("a", "bc", "p.md", 0);
        assert_ne!(a, b);
    }

    #[test]
    fn edp_new_computes_consistent_id() {
        let edp = EntityDescriptionPair::new(
            "entity",
            "description",
            vec![],
            anchor("p.md", 0, 20),
            vec![],
        );
        let expected = FactId::compute("entity", "description", "p.md", 0);
        assert_eq!(edp.id, expected);
    }

    #[test]
    fn edp_serializes_omits_empty_tags_and_edges() {
        let edp = EntityDescriptionPair::new(
            "entity",
            "description",
            vec![],
            anchor("p.md", 0, 20),
            vec![],
        );
        let json = serde_json::to_string(&edp).unwrap();
        assert!(!json.contains("tags"), "empty tags list must be skipped");
        assert!(!json.contains("edges"), "empty edges list must be skipped");
    }

    #[test]
    fn edp_jsonl_roundtrip() {
        let original = EntityDescriptionPair::new(
            "Retrieval",
            "BM25 beats chunks at low token budgets.",
            vec![Tag::new("kind", "fact"), Tag::new("topic", "retrieval")],
            anchor("notes.md", 42, 100),
            vec![EdgeRef {
                to: FactId::from_raw("fact-abcdef0123456789"),
                kind: EdgeKind::SharedFile,
            }],
        );
        let line = original.to_jsonl_line().unwrap();
        assert!(!line.contains('\n'), "JSONL line must not contain newlines");
        let parsed = EntityDescriptionPair::from_jsonl_line(&line).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn edp_jsonl_line_tags_are_wire_strings() {
        let edp = EntityDescriptionPair::new(
            "e",
            "d",
            vec![Tag::new("kind", "rule")],
            anchor("p.md", 0, 1),
            vec![],
        );
        let line = edp.to_jsonl_line().unwrap();
        assert!(
            line.contains("\"kind:rule\""),
            "tag should serialize as compact ns:value string, got: {line}"
        );
    }

    #[test]
    fn source_anchor_span_len() {
        let a = anchor("p.md", 10, 40);
        assert_eq!(a.span_len(), 30);
    }

    #[test]
    fn source_anchor_span_len_saturates_on_inverted_range() {
        let mut a = anchor("p.md", 40, 10);
        assert_eq!(a.span_len(), 0);
        a.byte_range = (0, 0);
        assert_eq!(a.span_len(), 0);
    }

    #[test]
    fn edge_kind_serializes_snake_case() {
        let kinds = vec![EdgeKind::MarkdownLink, EdgeKind::SharedFile, EdgeKind::SharedEntity];
        let json = serde_json::to_string(&kinds).unwrap();
        assert!(json.contains("\"markdown_link\""));
        assert!(json.contains("\"shared_file\""));
        assert!(json.contains("\"shared_entity\""));
    }

    #[test]
    fn fact_id_renders_with_fact_prefix_and_16_hex() {
        let id = FactId::compute("x", "y", "z.md", 0);
        let s = id.to_string();
        assert!(s.starts_with("fact-"));
        assert_eq!(s.len(), 5 + 16, "fact-<16 hex chars>");
        assert!(s.chars().skip(5).all(|c| c.is_ascii_hexdigit()));
    }
}
