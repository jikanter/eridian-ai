//! Phase 34C: session-exit Reflector pass for the freeform `memory/` store.
//!
//! Structurally a sibling of the typed-knowledge Reflector in
//! [`crate::knowledge::evolve`]: transcript-in, structured-out, role-driven.
//! It differs in three ways mandated by `docs/roadmap/phase-34-overview.md`
//! §34C:
//!
//! 1. **Output is freeform markdown** (`MemoryCandidate`), not typed
//!    `EntityDescriptionPair` JSONL.
//! 2. **Topic names derive from a recurring noun phrase**, not a `FactId`
//!    content hash. [`derive_topic_name`] is the fallback when the Reflector
//!    omits a usable slug.
//! 3. **A secret-redaction pass runs first**, on the full transcript, before
//!    the Reflector ever sees it. This is the *only* secret defence (the
//!    curator displays whatever the Reflector returns), so [`redact_secrets`]
//!    is tested aggressively — every default pattern has positive and negative
//!    coverage.
//!
//! No new dependency: the redaction patterns are matched by hand rather than
//! pulling in `regex`, per the project's "ask before adding deps" rule. The
//! YAML-configurable `redact_patterns.yaml` extension point named in the deep
//! design is deliberately out of scope here.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{GlobalConfig, Input, Role};
use crate::utils::get_env_name;

/// One freeform topic file proposed by the Reflector. Frontmatter is *not*
/// carried here — the curator (34D) stamps `created` / `session` /
/// `reflector_model` / `curator` at write time so provenance reflects the
/// actual persistence event, not the proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCandidate {
    /// Slug for the topic file (`memory/<topic>.md`). Sanitized at write time.
    pub topic: String,
    /// Freeform markdown body (no frontmatter).
    pub body: String,
    /// Transcript turn indices that motivated this candidate — audit trail.
    #[serde(default)]
    pub turns_referenced: Vec<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryCandidateSet {
    pub candidates: Vec<MemoryCandidate>,
}

/// Env override that swaps the live LLM Reflector for a deterministic echo:
/// the (already-redacted) transcript becomes a single candidate body. This is
/// the test seam the bats `34C` secret-redaction case relies on — it asserts
/// the candidate contains `[REDACTED:...]` rather than the literal key,
/// proving redaction ran *before* the Reflector boundary.
const ECHO_ENV: &str = "memory_reflect_echo";

const DEFAULT_MEMORY_REFLECTOR_PROMPT: &str = "\
You are extracting durable preferences and incidental learnings from a
conversation transcript that the user will want to remember in future
sessions. You emit zero or more topic files, each scoped to ONE coherent
theme.

Emit JSON of the form:
{\"candidates\": [
  {\"topic\": \"short_snake_case_slug\", \"body\": \"freeform markdown\", \"turns_referenced\": [3, 5]}
]}

DO NOT emit topics for:
- Single-use answers to one-shot questions (e.g. \"what's the syntax for X\").
- Information already captured in the project's CLAUDE.md / AGENTS.md.
- Anything that looks like a secret (it has already been redacted to
  [REDACTED:...] markers — never try to reconstruct the original value).

DO emit topics for:
- Stated user preferences (\"I always prefer X over Y for this project\").
- Errors observed and the resolution path the user adopted.
- Hints about which roles or models worked well for which task today.

Rules:
- `topic` is a short snake_case slug derived from the recurring noun phrase.
- `body` is freeform markdown; keep it to a few sentences per topic.
- `turns_referenced` lists the 0-based transcript turns that motivated it.";

// ---------- secret redaction ----------

/// Run the mandatory secret-redaction pass over a transcript. Each match is
/// replaced with `[REDACTED:<category>]` so the Reflector keeps the structural
/// context ("a key was set here") without the secret itself. Operates
/// line-by-line; intra-line whitespace is normalized to single spaces, which
/// is harmless for a transcript fed to an LLM.
pub fn redact_secrets(input: &str) -> String {
    let joined = input
        .lines()
        .map(redact_line)
        .collect::<Vec<_>>()
        .join("\n");
    if input.ends_with('\n') {
        format!("{joined}\n")
    } else {
        joined
    }
}

fn redact_line(line: &str) -> String {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        // `Bearer <token>` — the secret is the following whitespace-separated
        // token, so consume two tokens at once.
        if tokens[i] == "Bearer" && i + 1 < tokens.len() {
            out.push("Bearer".to_string());
            out.push("[REDACTED:bearer_token]".to_string());
            i += 2;
            continue;
        }
        out.push(redact_token(tokens[i]));
        i += 1;
    }
    out.join(" ")
}

fn redact_token(tok: &str) -> String {
    // key=value / key:value — redact the value when the key looks secret-ish;
    // otherwise still classify the value by prefix (e.g. `auth=sk-ant-...`).
    if let Some((left, sep, right)) = split_kv(tok) {
        if is_secret_key(left) {
            return format!("{left}{sep}[REDACTED:generic_secret]");
        }
        return format!("{left}{sep}{}", redact_token(right));
    }
    match classify_prefix(tok) {
        Some(cat) => format!("[REDACTED:{cat}]"),
        None => tok.to_string(),
    }
}

/// Split a token into `(key, separator, value)` on the first `=`, or on the
/// first `:` only when the left side is a bare identifier (so URLs and
/// timestamps like `12:04:00` are left alone).
fn split_kv(tok: &str) -> Option<(&str, char, &str)> {
    if let Some(idx) = tok.find('=') {
        return Some((&tok[..idx], '=', &tok[idx + 1..]));
    }
    if let Some(idx) = tok.find(':') {
        let left = &tok[..idx];
        if !left.is_empty()
            && left
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Some((left, ':', &tok[idx + 1..]));
        }
    }
    None
}

