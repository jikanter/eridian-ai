//! Phase 25D: Compiled Knowledge Base storage.
//!
//! On-disk layout for a single KB, rooted at `<dir>/`:
//!
//! ```text
//! <dir>/
//!   manifest.yaml    — KB metadata + per-source content hashes (authoritative)
//!   facts.jsonl      — one EDP per line (authoritative)
//!   edges.jsonl      — one EdgeEntry per line (authoritative graph)
//!   knowledge.yaml   — optional tag schema; when absent, validation is skipped
//! ```
//!
//! Mutation is **append/patch only** at this layer (Phase 27A prescribes the
//! same discipline for the higher-level evolution loop). `save()` rewrites
//! the whole directory via write-to-tmp + rename so partial writes never
//! leave a corrupted KB on disk.
//!
//! Re-compilation skips unchanged source files by consulting
//! `Manifest::needs_recompile(path, content_hash)` — the heavy LLM-driven
//! extraction in Phase 25B only runs for files whose content hash changed.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use super::edp::{EdgeKind, EntityDescriptionPair, FactId};
use super::tags::TagSchema;

pub const MANIFEST_FILE: &str = "manifest.yaml";
pub const FACTS_FILE: &str = "facts.jsonl";
pub const EDGES_FILE: &str = "edges.jsonl";
pub const SCHEMA_FILE: &str = "knowledge.yaml";
pub const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: u32,
    #[serde(default)]
    pub sources: IndexMap<String, SourceEntry>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub fact_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEntry {
    pub content_hash: String,
    pub fact_count: usize,
}

impl Manifest {
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            name: name.into(),
            version: CURRENT_VERSION,
            sources: IndexMap::new(),
            created_at: now.clone(),
            updated_at: now,
            fact_count: 0,
        }
    }

    /// True when the KB does not yet know about this source, or when its
    /// recorded content hash differs from the one observed on disk.
    pub fn needs_recompile(&self, path: &str, content_hash: &str) -> bool {
        match self.sources.get(path) {
            Some(entry) => entry.content_hash != content_hash,
            None => true,
        }
    }

    pub fn upsert_source(&mut self, path: impl Into<String>, hash: impl Into<String>, fact_count: usize) {
        self.sources.insert(
            path.into(),
            SourceEntry {
                content_hash: hash.into(),
                fact_count,
            },
        );
        self.touch();
    }

    pub fn remove_source(&mut self, path: &str) {
        if self.sources.shift_remove(path).is_some() {
            self.touch();
        }
    }

    fn touch(&mut self) {
        self.updated_at = Utc::now().to_rfc3339();
    }
}

/// A persisted graph edge. EDPs themselves carry an `edges` field for ad-hoc
/// construction, but the store treats `edges.jsonl` as the authoritative
/// source — the EDP field is not read back on load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeEntry {
    pub from: FactId,
    pub to: FactId,
    pub kind: EdgeKind,
}

#[derive(Debug)]
pub struct KnowledgeStore {
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub schema: TagSchema,
    pub facts: Vec<EntityDescriptionPair>,
    pub edges: Vec<EdgeEntry>,
}

impl KnowledgeStore {
    /// Create a fresh store on disk. Fails if `dir` already contains a
    /// manifest (refuse to overwrite without an explicit caller action).
    pub fn create(dir: impl Into<PathBuf>, name: impl Into<String>) -> Result<Self> {
        let dir = dir.into();
        let manifest_path = dir.join(MANIFEST_FILE);
        if manifest_path.exists() {
            bail!(
                "KB already exists at {}. Delete it first or load with KnowledgeStore::load.",
                dir.display()
            );
        }
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create KB dir {}", dir.display()))?;
        let store = Self {
            dir,
            manifest: Manifest::new(name),
            schema: TagSchema::default(),
            facts: Vec::new(),
            edges: Vec::new(),
        };
        store.save()?;
        Ok(store)
    }

