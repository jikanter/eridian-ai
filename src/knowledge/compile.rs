//! Phase 25B: Knowledge compiler.
//!
//! Compiles source files into atomic EDPs via the LLM, then commits them to
//! a `KnowledgeStore` after deterministic gating (AEVS restore-check, Phase
//! 25C) and dedup. Compilation is expensive; query is cheap. That's the
//! "compile once, query forever" premise (Epic 9 Feature 2).
//!
//! Structure:
//!
//! - **Pure core (`commit_candidates`, `line_range_to_bytes`,
//!   `dedupe_candidates`)** — deterministic, testable. Given a list of
//!   LLM-emitted candidates, computes provenance, runs restore-check, dedupes,
//!   and commits. No LLM call; tests inject candidates directly.
//!
//! - **LLM invocation (`llm_extract_candidates`)** — calls out to the current
//!   chat model with a dedicated system prompt + JSON output schema (so
//!   Phase 9A/9B constrains the response shape). Not unit-tested; exercised
//!   by integration runs and the Phase 25 showboat demo.
//!
//! - **Orchestrator (`compile_file`, `compile_files`)** — ties it together.
//!   Consults the manifest for per-source cache hits, runs N sample
//!   augmentations, caches via Phase 10B `StageCache`, and reports results
//!   in a `CompileReport`.
//!
//! Intentionally deferred (not in first ship, documented in the Phase 25
//! roadmap notes): FADER "question speculation" step, edge extraction,
//! parallel per-file compilation, streaming extraction.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cache::StageCache;
use crate::config::{Config, GlobalConfig, Input, Role};
use crate::utils::sha256;

use super::edp::{EntityDescriptionPair, FactId, SourceAnchor};
use super::restore::restore_check;
use super::store::KnowledgeStore;
use super::tags::{Tag, TagSchema};

/// Bumped when the extraction prompt, schema, or candidate shape changes —
/// invalidates `StageCache` entries produced by a previous compiler.
pub const COMPILER_VERSION: &str = "v1";

pub struct CompileOptions {
    /// How many independent extraction runs per file (sample augmentation,
    /// FADER step 3). Results are deduped by `(entity, description)`.
    pub samples: usize,
    /// When true, skip files whose manifest content-hash still matches the
    /// observed hash. False forces full re-extraction of every input.
    pub skip_unchanged: bool,
    /// StageCache TTL for cached per-file extraction results.
    pub cache_ttl_secs: u64,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            samples: 2,
            skip_unchanged: true,
            // Compiled knowledge is reusable for a long time; content-hash
            // invalidation catches real changes without a short TTL.
            cache_ttl_secs: 60 * 60 * 24 * 7, // 7 days
        }
    }
}

/// A pre-provenance fact shape — what the extractor produces before we run
/// restore-check and compute the byte anchor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdpCandidate {
    pub entity: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<Tag>,
    /// 1-indexed inclusive line range the LLM claims this fact came from.
    pub line_range: (usize, usize),
}

#[derive(Debug, Default)]
pub struct CompileReport {
    pub files_processed: usize,
    pub files_skipped: usize,
    pub facts_added: usize,
    pub facts_rejected: usize,
    pub errors: Vec<(String, String)>,
}

impl CompileReport {
    pub fn summary(&self) -> String {
        format!(
            "compile summary: {} file(s) processed, {} skipped; {} facts added, {} rejected by restore-check, {} error(s)",
            self.files_processed,
            self.files_skipped,
            self.facts_added,
            self.facts_rejected,
            self.errors.len()
        )
    }
}

// ============================================================================
// Pure core
// ============================================================================

