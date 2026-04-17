//! Phase 27B: ACE generation/reflection/curation loop.
//!
//! The knowledge base produced by Phase 25 (compilation) drifts over time —
//! source files change, queries miss, extraction errors persist. The ACE
//! paper (arXiv 2510.04618) names the anti-pattern of rewriting the KB
//! wholesale to fix this; the paper's prescription is a three-role loop:
//!
//! 1. **Generator** — produces the initial facts (Phase 25B compilation).
//! 2. **Reflector** — reads traces / failure cases, emits *candidate*
//!    patches (new facts, fixes to existing ones, deprecations).
//! 3. **Curator** — accepts or rejects each candidate before the
//!    append/patch-only API (Phase 27A) commits it.
//!
//! Both Reflector and Curator are user-customizable via role frontmatter
//! (`ace_role: reflector | curator`). The defaults below ship in-box.
//!
//! Both commands are explicit subcommands — no background daemon, no
//! automatic loops. Cost-conscious: only runs when the user asks.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{GlobalConfig, Input, Role};

use super::cli::kb_dir;
use super::edp::{EntityDescriptionPair, FactId, SourceAnchor};
use super::store::{FactPatch, KnowledgeStore};
use super::tags::Tag;

// ---------- types ----------

/// One candidate mutation proposed by the Reflector and reviewed by the
/// Curator. Mirrors the append/patch API shape so acceptance is a one-to-one
/// dispatch with no reinterpretation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Candidate {
    /// Add a new atomic fact. Source anchor is synthetic (no new source
    /// file required), so the Curator should prefer patches against
    /// existing grounded facts unless the Reflector supplies one.
    Append {
        entity: String,
        description: String,
        #[serde(default)]
        tags: Vec<String>,
        #[serde(default)]
        source_path: Option<String>,
        #[serde(default)]
        reason: Option<String>,
    },
    /// Modify an existing fact's description and/or tags.
    Patch {
        id: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        tags: Option<Vec<String>>,
        #[serde(default)]
        reason: Option<String>,
    },
    /// Mark a fact as superseded.
    Deprecate {
        id: String,
        #[serde(default)]
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CandidateSet {
    pub candidates: Vec<Candidate>,
}

// ---------- system prompts (defaults) ----------

const DEFAULT_REFLECTOR_PROMPT: &str = "\
You are the Reflector for a knowledge base. You read retrieval traces (queries
that returned no results, or returned the wrong facts) and the current KB
summary, and you propose *candidate* patches that a human-supervised Curator
will review before any change lands.