    pub fn load(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        let manifest_yaml = fs::read_to_string(dir.join(MANIFEST_FILE)).with_context(|| {
            format!("Failed to read manifest at {}", dir.join(MANIFEST_FILE).display())
        })?;
        let manifest: Manifest = serde_yaml::from_str(&manifest_yaml)
            .context("Failed to parse manifest.yaml")?;
        if manifest.version != CURRENT_VERSION {
            bail!(
                "KB version {} not supported (this build expects version {CURRENT_VERSION})",
                manifest.version
            );
        }

        let schema = read_schema(&dir.join(SCHEMA_FILE))?;
        let facts = read_jsonl_facts(&dir.join(FACTS_FILE))?;
        let edges = read_jsonl_edges(&dir.join(EDGES_FILE))?;

        Ok(Self {
            dir,
            manifest,
            schema,
            facts,
            edges,
        })
    }

    /// Write the store to disk atomically. Each file is written to `.tmp`
    /// then renamed, so partial crashes never leave a half-written KB.
    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("Failed to create KB dir {}", self.dir.display()))?;

        atomic_write(&self.dir.join(MANIFEST_FILE), |w| {
            w.write_all(serde_yaml::to_string(&self.manifest)?.as_bytes())?;
            Ok(())
        })?;

        atomic_write(&self.dir.join(FACTS_FILE), |w| {
            for fact in &self.facts {
                // Strip transient `edges` from EDPs before persisting — the
                // authoritative graph lives in edges.jsonl.
                let mut persisted = fact.clone();
                persisted.edges.clear();
                writeln!(w, "{}", persisted.to_jsonl_line()?)?;
            }
            Ok(())
        })?;

        atomic_write(&self.dir.join(EDGES_FILE), |w| {
            for edge in &self.edges {
                writeln!(w, "{}", serde_json::to_string(edge)?)?;
            }
            Ok(())
        })?;

        if !self.schema.is_empty() {
            atomic_write(&self.dir.join(SCHEMA_FILE), |w| {
                w.write_all(serde_yaml::to_string(&self.schema)?.as_bytes())?;
                Ok(())
            })?;
        }

        Ok(())
    }

    /// Append a fact. Validates against the tag schema (if one is loaded) and
    /// rejects duplicates by id.
    pub fn append_fact(&mut self, fact: EntityDescriptionPair) -> Result<()> {
        if !self.schema.is_empty() {
            for tag in &fact.tags {
                self.schema
                    .validate(tag)
                    .with_context(|| format!("Tag validation failed for fact {}", fact.id))?;
            }
        }
        if self.facts.iter().any(|f| f.id == fact.id) {
            bail!("Fact id {} already present in KB", fact.id);
        }
        self.facts.push(fact);
        self.manifest.fact_count = self.facts.len();
        self.manifest.touch();
        Ok(())
    }

    /// Append an edge. Silently skips if the exact `(from, to, kind)` triple
    /// is already recorded — edge extraction (Phase 25B) is noisy and we
    /// don't want duplicates inflating the graph.
    pub fn append_edge(&mut self, edge: EdgeEntry) {
        if self.edges.iter().any(|e| e == &edge) {
            return;
        }
        self.edges.push(edge);
        self.manifest.touch();
    }

    /// Drop every fact (and its outbound edges) that was extracted from
    /// `source_path`. Used on recompile when a source file changed.
    pub fn remove_facts_by_source(&mut self, source_path: &str) -> usize {
        let before = self.facts.len();
        let mut dropped_ids: Vec<FactId> = Vec::new();
        self.facts.retain(|f| {
            if f.provenance.path == source_path {
                dropped_ids.push(f.id.clone());
                false
            } else {
                true
            }
        });
        let removed = before - self.facts.len();
        if removed > 0 {
            self.edges
                .retain(|e| !dropped_ids.contains(&e.from) && !dropped_ids.contains(&e.to));
            self.manifest.remove_source(source_path);
            self.manifest.fact_count = self.facts.len();
        }
        removed
    }

    /// Build a `{tag → [fact_ids]}` inverted index from the current facts.
    /// Not persisted — rebuilt on demand. Key type is the serialized
    /// `"ns:value"` form so callers can look up by tag literal without first
    /// reconstructing a `Tag` struct.
    pub fn tag_index(&self) -> IndexMap<String, Vec<FactId>> {
        let mut idx: IndexMap<String, Vec<FactId>> = IndexMap::new();
        for fact in &self.facts {
            for tag in &fact.tags {
                idx.entry(tag.to_string()).or_default().push(fact.id.clone());
            }
        }
        idx
    }

    /// Return every edge whose `from` is `fact_id`. 1-hop outbound adjacency;
    /// Phase 26B's graph walk consumes this.
    pub fn outbound_edges(&self, fact_id: &FactId) -> Vec<&EdgeEntry> {
        self.edges.iter().filter(|e| &e.from == fact_id).collect()
    }

    /// Install a tag schema and persist it on the next `save()`.
    pub fn set_schema(&mut self, schema: TagSchema) {
        self.schema = schema;
        self.manifest.touch();
    }
}