/// Commit a batch of candidates against a single source file. Removes any
/// previously-persisted facts attributed to `source_path` (the file content
/// may have changed) before appending the new set. Each candidate passes
/// through:
///
/// 1. Provenance synthesis (line range → byte range).
/// 2. EDP construction (computes content-hashed `FactId`).
/// 3. Within-batch dedup by id.
/// 4. AEVS restore-check against `source_content`.
/// 5. `store.append_fact` (which re-checks tag schema when one is present).
///
/// Returns `(added, rejected)`.
pub fn commit_candidates(
    store: &mut KnowledgeStore,
    source_path: &str,
    source_content: &str,
    source_hash: &str,
    candidates: Vec<EdpCandidate>,
) -> Result<(usize, usize)> {
    // A re-compile supersedes the prior facts from this source wholesale.
    store.remove_facts_by_source(source_path);

    let mut added = 0usize;
    let mut rejected = 0usize;
    let mut seen: std::collections::HashSet<FactId> = std::collections::HashSet::new();

    for cand in candidates {
        let byte_range = line_range_to_bytes(source_content, cand.line_range);
        let mut edp = EntityDescriptionPair::new(
            &cand.entity,
            &cand.description,
            cand.tags,
            SourceAnchor {
                path: source_path.to_string(),
                byte_range,
                line_range: cand.line_range,
                content_hash: source_hash.to_string(),
            },
            vec![],
        );

        if !seen.insert(edp.id.clone()) {
            continue; // exact dup within this batch — skip silently
        }

        match restore_check(&edp.description, source_content) {
            Some(outcome) => {
                // Refine anchor to the matched byte span — the LLM's line
                // range is approximate, restore-check tells us where the
                // text actually lives.
                edp.provenance.byte_range = outcome.matched_byte_range;
                match store.append_fact(edp) {
                    Ok(()) => added += 1,
                    Err(e) => {
                        debug!("Dropping fact: {e:#}");
                        rejected += 1;
                    }
                }
            }
            None => {
                rejected += 1;
            }
        }
    }

    if added > 0 {
        store
            .manifest
            .upsert_source(source_path.to_string(), source_hash.to_string(), added);
    }

    Ok((added, rejected))
}

/// Convert a 1-indexed inclusive line range into a half-open byte range
/// into `source`. Clamps to source bounds on out-of-range input (LLMs
/// occasionally hallucinate line numbers; no reason to crash over it).
pub fn line_range_to_bytes(source: &str, line_range: (usize, usize)) -> (usize, usize) {
    let (start_line, end_line) = line_range;
    if start_line == 0 || end_line == 0 || source.is_empty() {
        return (0, source.len());
    }

    let mut line = 1usize;
    let mut byte_start: Option<usize> = None;
    let mut byte_end: Option<usize> = None;

    if line == start_line {
        byte_start = Some(0);
    }

    for (idx, ch) in source.char_indices() {
        if ch == '\n' {
            line += 1;
            if line == start_line && byte_start.is_none() {
                // Byte position right after the newline.
                byte_start = Some(idx + ch.len_utf8());
            }
            if line > end_line {
                byte_end = Some(idx);
                break;
            }
        }
    }

    let byte_start = byte_start.unwrap_or(source.len());
    let byte_end = byte_end.unwrap_or(source.len());
    (byte_start.min(source.len()), byte_end.min(source.len()))
}

/// Dedupe candidates across multiple sample runs by `(entity, description)`.
/// Preserves insertion order so the first-seen form of a fact wins.
pub fn dedupe_candidates(candidates: Vec<EdpCandidate>) -> Vec<EdpCandidate> {
    let mut seen: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    candidates
        .into_iter()
        .filter(|c| seen.insert((c.entity.clone(), c.description.clone())))
        .collect()
}

// ============================================================================
// LLM invocation
// ============================================================================

const EXTRACTION_SYSTEM_PROMPT: &str = "\
You extract atomic knowledge facts (Entity-Description Pairs, or EDPs) from source text.

For each logically atomic fact in the document, emit:
- `entity`: a short noun phrase (3-10 words) identifying the subject of the fact.
- `description`: a single self-contained sentence stating the fact exactly as the source says it. Preserve the source wording when possible.
- `tags`: zero or more tags in `namespace:value` form. Only use tags listed in the provided schema, if any.
- `line_start`, `line_end`: 1-indexed inclusive line numbers where this fact lives in the source.

Rules:
- Each description must be grounded in the source — do not infer beyond what the text says.
- Prefer many small atomic facts over a few broad ones.
- The description should be findable in the source by substring or near-paraphrase.
- If the source has no extractable facts, return {\"facts\": []}.

Return JSON of the form:
{\"facts\": [{\"entity\": \"...\", \"description\": \"...\", \"tags\": [\"kind:fact\"], \"line_start\": 1, \"line_end\": 3}, ...]}";

fn extraction_json_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["facts"],
        "properties": {
            "facts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["entity", "description", "line_start", "line_end"],
                    "properties": {
                        "entity": {"type": "string", "minLength": 1},
                        "description": {"type": "string", "minLength": 1},
                        "tags": {"type": "array", "items": {"type": "string"}},
                        "line_start": {"type": "integer", "minimum": 1},
                        "line_end": {"type": "integer", "minimum": 1}
                    }
                }
            }
        }
    })
}