fn is_secret_key(left: &str) -> bool {
    let lowered: String = left
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .map(|c| c.to_ascii_lowercase())
        .collect();
    const KEYS: [&str; 6] = ["api_key", "apikey", "api-key", "secret", "password", "token"];
    KEYS.iter().any(|k| lowered.contains(k))
}

/// Classify a bare token by its provider key prefix. Returns the redaction
/// category, or `None` when the token is not a recognized secret shape.
fn classify_prefix(tok: &str) -> Option<&'static str> {
    // Strip surrounding quotes / trailing punctuation so `"sk-...",` matches.
    let t = tok.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | '`' | '(' | ')'));

    // Anthropic must precede the generic OpenAI `sk-` test (it is a subset).
    if let Some(rest) = t.strip_prefix("sk-ant-") {
        if rest.len() >= 20 && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Some("anthropic_key");
        }
    }
    if let Some(rest) = t.strip_prefix("sk-") {
        if rest.len() >= 20 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Some("openai_key");
        }
    }
    if let Some(rest) = t.strip_prefix("xoxb-") {
        // xoxb-<digits>-<digits>-<alnum>
        let parts: Vec<&str> = rest.split('-').collect();
        if parts.len() >= 3
            && parts[0].chars().all(|c| c.is_ascii_digit())
            && parts[1].chars().all(|c| c.is_ascii_digit())
            && !parts[2].is_empty()
            && parts[2].chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Some("slack_token");
        }
    }
    if let Some(rest) = t.strip_prefix("ghp_") {
        if rest.len() == 36 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Some("github_pat");
        }
    }
    None
}

// ---------- topic-name derivation ----------

const STOPWORDS: [&str; 24] = [
    "the", "a", "an", "and", "or", "but", "for", "to", "of", "in", "on", "at", "is", "are", "was",
    "were", "be", "this", "that", "with", "i", "you", "we", "it",
];

/// Derive a snake_case topic slug from a markdown body when the Reflector
/// fails to supply a usable one. Picks the most frequent non-stopword tokens
/// in document order, deterministically (ties broken by first appearance).
pub fn derive_topic_name(body: &str) -> String {
    use std::collections::HashMap;
    let mut counts: HashMap<String, (usize, usize)> = HashMap::new(); // word -> (count, first_pos)
    for (pos, raw) in body.split(|c: char| !c.is_ascii_alphanumeric()).enumerate() {
        if raw.is_empty() {
            continue;
        }
        let w = raw.to_ascii_lowercase();
        if w.len() < 3 || STOPWORDS.contains(&w.as_str()) || w.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        counts.entry(w).or_insert((0, pos)).0 += 1;
    }
    if counts.is_empty() {
        return "untitled".to_string();
    }
    let mut ranked: Vec<(String, usize, usize)> = counts
        .into_iter()
        .map(|(w, (c, p))| (w, c, p))
        .collect();
    // Most frequent first; ties broken by earliest appearance.
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    ranked
        .into_iter()
        .take(3)
        .map(|(w, _, _)| w)
        .collect::<Vec<_>>()
        .join("_")
}