// ---------- private I/O helpers ----------

fn atomic_write<F>(path: &Path, write: F) -> Result<()>
where
    F: FnOnce(&mut std::io::BufWriter<fs::File>) -> Result<()>,
{
    let tmp = path.with_extension(
        path.extension()
            .map(|e| format!("{}.tmp", e.to_string_lossy()))
            .unwrap_or_else(|| "tmp".to_string()),
    );
    {
        let file = fs::File::create(&tmp)
            .with_context(|| format!("Failed to open {} for writing", tmp.display()))?;
        let mut w = std::io::BufWriter::new(file);
        write(&mut w)?;
        w.flush()?;
    }
    fs::rename(&tmp, path).with_context(|| {
        format!("Failed to atomically rename {} -> {}", tmp.display(), path.display())
    })?;
    Ok(())
}

fn read_schema(path: &Path) -> Result<TagSchema> {
    if !path.exists() {
        return Ok(TagSchema::default());
    }
    let yaml = fs::read_to_string(path)
        .with_context(|| format!("Failed to read schema at {}", path.display()))?;
    TagSchema::from_yaml_str(&yaml)
        .with_context(|| format!("Failed to parse schema at {}", path.display()))
}

fn read_jsonl_facts(path: &Path) -> Result<Vec<EntityDescriptionPair>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let mut facts = Vec::new();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let fact = EntityDescriptionPair::from_jsonl_line(&line)
            .with_context(|| format!("Malformed JSONL at {}:{}", path.display(), i + 1))?;
        facts.push(fact);
    }
    Ok(facts)
}

