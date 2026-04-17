//! Phase 25A: typed tag schema.
//!
//! A tag is a `(namespace, value)` pair. The `TagSchema` declares which
//! namespaces exist and which values each namespace admits. Unknown
//! namespaces and unknown values are rejected at compile time — EDPs emitted
//! during compilation must validate against the schema or the compiler logs
//! the mismatch and drops the fact.
//!
//! Tags serialize as compact `"namespace:value"` strings both in the store
//! and in query syntax, matching the user's existing ACE tagging convention.

use anyhow::{anyhow, bail, Result};
use indexmap::IndexMap;
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag {
    pub namespace: String,
    pub value: String,
}

impl Tag {
    pub fn new<N: Into<String>, V: Into<String>>(namespace: N, value: V) -> Self {
        Self {
            namespace: namespace.into(),
            value: value.into(),
        }
    }

    /// Parse the canonical `namespace:value` wire form. The namespace and
    /// value are trimmed; empty segments or missing colon rejected.
    pub fn parse(s: &str) -> Result<Self> {
        let (ns, val) = s
            .split_once(':')
            .ok_or_else(|| anyhow!("Tag '{s}' missing ':' separator"))?;
        let ns = ns.trim();
        let val = val.trim();
        if ns.is_empty() {
            bail!("Tag '{s}' has empty namespace");
        }
        if val.is_empty() {
            bail!("Tag '{s}' has empty value");
        }
        Ok(Self {
            namespace: ns.to_string(),
            value: val.to_string(),
        })
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.value)
    }
}

impl Serialize for Tag {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Tag {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        Tag::parse(&s).map_err(DeError::custom)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TagSchema {
    /// namespace → allowed values. Insertion order preserved for deterministic
    /// emission in stats / show output (Phase 25E).
    #[serde(default)]
    pub namespaces: IndexMap<String, Vec<String>>,
}

impl TagSchema {
    pub fn from_yaml_str(s: &str) -> Result<Self> {
        let schema: Self = serde_yaml::from_str(s)?;
        Ok(schema)
    }

    /// Validate a tag against the schema. Unknown namespace → error. Known
    /// namespace with non-whitelisted value → error.
    pub fn validate(&self, tag: &Tag) -> Result<()> {
        let allowed = self
            .namespaces
            .get(&tag.namespace)
            .ok_or_else(|| anyhow!("Unknown tag namespace '{}'", tag.namespace))?;
        if !allowed.iter().any(|v| v == &tag.value) {
            bail!(
                "Tag '{}:{}' not in schema-allowed values for namespace '{}': [{}]",
                tag.namespace,
                tag.value,
                tag.namespace,
                allowed.join(", ")
            );
        }
        Ok(())
    }

    /// True when the schema has no declared namespaces — treated as "no schema
    /// enforcement" (used before a user has authored `knowledge.yaml`).
    pub fn is_empty(&self) -> bool {
        self.namespaces.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_parse_roundtrip() {
        let t = Tag::parse("kind:rule").unwrap();
        assert_eq!(t.namespace, "kind");
        assert_eq!(t.value, "rule");
        assert_eq!(t.to_string(), "kind:rule");
    }

    #[test]
    fn tag_parse_trims_whitespace_around_segments() {
        let t = Tag::parse("  topic  :  retrieval ").unwrap();
        assert_eq!(t.namespace, "topic");
        assert_eq!(t.value, "retrieval");
    }

    #[test]
    fn tag_parse_rejects_malformed() {
        assert!(Tag::parse("no-colon-here").is_err());
        assert!(Tag::parse(":no-namespace").is_err());
        assert!(Tag::parse("no-value:").is_err());
        assert!(Tag::parse(":").is_err());
    }

    #[test]
    fn tag_parse_allows_colons_in_value() {
        // split_once(':') only splits on the first colon — multi-colon values
        // (e.g. "url:https://x" shape) round-trip fine.
        let t = Tag::parse("url:https://example.com").unwrap();
        assert_eq!(t.namespace, "url");
        assert_eq!(t.value, "https://example.com");
    }

    #[test]
    fn tag_serialize_as_string() {
        let t = Tag::new("kind", "rule");
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"kind:rule\"");
    }

    #[test]
    fn tag_deserialize_from_string() {
        let t: Tag = serde_json::from_str("\"kind:rule\"").unwrap();
        assert_eq!(t, Tag::new("kind", "rule"));
    }

    #[test]
    fn tag_deserialize_rejects_malformed_wire_form() {
        let r: Result<Tag, _> = serde_json::from_str("\"not-a-tag\"");
        assert!(r.is_err());
    }

    #[test]
    fn schema_from_yaml_preserves_namespace_order() {
        let yaml = r#"
namespaces:
  kind: [rule, fact, example]
  topic: [retrieval, tools]
"#;
        let schema = TagSchema::from_yaml_str(yaml).unwrap();
        let names: Vec<_> = schema.namespaces.keys().cloned().collect();
        assert_eq!(names, vec!["kind", "topic"]);
    }

    #[test]
    fn schema_validate_accepts_declared_tag() {
        let yaml = "namespaces:\n  kind: [rule, fact]\n";
        let schema = TagSchema::from_yaml_str(yaml).unwrap();
        assert!(schema.validate(&Tag::new("kind", "rule")).is_ok());
        assert!(schema.validate(&Tag::new("kind", "fact")).is_ok());
    }

    #[test]
    fn schema_validate_rejects_unknown_namespace() {
        let schema = TagSchema::from_yaml_str("namespaces:\n  kind: [rule]\n").unwrap();
        let err = schema
            .validate(&Tag::new("unknown", "whatever"))
            .unwrap_err();
        assert!(err.to_string().contains("Unknown tag namespace"));
    }

    #[test]
    fn schema_validate_rejects_unknown_value() {
        let schema = TagSchema::from_yaml_str("namespaces:\n  kind: [rule, fact]\n").unwrap();
        let err = schema.validate(&Tag::new("kind", "decision")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not in schema-allowed"));
        assert!(msg.contains("kind"));
        assert!(msg.contains("decision"));
    }

    #[test]
    fn schema_is_empty_on_default() {
        let schema = TagSchema::default();
        assert!(schema.is_empty());
    }

    #[test]
    fn schema_ignores_extra_yaml_fields() {
        // User might add comments, metadata, etc. to knowledge.yaml — those
        // must not break the schema load.
        let yaml = r#"
namespaces:
  kind: [rule]
extra_unused_field: whatever
"#;
        assert!(TagSchema::from_yaml_str(yaml).is_ok());
    }
}