/// Sanitize a Reflector-proposed topic into a safe filename stem: lowercase,
/// alphanumeric runs collapsed to single underscores, no path separators, no
/// leading/trailing underscores. Empty input falls back to `untitled`.
pub fn sanitize_topic(topic: &str) -> String {
    let mut out = String::new();
    let mut prev_us = false;
    for c in topic.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_us = false;
        } else if !prev_us && !out.is_empty() {
            out.push('_');
            prev_us = true;
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed
    }
}

// ---------- reflector invocation ----------

fn parse_candidate_set(text: &str) -> Result<MemoryCandidateSet> {
    let parsed: MemoryCandidateSet = serde_json::from_str(text.trim())
        .with_context(|| format!("Memory Reflector returned non-JSON or wrong shape: {text}"))?;
    Ok(parsed)
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
                    "type": "object",
                    "required": ["topic", "body"],
                    "properties": {
                        "topic": {"type": "string"},
                        "body": {"type": "string"},
                        "turns_referenced": {"type": "array", "items": {"type": "integer", "minimum": 0}}
                    }
                }
            }
        }
    })
}

/// Resolve a user-defined memory-reflector role (a role whose name ends in
/// `-memory-reflector`), falling back to a built-in role carrying the default
/// prompt. Mirrors [`crate::knowledge::evolve`]'s `resolve_ace_role` suffix
/// convention.
fn resolve_reflector_role(config: &GlobalConfig) -> Role {
    let all = crate::config::Config::all_roles();
    for role in &all {
        if role.name().ends_with("-memory-reflector") {
            return role.clone();
        }
    }
    let mut role = config.read().extract_role();
    let mut fresh = Role::new(role.name(), DEFAULT_MEMORY_REFLECTOR_PROMPT);
    fresh.sync(&role);
    role = fresh;
    role
}

/// Run the Reflector over a transcript and return candidate topic files.
/// Redacts secrets first (always), then either echoes (test seam) or invokes
/// the role-driven LLM. Nothing is written to disk — 34D's curator gates that.
pub async fn reflect(config: &GlobalConfig, transcript: &str) -> Result<MemoryCandidateSet> {
    let redacted = redact_secrets(transcript);

    if std::env::var(get_env_name(ECHO_ENV)).is_ok() {
        // Deterministic echo: the redacted transcript is the candidate body.
        let topic = derive_topic_name(&redacted);
        return Ok(MemoryCandidateSet {
            candidates: vec![MemoryCandidate {
                topic,
                body: redacted,
                turns_referenced: vec![],
            }],
        });
    }

    let mut role = resolve_reflector_role(config);
    role.set_output_schema(Some(reflector_schema()));
    let prompt = format!(
        "Conversation transcript (secrets already redacted):\n\n{redacted}"
    );
    let input = Input::from_str(config, &prompt, Some(role));
    let text = input
        .fetch_chat_text()
        .await
        .context("Memory Reflector role call failed")?;
    let mut set = parse_candidate_set(&text)?;
    // Backfill / sanitize topic slugs so the curator and writer never see an
    // unsafe or empty filename stem.
    for cand in set.candidates.iter_mut() {
        let slug = if cand.topic.trim().is_empty() {
            derive_topic_name(&cand.body)
        } else {
            sanitize_topic(&cand.topic)
        };
        cand.topic = slug;
    }
    Ok(set)
}

// ---------- CLI entry points ----------

/// Read a transcript from `path`, or from stdin when `path` is `None`.
pub fn read_transcript(path: Option<&str>) -> Result<String> {
    use std::io::Read as _;
    match path {
        Some(p) => std::fs::read_to_string(p)
            .with_context(|| format!("Failed to read transcript file {p}")),
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("Failed to read transcript from stdin")?;
            Ok(buf)
        }
    }
}