#[derive(Debug, Deserialize)]
struct LlmFact {
    entity: String,
    description: String,
    #[serde(default)]
    tags: Vec<String>,
    line_start: usize,
    line_end: usize,
}

#[derive(Debug, Deserialize)]
struct LlmResponse {
    #[serde(default)]
    facts: Vec<LlmFact>,
}

/// Run a single extraction pass against the current chat model. The system
/// prompt + output-schema combination force JSON with the expected shape
/// (Phase 9A/9B provider-native structured output kicks in automatically
/// for supporting models).
pub async fn llm_extract_candidates(
    config: &GlobalConfig,
    content: &str,
    source_path: &str,
    schema: &TagSchema,
) -> Result<Vec<EdpCandidate>> {
    let role_prompt = build_role_prompt(schema);
    let mut role = Role::new("knowledge-extractor", &role_prompt);
    role.set_output_schema(Some(extraction_json_schema()));

    let user_input = format!("Source path: {source_path}\n\n---\n{content}\n---");
    let input = Input::from_str(config, &user_input, Some(role));

    let text = input
        .fetch_chat_text()
        .await
        .context("Knowledge-extractor LLM call failed")?;
    parse_llm_response(&text)
}

fn build_role_prompt(schema: &TagSchema) -> String {
    let schema_hint = if schema.is_empty() {
        String::new()
    } else {
        let mut lines = String::from("\n\nTAG SCHEMA (use only these namespaces and values):\n");
        for (ns, values) in &schema.namespaces {
            lines.push_str(&format!("- {ns}: [{}]\n", values.join(", ")));
        }
        lines
    };
    format!("---\n---\n{EXTRACTION_SYSTEM_PROMPT}{schema_hint}")
}

fn parse_llm_response(text: &str) -> Result<Vec<EdpCandidate>> {
    let parsed: LlmResponse = serde_json::from_str(text.trim())
        .with_context(|| format!("Knowledge-extractor returned non-JSON or wrong shape: {text}"))?;
    Ok(parsed
        .facts
        .into_iter()
        .map(|f| EdpCandidate {
            entity: f.entity,
            description: f.description,
            tags: f
                .tags
                .into_iter()
                .filter_map(|s| Tag::parse(&s).ok())
                .collect(),
            line_range: (f.line_start, f.line_end),
        })
        .collect())
}

// ============================================================================
// Orchestration
// ============================================================================

pub async fn compile_file(
    config: &GlobalConfig,
    store: &mut KnowledgeStore,
    path: &Path,
    options: &CompileOptions,
) -> Result<(bool, usize, usize)> {
    // Returns (was_processed, added, rejected). was_processed=false → skipped.
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read source file {}", path.display()))?;
    let hash = sha256(&content);
    let path_str = path.to_string_lossy().to_string();

    if options.skip_unchanged && !store.manifest.needs_recompile(&path_str, &hash) {
        debug!("Skipping unchanged source {}", path.display());
        return Ok((false, 0, 0));
    }

    let cache_key = StageCache::key(
        "knowledge-extractor",
        COMPILER_VERSION,
        &format!("{}\0{}", options.samples, hash),
    );
    let cache = StageCache::new(
        Config::local_path(".cache/knowledge"),
        options.cache_ttl_secs,
    );

    let candidates: Vec<EdpCandidate> = if let Some(cached) = cache.get(&cache_key) {
        debug!("Knowledge cache hit for {}", path.display());
        serde_json::from_str(&cached)
            .with_context(|| format!("Corrupt cache entry {cache_key}"))?
    } else {
        let mut all: Vec<EdpCandidate> = Vec::new();
        for sample in 0..options.samples.max(1) {
            debug!(
                "Extraction sample {}/{} for {}",
                sample + 1,
                options.samples,
                path.display()
            );
            let batch = llm_extract_candidates(config, &content, &path_str, &store.schema).await?;
            all.extend(batch);
        }
        let deduped = dedupe_candidates(all);
        if let Ok(json) = serde_json::to_string(&deduped) {
            if let Err(e) = cache.put(&cache_key, &json) {
                debug!("Failed to cache extraction result: {e}");
            }
        }
        deduped
    };

    let (added, rejected) = commit_candidates(store, &path_str, &content, &hash, candidates)?;
    Ok((true, added, rejected))
}