fn read_jsonl_edges(path: &Path) -> Result<Vec<EdgeEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let mut edges = Vec::new();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let edge: EdgeEntry = serde_json::from_str(&line)
            .with_context(|| format!("Malformed JSONL at {}:{}", path.display(), i + 1))?;
        edges.push(edge);
    }
    Ok(edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::edp::{EdgeKind, EntityDescriptionPair, SourceAnchor};
    use crate::knowledge::tags::Tag;
    use tempfile::tempdir;

    fn sample_edp(path: &str, entity: &str, tags: Vec<Tag>) -> EntityDescriptionPair {
        EntityDescriptionPair::new(
            entity,
            format!("{entity} description body."),
            tags,
            SourceAnchor {
                path: path.into(),
                byte_range: (0, 30),
                line_range: (1, 2),
                content_hash: "srchash".into(),
            },
            vec![],
        )
    }

    #[test]
    fn create_then_load_roundtrip_empty_kb() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let store = KnowledgeStore::create(&path, "test-kb").unwrap();
        assert_eq!(store.manifest.name, "test-kb");
        assert_eq!(store.manifest.version, CURRENT_VERSION);
        assert!(store.facts.is_empty());
        assert!(store.edges.is_empty());

        let reloaded = KnowledgeStore::load(&path).unwrap();
        assert_eq!(reloaded.manifest.name, "test-kb");
        assert!(reloaded.facts.is_empty());
    }

    #[test]
    fn create_refuses_to_clobber_existing_kb() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        KnowledgeStore::create(&path, "once").unwrap();
        let err = KnowledgeStore::create(&path, "twice").unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn append_fact_then_save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        store
            .append_fact(sample_edp("notes.md", "alpha", vec![]))
            .unwrap();
        store
            .append_fact(sample_edp("notes.md", "beta", vec![]))
            .unwrap();
        store.manifest.upsert_source("notes.md", "srchash", 2);
        store.save().unwrap();

        let loaded = KnowledgeStore::load(&path).unwrap();
        assert_eq!(loaded.facts.len(), 2);
        assert_eq!(loaded.manifest.fact_count, 2);
        assert_eq!(loaded.manifest.sources.get("notes.md").unwrap().fact_count, 2);
    }

    #[test]
    fn append_fact_rejects_duplicate_id() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        let edp = sample_edp("a.md", "e", vec![]);
        store.append_fact(edp.clone()).unwrap();
        let err = store.append_fact(edp).unwrap_err();
        assert!(err.to_string().contains("already present"));
    }

    #[test]
    fn append_fact_enforces_schema_when_present() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        store.set_schema(
            TagSchema::from_yaml_str("namespaces:\n  kind: [rule]\n").unwrap(),
        );

        // Accepted: known tag.
        assert!(store
            .append_fact(sample_edp("a.md", "e", vec![Tag::new("kind", "rule")]))
            .is_ok());

        // Rejected: unknown namespace.
        let err = store
            .append_fact(sample_edp(
                "b.md",
                "f",
                vec![Tag::new("topic", "anything")],
            ))
            .unwrap_err();
        // anyhow wraps with context — search the whole chain.
        let chain_msg = format!("{err:#}");
        assert!(
            chain_msg.contains("Unknown tag namespace"),
            "expected chain to include the root cause, got: {chain_msg}"
        );

        // Rejected: unknown value in known namespace.
        let err = store
            .append_fact(sample_edp(
                "c.md",
                "g",
                vec![Tag::new("kind", "heresy")],
            ))
            .unwrap_err();
        let chain_msg = format!("{err:#}");
        assert!(
            chain_msg.contains("not in schema-allowed"),
            "expected chain to include the root cause, got: {chain_msg}"
        );
    }

    #[test]
    fn append_fact_without_schema_does_not_validate_tags() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        // Empty schema → tags pass through. Phase 25A: unenforced KBs still work.
        assert!(store
            .append_fact(sample_edp(
                "a.md",
                "e",
                vec![Tag::new("arbitrary", "anything")],
            ))
            .is_ok());
    }

    #[test]
    fn append_edge_dedups() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        let e1 = sample_edp("a.md", "a", vec![]);
        let e2 = sample_edp("a.md", "b", vec![]);
        let edge = EdgeEntry {
            from: e1.id.clone(),
            to: e2.id.clone(),
            kind: EdgeKind::SharedFile,
        };
        store.append_edge(edge.clone());
        store.append_edge(edge.clone());
        assert_eq!(store.edges.len(), 1);
    }

    #[test]
    fn save_strips_runtime_edges_from_edp_before_persisting() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        // Construct an EDP that carries a transient edge in its struct.
        let edp = EntityDescriptionPair::new(
            "runtime-edge",
            "description",
            vec![],
            SourceAnchor {
                path: "a.md".into(),
                byte_range: (0, 1),
                line_range: (1, 1),
                content_hash: "x".into(),
            },
            vec![crate::knowledge::edp::EdgeRef {
                to: FactId::from_raw("fact-somewhere"),
                kind: EdgeKind::MarkdownLink,
            }],
        );
        store.append_fact(edp).unwrap();
        store.save().unwrap();

        let facts_raw = fs::read_to_string(path.join(FACTS_FILE)).unwrap();
        assert!(
            !facts_raw.contains("edges"),
            "EDP.edges must not be persisted to facts.jsonl (edges.jsonl is authoritative)"
        );
    }

    #[test]
    fn remove_facts_by_source_drops_facts_and_outbound_edges() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        let a = sample_edp("notes.md", "alpha", vec![]);
        let b = sample_edp("other.md", "beta", vec![]);
        store.append_fact(a.clone()).unwrap();
        store.append_fact(b.clone()).unwrap();
        store.manifest.upsert_source("notes.md", "hash-a", 1);
        store.manifest.upsert_source("other.md", "hash-b", 1);
        store.append_edge(EdgeEntry {
            from: a.id.clone(),
            to: b.id.clone(),
            kind: EdgeKind::MarkdownLink,
        });

        let removed = store.remove_facts_by_source("notes.md");
        assert_eq!(removed, 1);
        assert_eq!(store.facts.len(), 1);
        assert_eq!(store.facts[0].id, b.id);
        assert!(store.edges.is_empty(), "edge touching dropped fact must go");
        assert!(
            !store.manifest.sources.contains_key("notes.md"),
            "manifest source entry must be removed"
        );
    }

    #[test]
    fn manifest_needs_recompile_on_new_or_changed_source() {
        let mut m = Manifest::new("kb");
        assert!(m.needs_recompile("x.md", "hash1"), "new source always needs compile");
        m.upsert_source("x.md", "hash1", 0);
        assert!(!m.needs_recompile("x.md", "hash1"));
        assert!(m.needs_recompile("x.md", "hash2"));
    }

    #[test]
    fn tag_index_groups_facts_by_serialized_tag() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        store
            .append_fact(sample_edp("a.md", "A", vec![Tag::new("kind", "rule")]))
            .unwrap();
        store
            .append_fact(sample_edp(
                "b.md",
                "B",
                vec![Tag::new("kind", "rule"), Tag::new("topic", "retrieval")],
            ))
            .unwrap();
        let idx = store.tag_index();
        assert_eq!(idx.get("kind:rule").map(|v| v.len()), Some(2));
        assert_eq!(idx.get("topic:retrieval").map(|v| v.len()), Some(1));
    }

    #[test]
    fn outbound_edges_returns_only_matching_from() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        let a = sample_edp("a.md", "A", vec![]);
        let b = sample_edp("a.md", "B", vec![]);
        let c = sample_edp("a.md", "C", vec![]);
        store.append_fact(a.clone()).unwrap();
        store.append_fact(b.clone()).unwrap();
        store.append_fact(c.clone()).unwrap();
        store.append_edge(EdgeEntry {
            from: a.id.clone(),
            to: b.id.clone(),
            kind: EdgeKind::SharedFile,
        });
        store.append_edge(EdgeEntry {
            from: a.id.clone(),
            to: c.id.clone(),
            kind: EdgeKind::SharedFile,
        });
        store.append_edge(EdgeEntry {
            from: b.id.clone(),
            to: c.id.clone(),
            kind: EdgeKind::SharedFile,
        });

        let a_out = store.outbound_edges(&a.id);
        assert_eq!(a_out.len(), 2);
        assert!(a_out.iter().all(|e| e.from == a.id));
    }

    #[test]
    fn save_load_roundtrip_preserves_tag_schema() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        store.set_schema(
            TagSchema::from_yaml_str("namespaces:\n  kind: [rule, fact]\n  topic: [retrieval]\n")
                .unwrap(),
        );
        store.save().unwrap();

        let loaded = KnowledgeStore::load(&path).unwrap();
        assert!(loaded
            .schema
            .validate(&Tag::new("kind", "rule"))
            .is_ok());
        assert!(loaded
            .schema
            .validate(&Tag::new("kind", "unknown"))
            .is_err());
    }

    #[test]
    fn load_rejects_incompatible_version() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        fs::create_dir_all(&path).unwrap();
        let bad_manifest = serde_yaml::to_string(&Manifest {
            name: "x".into(),
            version: 99, // future version
            sources: IndexMap::new(),
            created_at: "now".into(),
            updated_at: "now".into(),
            fact_count: 0,
        })
        .unwrap();
        fs::write(path.join(MANIFEST_FILE), bad_manifest).unwrap();

        let err = KnowledgeStore::load(&path).unwrap_err();
        assert!(err.to_string().contains("version"));
    }

    #[test]
    fn atomic_write_leaves_no_tmp_on_success() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        store
            .append_fact(sample_edp("a.md", "A", vec![]))
            .unwrap();
        store.save().unwrap();

        let entries: Vec<_> = fs::read_dir(&path)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            entries.iter().all(|name| !name.ends_with(".tmp")),
            "no .tmp files should remain after save: {entries:?}"
        );
    }
}