/// Parse a JSON candidate-set file (the `--memory-candidates` path), bypassing
/// the Reflector. Topic slugs are sanitized so a hand-written file can't
/// inject path separators.
pub fn load_candidate_set_file(path: &str) -> Result<MemoryCandidateSet> {
    let json = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read candidates file {path}"))?;
    let mut set: MemoryCandidateSet = serde_json::from_str(&json)
        .with_context(|| format!("candidates file {path} is not a valid MemoryCandidateSet"))?;
    for c in set.candidates.iter_mut() {
        c.topic = if c.topic.trim().is_empty() {
            derive_topic_name(&c.body)
        } else {
            sanitize_topic(&c.topic)
        };
    }
    Ok(set)
}

/// `--memory-reflect`: redact + reflect over a transcript, emit candidates as
/// JSON to stdout. Deterministic shape; `--memory-curate --memory-candidates`
/// consumes it.
pub async fn run_reflect_cli(config: &GlobalConfig, transcript_path: Option<&str>) -> Result<()> {
    let transcript = read_transcript(transcript_path)?;
    let set = reflect(config, &transcript).await?;
    println!("{}", serde_json::to_string_pretty(&set)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_all_default_patterns() {
        let cases = [
            ("OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz012345", "generic_secret"),
            ("here is sk-abcdefghijklmnopqrstuvwxyz012345 alone", "openai_key"),
            ("key sk-ant-abcdefghijklmnopqrstuvwxyz_-012 here", "anthropic_key"),
            ("token xoxb-12345-67890-AbCdEfGhIjK done", "slack_token"),
            ("pat ghp_abcdefghijklmnopqrstuvwxyz0123456789 ok", "github_pat"),
            ("Authorization: Bearer eyJhbG.foo-bar_baz here", "bearer_token"),
            ("password=hunter2supersecret", "generic_secret"),
            ("secret: mytopsecretvalue", "generic_secret"),
        ];
        for (input, cat) in cases {
            let out = redact_secrets(input);
            assert!(
                out.contains(&format!("[REDACTED:{cat}]")),
                "input {input:?} should redact as {cat}, got {out:?}"
            );
        }
    }

    #[test]
    fn redaction_drops_the_literal_secret() {
        let out = redact_secrets("export OPENAI_API_KEY=sk-test-12345");
        assert!(!out.contains("sk-test-12345"), "literal key survived: {out}");
        assert!(out.contains("[REDACTED:"));
    }

    #[test]
    fn does_not_redact_lookalike_text() {
        // Short `sk-` token, a bare word, a URL, and a timestamp must all
        // survive untouched.
        let inputs = [
            "sk-abc is too short",
            "the word token-ring is fine here",
            "see http://example.com/path for details",
            "logged at 12:04:00 today",
        ];
        for input in inputs {
            let out = redact_secrets(input);
            assert!(
                !out.contains("[REDACTED"),
                "input {input:?} should not redact, got {out:?}"
            );
        }
    }

    #[test]
    fn redaction_preserves_non_secret_structure() {
        let out = redact_secrets("user prefers tokio over async_std");
        assert_eq!(out, "user prefers tokio over async_std");
    }

    #[test]
    fn derives_topic_name_from_noun_phrase() {
        let body = "The user prefers tokio spawn over async_std for tokio projects. \
                    Tokio is the standard.";
        let topic = derive_topic_name(body);
        // `tokio` is the dominant token → leads the slug.
        assert!(topic.starts_with("tokio"), "got {topic}");
        assert!(!topic.contains(' '));
    }

    #[test]
    fn derive_topic_name_empty_body_is_untitled() {
        assert_eq!(derive_topic_name("the a an and"), "untitled");
    }

    #[test]
    fn sanitize_topic_strips_unsafe_chars() {
        assert_eq!(sanitize_topic("Rust Async/Preferences!"), "rust_async_preferences");
        assert_eq!(sanitize_topic("../../etc/passwd"), "etc_passwd");
        assert_eq!(sanitize_topic("   "), "untitled");
        assert_eq!(sanitize_topic("already_good"), "already_good");
    }

    #[test]
    fn parse_candidate_set_round_trips() {
        let json = r#"{"candidates":[
            {"topic":"t","body":"b","turns_referenced":[1,2]},
            {"topic":"t2","body":"b2"}
        ]}"#;
        let set = parse_candidate_set(json).unwrap();
        assert_eq!(set.candidates.len(), 2);
        assert_eq!(set.candidates[0].turns_referenced, vec![1, 2]);
        assert!(set.candidates[1].turns_referenced.is_empty());
    }
}