Emit JSON of the form:
{\"candidates\": [
  {\"op\": \"append\", \"entity\": \"...\", \"description\": \"...\", \"tags\": [\"kind:fact\"], \"reason\": \"why add\"},
  {\"op\": \"patch\", \"id\": \"fact-abc123\", \"description\": \"corrected text\", \"reason\": \"why patch\"},
  {\"op\": \"deprecate\", \"id\": \"fact-def456\", \"reason\": \"why deprecate\"}
]}

Rules:
- Prefer small, atomic candidates (one fact per candidate).
- Always include a short `reason` — it ends up in the revision log.
- Never invent fact ids; only reference ids that appear in the input.
- Do not restate facts already present verbatim.";

const DEFAULT_CURATOR_PROMPT: &str = "\
You are the Curator for a knowledge base. The Reflector has proposed a list of
candidate patches. For each candidate, decide `accept` or `reject`.

Emit JSON of the form:
{\"decisions\": [
  {\"index\": 0, \"decision\": \"accept\", \"reason\": \"matches existing facts and fills a gap\"},
  {\"index\": 1, \"decision\": \"reject\", \"reason\": \"duplicates fact-abc\"}
]}

Rules:
- Index is 0-based into the candidate list you were shown.
- One decision per candidate (skip none; `reject` is allowed and common).
- Be conservative: accept only candidates that are clearly correct and useful.";

// ---------- orchestration ----------

/// `--knowledge-reflect <kb>`: run the Reflector role over the current state
/// of the KB and emit a candidate set to stdout. Deterministic output
/// shape — downstream tools (including `--knowledge-curate`) consume it.
pub async fn run_reflect(
    config: &GlobalConfig,
    kb_name: &str,
    trace_file: Option<&str>,
) -> Result<()> {
    let dir = kb_dir(kb_name);
    if !dir.exists() {
        bail!("Unknown knowledge base: {kb_name}");
    }
    let store = KnowledgeStore::load(&dir)?;

    let role = resolve_ace_role(config, "reflector", DEFAULT_REFLECTOR_PROMPT)?;
    let prompt = build_reflector_input(&store, trace_file)?;
    let mut role = role;
    role.set_output_schema(Some(reflector_schema()));
    let input = Input::from_str(config, &prompt, Some(role));
    let text = input
        .fetch_chat_text()
        .await
        .context("Reflector role call failed")?;
    let set: CandidateSet = parse_candidate_set(&text)?;
    println!("{}", serde_json::to_string_pretty(&set)?);
    Ok(())
}

/// `--knowledge-curate <kb> [--candidates <file>]`: feed candidates through
/// the Curator role and apply accepted ones via the Phase 27A mutation API.
/// If `--candidates` is not given, the Curator is run against a freshly
/// generated set from the Reflector — the one-shot "reflect then curate"
/// workflow for users who don't need to eyeball the intermediate output.
pub async fn run_curate(
    config: &GlobalConfig,
    kb_name: &str,
    candidates_file: Option<&str>,
    trace_file: Option<&str>,
) -> Result<()> {
    let dir = kb_dir(kb_name);
    if !dir.exists() {
        bail!("Unknown knowledge base: {kb_name}");
    }
    let mut store = KnowledgeStore::load(&dir)?;

    let set = match candidates_file {
        Some(path) => {
            let json = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read candidates file {path}"))?;
            parse_candidate_set(&json)?
        }
        None => {
            let role = resolve_ace_role(config, "reflector", DEFAULT_REFLECTOR_PROMPT)?;
            let mut role = role;
            role.set_output_schema(Some(reflector_schema()));
            let prompt = build_reflector_input(&store, trace_file)?;
            let input = Input::from_str(config, &prompt, Some(role));
            let text = input
                .fetch_chat_text()
                .await
                .context("Reflector role call failed")?;
            parse_candidate_set(&text)?
        }
    };

    if set.candidates.is_empty() {
        eprintln!("curate: no candidates to review.");
        return Ok(());
    }

    let role = resolve_ace_role(config, "curator", DEFAULT_CURATOR_PROMPT)?;
    let mut role = role;
    role.set_output_schema(Some(curator_schema()));
    let curator_prompt = build_curator_input(&set)?;
    let input = Input::from_str(config, &curator_prompt, Some(role));
    let text = input
        .fetch_chat_text()
        .await
        .context("Curator role call failed")?;
    let decisions = parse_curator_decisions(&text, set.candidates.len())?;

    let mut accepted = 0usize;
    let mut rejected = 0usize;
    let mut errors = 0usize;
    for (idx, decision) in decisions.iter().enumerate() {
        let cand = match set.candidates.get(decision.index) {
            Some(c) => c,
            None => {
                errors += 1;
                continue;
            }
        };
        if decision.decision != "accept" {
            rejected += 1;
            continue;
        }
        match apply_candidate(&mut store, cand, idx) {
            Ok(()) => accepted += 1,
            Err(e) => {
                eprintln!("curate: candidate {idx} failed to apply: {e:#}");
                errors += 1;
            }
        }
    }
    store.save()?;

    println!(
        "curate summary: {accepted} accepted, {rejected} rejected, {errors} error(s)"
    );
    Ok(())
}

// ---------- helpers ----------

/// Resolve a user-defined ACE role by name, falling back to a built-in
/// default role with the shipped system prompt. Looks through
/// `Config::all_roles()` for one whose `ace_role` frontmatter matches
/// `ace_role_kind` (either "reflector" or "curator").
fn resolve_ace_role(
    config: &GlobalConfig,
    ace_role_kind: &str,
    default_prompt: &str,
) -> Result<Role> {
    let all = crate::config::Config::all_roles();
    for role in &all {
        // We accept any role whose name contains the literal ace role kind
        // as a suffix. A user sets `ace_role: reflector` in frontmatter and
        // names the role file `my-reflector.md`; we don't need a dedicated
        // metadata lookup — the suffix convention is enough to identify
        // them, and keeps the role-level API narrow.
        if role.name().ends_with(&format!("-{ace_role_kind}")) {
            return Ok(role.clone());
        }
    }
    // No user-defined role → construct an anonymous one from the default.
    // We still want the config's default model to take effect, so resolve
    // a fresh extracted role and overwrite its prompt.
    let mut role = config.read().extract_role();
    // The Role's prompt is the system text; our defaults are pure system
    // instructions so we just set them as prompt.
    set_role_prompt(&mut role, default_prompt);
    Ok(role)
}

/// Helper — `Role::prompt` isn't publicly mutable; build a new Role with
/// the desired prompt and copy across fields we care about.
fn set_role_prompt(role: &mut Role, prompt: &str) {
    // Roles expose no prompt setter, so we reconstruct via `Role::new`.
    let mut fresh = Role::new(role.name(), prompt);
    fresh.sync(role);
    *role = fresh;
}

fn build_reflector_input(
    store: &KnowledgeStore,
    trace_file: Option<&str>,
) -> Result<String> {
    let mut buf = String::new();
    buf.push_str(&format!("Knowledge base: {}\n\n", store.manifest.name));
    buf.push_str(&format!(
        "Current fact count: {} ({} deprecated)\n\n",
        store.facts.iter().filter(|f| !f.deprecated).count(),
        store.facts.iter().filter(|f| f.deprecated).count(),
    ));
    // Show a sampling of ids so the Reflector can reference them.
    buf.push_str("Existing fact ids (first 50):\n");
    for fact in store.facts.iter().take(50) {
        buf.push_str(&format!(
            "- {} entity={:?}{}\n",
            fact.id,
            fact.entity,
            if fact.deprecated { " [deprecated]" } else { "" },
        ));
    }
    if store.facts.len() > 50 {
        buf.push_str(&format!("... ({} more)\n", store.facts.len() - 50));
    }
    buf.push('\n');

    if let Some(path) = trace_file {
        let trace = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read trace file {path}"))?;
        buf.push_str("---\nRetrieval trace (JSONL):\n");
        buf.push_str(&trace);
    } else {
        buf.push_str(
            "No retrieval trace supplied. Propose patches based on the KB summary alone — e.g., obvious gaps or duplicates by id.\n",
        );
    }
    Ok(buf)
}

fn build_curator_input(set: &CandidateSet) -> Result<String> {
    let mut buf = String::from("Candidates from the Reflector:\n\n");
    for (idx, c) in set.candidates.iter().enumerate() {
        buf.push_str(&format!(
            "[{idx}] {}\n",
            serde_json::to_string(c).unwrap_or_default()
        ));
    }
    Ok(buf)
}

fn parse_candidate_set(text: &str) -> Result<CandidateSet> {
    let parsed: CandidateSet = serde_json::from_str(text.trim())
        .with_context(|| format!("Reflector returned non-JSON or wrong shape: {text}"))?;
    Ok(parsed)
}

#[derive(Debug, Deserialize)]
pub struct CuratorDecision {
    pub index: usize,
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CuratorResponse {
    #[serde(default)]
    decisions: Vec<CuratorDecision>,
}

fn parse_curator_decisions(text: &str, n_candidates: usize) -> Result<Vec<CuratorDecision>> {
    let parsed: CuratorResponse = serde_json::from_str(text.trim())
        .with_context(|| format!("Curator returned non-JSON or wrong shape: {text}"))?;
    if parsed.decisions.len() > n_candidates {
        bail!(
            "Curator returned {} decisions for {n_candidates} candidates",
            parsed.decisions.len()
        );
    }
    Ok(parsed.decisions)
}

fn apply_candidate(
    store: &mut KnowledgeStore,
    cand: &Candidate,
    idx: usize,
) -> Result<()> {
    match cand {
        Candidate::Append {
            entity,
            description,
            tags,
            source_path,
            reason,
        } => {
            let tag_vals: Vec<Tag> = tags
                .iter()
                .filter_map(|s| Tag::parse(s).ok())
                .collect();
            // Synthetic provenance — the Reflector proposes new facts that
            // didn't come from a compiled source file. Callers who want
            // true source anchors should `knowledge compile` instead.
            let anchor = SourceAnchor {
                path: source_path
                    .clone()
                    .unwrap_or_else(|| format!("ace/reflector#{idx}")),
                byte_range: (0, description.len()),
                line_range: (1, 1),
                content_hash: "ace-synthetic".into(),
            };
            let edp = EntityDescriptionPair::new(
                entity.clone(),
                description.clone(),
                tag_vals,
                anchor,
                vec![],
            );
            store.append_fact_with_reason(edp, reason.clone())?;
            Ok(())
        }
        Candidate::Patch {
            id,
            description,
            tags,
            reason,
        } => {
            let tag_vals: Option<Vec<Tag>> = tags
                .as_ref()
                .map(|ts| ts.iter().filter_map(|s| Tag::parse(s).ok()).collect());
            store.patch_fact(
                &FactId::from_raw(id.clone()),
                FactPatch {
                    description: description.clone(),
                    tags: tag_vals,
                    reason: reason.clone(),
                },
            )
        }
        Candidate::Deprecate { id, reason } => {
            store.deprecate_fact(&FactId::from_raw(id.clone()), reason.clone())
        }
    }
}

fn reflector_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["candidates"],
        "additionalProperties": false,
        "properties": {
            "candidates": {
                "type": "array",
                "items": {
                    "oneOf": [
                        {
                            "type": "object",
                            "required": ["op", "entity", "description"],
                            "properties": {
                                "op": {"const": "append"},
                                "entity": {"type": "string"},
                                "description": {"type": "string"},
                                "tags": {"type": "array", "items": {"type": "string"}},
                                "source_path": {"type": "string"},
                                "reason": {"type": "string"}
                            }
                        },
                        {
                            "type": "object",
                            "required": ["op", "id"],
                            "properties": {
                                "op": {"const": "patch"},
                                "id": {"type": "string"},
                                "description": {"type": "string"},
                                "tags": {"type": "array", "items": {"type": "string"}},
                                "reason": {"type": "string"}
                            }
                        },
                        {
                            "type": "object",
                            "required": ["op", "id"],
                            "properties": {
                                "op": {"const": "deprecate"},
                                "id": {"type": "string"},
                                "reason": {"type": "string"}
                            }
                        }
                    ]
                }
            }
        }
    })
}

fn curator_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["decisions"],
        "additionalProperties": false,
        "properties": {
            "decisions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["index", "decision"],
                    "properties": {
                        "index": {"type": "integer", "minimum": 0},
                        "decision": {"type": "string", "enum": ["accept", "reject"]},
                        "reason": {"type": "string"}
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn seed_store(dir: &std::path::Path) -> (KnowledgeStore, FactId) {
        let mut store = KnowledgeStore::create(dir, "kb").unwrap();
        let edp = EntityDescriptionPair::new(
            "existing",
            "existing description",
            vec![],
            SourceAnchor {
                path: "a.md".into(),
                byte_range: (0, 10),
                line_range: (1, 1),
                content_hash: "h".into(),
            },
            vec![],
        );
        let id = edp.id.clone();
        store.append_fact(edp).unwrap();
        (store, id)
    }

    #[test]
    fn parse_candidate_set_accepts_all_three_ops() {
        let json = r#"{"candidates": [
            {"op": "append", "entity": "e", "description": "d"},
            {"op": "patch", "id": "fact-1", "description": "new"},
            {"op": "deprecate", "id": "fact-2", "reason": "obsolete"}
        ]}"#;
        let set = parse_candidate_set(json).unwrap();
        assert_eq!(set.candidates.len(), 3);
        assert!(matches!(set.candidates[0], Candidate::Append { .. }));
        assert!(matches!(set.candidates[1], Candidate::Patch { .. }));
        assert!(matches!(set.candidates[2], Candidate::Deprecate { .. }));
    }

    #[test]
    fn apply_candidate_append_creates_new_fact() {
        let dir = tempdir().unwrap();
        let (mut store, _) = seed_store(dir.path());
        let cand = Candidate::Append {
            entity: "new".into(),
            description: "new description".into(),
            tags: vec![],
            source_path: None,
            reason: Some("testing".into()),
        };
        apply_candidate(&mut store, &cand, 0).unwrap();
        assert_eq!(store.facts.len(), 2);
        // Revision recorded with reason.
        assert_eq!(
            store.revisions.last().unwrap().reason.as_deref(),
            Some("testing")
        );
    }

    #[test]
    fn apply_candidate_patch_updates_description() {
        let dir = tempdir().unwrap();
        let (mut store, id) = seed_store(dir.path());
        let cand = Candidate::Patch {
            id: id.to_string(),
            description: Some("patched".into()),
            tags: None,
            reason: Some("review".into()),
        };
        apply_candidate(&mut store, &cand, 0).unwrap();
        let fact = store.facts.iter().find(|f| f.id == id).unwrap();
        assert_eq!(fact.description, "patched");
    }

    #[test]
    fn apply_candidate_deprecate_sets_flag() {
        let dir = tempdir().unwrap();
        let (mut store, id) = seed_store(dir.path());
        let cand = Candidate::Deprecate {
            id: id.to_string(),
            reason: Some("superseded".into()),
        };
        apply_candidate(&mut store, &cand, 0).unwrap();
        assert!(store.facts.iter().find(|f| f.id == id).unwrap().deprecated);
    }

    #[test]
    fn apply_candidate_patch_rejects_unknown_id() {
        let dir = tempdir().unwrap();
        let (mut store, _) = seed_store(dir.path());
        let cand = Candidate::Patch {
            id: "fact-doesnotexist".into(),
            description: Some("x".into()),
            tags: None,
            reason: None,
        };
        assert!(apply_candidate(&mut store, &cand, 0).is_err());
    }

    #[test]
    fn parse_curator_decisions_caps_at_candidate_count() {
        let json = r#"{"decisions": [
            {"index": 0, "decision": "accept"},
            {"index": 1, "decision": "reject"}
        ]}"#;
        let decisions = parse_curator_decisions(json, 2).unwrap();
        assert_eq!(decisions.len(), 2);
        // Too many decisions → error.
        let too_many = r#"{"decisions": [
            {"index": 0, "decision": "accept"},
            {"index": 1, "decision": "reject"}
        ]}"#;
        assert!(parse_curator_decisions(too_many, 1).is_err());
    }
}