pub async fn compile_files(
    config: &GlobalConfig,
    store: &mut KnowledgeStore,
    paths: Vec<PathBuf>,
    options: &CompileOptions,
) -> Result<CompileReport> {
    let mut report = CompileReport::default();
    for path in paths {
        match compile_file(config, store, &path, options).await {
            Ok((true, added, rejected)) => {
                report.files_processed += 1;
                report.facts_added += added;
                report.facts_rejected += rejected;
            }
            Ok((false, _, _)) => {
                report.files_skipped += 1;
            }
            Err(e) => {
                report
                    .errors
                    .push((path.display().to_string(), format!("{e:#}")));
            }
        }
    }
    Ok(report)
}

// ============================================================================
// Tests (pure core only — LLM-driven paths are exercised via integration)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::store::KnowledgeStore;
    #[allow(unused_imports)]
    use crate::knowledge::tags::TagSchema;
    use tempfile::tempdir;

    fn cand(entity: &str, description: &str, lines: (usize, usize)) -> EdpCandidate {
        EdpCandidate {
            entity: entity.into(),
            description: description.into(),
            tags: vec![],
            line_range: lines,
        }
    }

    #[test]
    fn line_range_to_bytes_basic() {
        let src = "line 1\nline 2\nline 3\n";
        assert_eq!(line_range_to_bytes(src, (1, 1)), (0, 6)); // "line 1"
        assert_eq!(line_range_to_bytes(src, (2, 2)), (7, 13)); // "line 2"
        assert_eq!(line_range_to_bytes(src, (1, 3)), (0, 20)); // whole first 3 lines
    }

    #[test]
    fn line_range_to_bytes_clamps_oob() {
        let src = "only line\n";
        let (s, e) = line_range_to_bytes(src, (5, 10));
        // Out-of-range: clamps to end-of-source, never panics.
        assert!(s <= src.len());
        assert!(e <= src.len());
    }

    #[test]
    fn line_range_to_bytes_zero_returns_whole_source() {
        let src = "hello";
        assert_eq!(line_range_to_bytes(src, (0, 0)), (0, 5));
    }

    #[test]
    fn dedupe_candidates_removes_duplicates_by_entity_and_description() {
        let cands = vec![
            cand("A", "description one", (1, 1)),
            cand("A", "description one", (5, 5)), // different line range, same content
            cand("B", "description two", (2, 2)),
        ];
        let out = dedupe_candidates(cands);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].entity, "A");
        assert_eq!(out[1].entity, "B");
    }

    #[test]
    fn commit_candidates_adds_grounded_facts_and_rejects_hallucinated() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();

        let source = "\
Retrieval augmented generation grounds LLM output.
BM25 beats vectors at low token budgets in FADER.
Unrelated text about cats.";
        let candidates = vec![
            cand(
                "Retrieval",
                "Retrieval augmented generation grounds LLM output.",
                (1, 1),
            ),
            cand(
                "BM25 efficiency",
                "BM25 beats vectors at low token budgets in FADER.",
                (2, 2),
            ),
            // Hallucinated: not in source at all.
            cand(
                "Hallucination",
                "The emperor penguin migrates across Antarctica each winter.",
                (1, 1),
            ),
        ];

        let (added, rejected) =
            commit_candidates(&mut store, "notes.md", source, "hash-1", candidates).unwrap();
        assert_eq!(added, 2);
        assert_eq!(rejected, 1);
        assert_eq!(store.facts.len(), 2);
    }

    #[test]
    fn commit_candidates_refines_byte_range_to_restore_match() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        let source = "header line\nalpha beta gamma delta\nfooter\n";
        let candidates = vec![cand(
            "middle phrase",
            "alpha beta gamma delta",
            // LLM says lines 1-3 (wrong); restore-check finds the real match.
            (1, 3),
        )];
        let (added, _rejected) =
            commit_candidates(&mut store, "src.md", source, "h", candidates).unwrap();
        assert_eq!(added, 1);
        let fact = &store.facts[0];
        let slice = &source[fact.provenance.byte_range.0..fact.provenance.byte_range.1];
        assert_eq!(slice, "alpha beta gamma delta");
    }

    #[test]
    fn commit_candidates_dedupes_within_batch() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        let source = "alpha beta gamma\n";
        let candidates = vec![
            cand("x", "alpha beta gamma", (1, 1)),
            // Identical entity+description → same FactId → deduped before commit.
            cand("x", "alpha beta gamma", (1, 1)),
        ];
        let (added, rejected) =
            commit_candidates(&mut store, "a.md", source, "h", candidates).unwrap();
        assert_eq!(added, 1);
        assert_eq!(rejected, 0);
        assert_eq!(store.facts.len(), 1);
    }

    #[test]
    fn commit_candidates_replaces_existing_facts_from_same_source() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();

        // First compile: two facts from notes.md.
        let source_v1 = "old fact one.\nold fact two.\n";
        commit_candidates(
            &mut store,
            "notes.md",
            source_v1,
            "hash-1",
            vec![
                cand("one", "old fact one.", (1, 1)),
                cand("two", "old fact two.", (2, 2)),
            ],
        )
        .unwrap();
        assert_eq!(store.facts.len(), 2);

        // Recompile with changed content → old facts go, new facts replace.
        let source_v2 = "new fact only.\n";
        commit_candidates(
            &mut store,
            "notes.md",
            source_v2,
            "hash-2",
            vec![cand("new", "new fact only.", (1, 1))],
        )
        .unwrap();
        assert_eq!(store.facts.len(), 1);
        assert_eq!(store.facts[0].entity, "new");
        assert_eq!(
            store
                .manifest
                .sources
                .get("notes.md")
                .unwrap()
                .content_hash,
            "hash-2"
        );
    }

    #[test]
    fn commit_candidates_touches_manifest_source_only_when_facts_added() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kb");
        let mut store = KnowledgeStore::create(&path, "kb").unwrap();
        let source = "real content here.\n";
        // All candidates hallucinated — zero restore.
        let candidates = vec![cand("bad", "fabricated claim", (1, 1))];
        let (added, rejected) =
            commit_candidates(&mut store, "a.md", source, "h", candidates).unwrap();
        assert_eq!(added, 0);
        assert_eq!(rejected, 1);
        assert!(
            !store.manifest.sources.contains_key("a.md"),
            "manifest source entry should not be created when no facts survived"
        );
    }

    #[test]
    fn parse_llm_response_accepts_valid_json() {
        let text = r#"{"facts":[{"entity":"E","description":"D","tags":["kind:rule"],"line_start":1,"line_end":2}]}"#;
        let out = parse_llm_response(text).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entity, "E");
        assert_eq!(out[0].description, "D");
        assert_eq!(out[0].tags.len(), 1);
        assert_eq!(out[0].tags[0].to_string(), "kind:rule");
        assert_eq!(out[0].line_range, (1, 2));
    }

    #[test]
    fn parse_llm_response_accepts_empty_facts() {
        let out = parse_llm_response(r#"{"facts":[]}"#).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn parse_llm_response_drops_malformed_tags_silently() {
        let text = r#"{"facts":[{"entity":"E","description":"D","tags":["valid:tag","broken-no-colon"],"line_start":1,"line_end":1}]}"#;
        let out = parse_llm_response(text).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tags.len(), 1);
        assert_eq!(out[0].tags[0].to_string(), "valid:tag");
    }

    #[test]
    fn parse_llm_response_rejects_non_json() {
        assert!(parse_llm_response("not json").is_err());
        assert!(parse_llm_response("").is_err());
    }

    #[test]
    fn build_role_prompt_without_schema_omits_schema_section() {
        let s = build_role_prompt(&TagSchema::default());
        assert!(!s.contains("TAG SCHEMA"));
    }

    #[test]
    fn build_role_prompt_with_schema_lists_namespaces_and_values() {
        let schema = TagSchema::from_yaml_str(
            "namespaces:\n  kind: [rule, fact]\n  topic: [retrieval]\n",
        )
        .unwrap();
        let s = build_role_prompt(&schema);
        assert!(s.contains("TAG SCHEMA"));
        assert!(s.contains("kind:"));
        assert!(s.contains("rule"));
        assert!(s.contains("fact"));
        assert!(s.contains("topic:"));
        assert!(s.contains("retrieval"));
    }

    #[test]
    fn compile_report_summary_mentions_key_counts() {
        let r = CompileReport {
            files_processed: 3,
            files_skipped: 1,
            facts_added: 42,
            facts_rejected: 5,
            errors: vec![("x.md".into(), "oops".into())],
        };
        let s = r.summary();
        assert!(s.contains("3"));
        assert!(s.contains("1"));
        assert!(s.contains("42"));
        assert!(s.contains("5"));
    }
}
