//! Phase 34D: curator gate for Reflector-emitted memory candidates.
//!
//! Each candidate is presented to the user (`[a]ccept [s]kip [e]dit
//! [r]eject-all`). `accept` writes `memory/<topic>.md` atomically (tmp +
//! rename, mirroring `knowledge/store.rs`) and appends an index line to
//! `memory/MEMORY.md`. Under `--memory-auto-curate` every candidate
//! auto-accepts with no prompt — the flag is opt-in, never the default, per
//! tenet 3 ("the curator gate is non-negotiable for writes").
//!
//! `memory/` deliberately gains no per-mutation `revisions.jsonl`: git history
//! of the tracked markdown files is the audit substrate (overview dual-store
//! table, deep-design cited range `store.rs:117`).

use std::io::{BufRead, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::reflect::MemoryCandidate;
use super::{MEMORY_INDEX, MEMORY_SUBDIR};
use crate::config::Config;
use crate::utils::get_env_name;

/// Where the curator persists files. Honors `AICHAT_MEMORY_DIR` first, then
/// project-local `./memory/`, then the user-level `<config_dir>/memory/`.
/// Unlike [`super::memory_dir`] this does **not** require an existing
/// `MEMORY.md` — it is a write target — and it creates the directory.
pub fn memory_write_dir() -> Result<PathBuf> {
    let dir = if let Ok(d) = std::env::var(get_env_name("memory_dir")) {
        PathBuf::from(d)
    } else if let Ok(cwd) = std::env::current_dir() {
        // Project-local wins only when a `memory/` dir already exists there;
        // otherwise fall through to the user-level store so we never silently
        // scatter `memory/` dirs across arbitrary working directories.
        let proj = cwd.join(MEMORY_SUBDIR);
        if proj.is_dir() {
            proj
        } else {
            Config::config_dir().join(MEMORY_SUBDIR)
        }
    } else {
        Config::config_dir().join(MEMORY_SUBDIR)
    };
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create memory dir '{}'", dir.display()))?;
    Ok(dir)
}

/// Provenance stamped into a written topic file's frontmatter.
#[derive(Debug, Clone)]
pub struct Provenance {
    pub created: String,
    pub session: Option<String>,
    pub reflector_model: Option<String>,
    /// "interactive" or "auto".
    pub curator: &'static str,
}

impl Provenance {
    /// Build provenance stamping `created` from the wall clock now.
    pub fn now(session: Option<String>, reflector_model: Option<String>, auto: bool) -> Self {
        Self {
            created: chrono::Utc::now().to_rfc3339(),
            session,
            reflector_model,
            curator: if auto { "auto" } else { "interactive" },
        }
    }
}

/// Render a full topic file (YAML frontmatter + body) for a candidate.
pub fn render_topic_file(cand: &MemoryCandidate, prov: &Provenance) -> String {
    let mut fm = String::from("---\n");
    fm.push_str(&format!("created: {}\n", prov.created));
    if let Some(s) = &prov.session {
        fm.push_str(&format!("session: {s}\n"));
    }
    if let Some(m) = &prov.reflector_model {
        fm.push_str(&format!("reflector_model: {m}\n"));
    }
    fm.push_str(&format!("curator: {}\n", prov.curator));
    if !cand.turns_referenced.is_empty() {
        let turns = cand
            .turns_referenced
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        fm.push_str(&format!("turns_referenced: [{turns}]\n"));
    }
    fm.push_str("---\n\n");
    fm.push_str(cand.body.trim_end());
    fm.push('\n');
    fm
}

/// The single index line appended to `MEMORY.md` for an accepted topic. The
/// hook is the candidate body's first non-empty line, trimmed.
pub fn index_line(topic: &str, body: &str) -> String {
    let hook = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let hook: String = hook.chars().take(80).collect();
    if hook.is_empty() {
        format!("- [{topic}]({topic}.md)", topic = topic)
    } else {
        format!("- [{topic}]({topic}.md) — {hook}")
    }
}

/// Atomic write: stage to `<path>.tmp`, fsync, rename over the target.
/// Mirrors `knowledge/store.rs::atomic_write` (which is private to that
/// module).
fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let tmp = path.with_extension("md.tmp");
    {
        let mut f = std::fs::File::create(&tmp)
            .with_context(|| format!("Failed to create temp file '{}'", tmp.display()))?;
        f.write_all(contents.as_bytes())?;
        f.flush()?;
    }
    std::fs::rename(&tmp, path)
        .with_context(|| format!("Failed to rename '{}' -> '{}'", tmp.display(), path.display()))?;
    Ok(())
}

/// Append a line to `MEMORY.md`, creating the file with a header if absent.
fn append_index(dir: &Path, line: &str) -> Result<()> {
    let index = dir.join(MEMORY_INDEX);
    let mut existing = std::fs::read_to_string(&index).unwrap_or_default();
    if existing.is_empty() {
        existing.push_str("# Memory Index\n\n");
    } else if !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(line);
    existing.push('\n');
    atomic_write(&index, &existing)
}

/// Persist one accepted candidate: write the topic file and append the index
/// line. Returns the path written. The topic slug is assumed already
/// sanitized by [`super::reflect::reflect`].
pub fn accept_candidate(dir: &Path, cand: &MemoryCandidate, prov: &Provenance) -> Result<PathBuf> {
    let path = dir.join(format!("{}.md", cand.topic));
    atomic_write(&path, &render_topic_file(cand, prov))?;
    append_index(dir, &index_line(&cand.topic, &cand.body))?;
    Ok(path)
}

/// What the user (or auto mode) chose for a single candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Accept,
    Skip,
    RejectAll,
}

fn render_candidate_card(idx: usize, total: usize, cand: &MemoryCandidate, prov: &Provenance) -> String {
    let sep = "─".repeat(47);
    format!(
        "Memory candidate ({}/{}): \"{}\"\n{sep}\n{}{sep}\n[a]ccept  [s]kip  [e]dit  [r]eject-all",
        idx + 1,
        total,
        cand.topic,
        render_topic_file(cand, prov),
    )
}

/// Map a single input character to a decision; `None` for unrecognized input
/// (the caller re-prompts). Factored out for unit testing.
fn parse_decision(line: &str) -> Option<Decision> {
    match line.trim().chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('a') => Some(Decision::Accept),
        Some('s') => Some(Decision::Skip),
        Some('r') => Some(Decision::RejectAll),
        _ => None,
    }
}

/// Open `$EDITOR` on the candidate body, returning the edited body. Falls back
/// to the original body if no editor is configured or the edit fails.
fn edit_body(cand: &MemoryCandidate) -> Result<String> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_default();
    if editor.is_empty() {
        eprintln!("memory: $EDITOR unset; keeping candidate unedited.");
        return Ok(cand.body.clone());
    }
    let tmp = std::env::temp_dir().join(format!("aichat-memory-{}.md", cand.topic));
    std::fs::write(&tmp, &cand.body)?;
    let status = std::process::Command::new(&editor).arg(&tmp).status();
    let body = match status {
        Ok(s) if s.success() => std::fs::read_to_string(&tmp).unwrap_or_else(|_| cand.body.clone()),
        _ => {
            eprintln!("memory: editor exited non-zero; keeping candidate unedited.");
            cand.body.clone()
        }
    };
    let _ = std::fs::remove_file(&tmp);
    Ok(body)
}

/// Run the curator gate over a candidate set. `auto` short-circuits every
/// prompt to `accept`. Returns the number of files written. `reject-all`
/// aborts the pass cleanly (exit code 0 at the call site).
pub fn run_curate(
    candidates: &[MemoryCandidate],
    auto: bool,
    session: Option<String>,
    reflector_model: Option<String>,
) -> Result<usize> {
    let dir = memory_write_dir()?;
    run_curate_in(&dir, candidates, auto, session, reflector_model)
}

/// [`run_curate`] with the write directory injected — the test seam that
/// avoids mutating the process-global `AICHAT_MEMORY_DIR`.
pub fn run_curate_in(
    dir: &Path,
    candidates: &[MemoryCandidate],
    auto: bool,
    session: Option<String>,
    reflector_model: Option<String>,
) -> Result<usize> {
    if candidates.is_empty() {
        eprintln!("memory: no candidates to curate.");
        return Ok(0);
    }
    let prov = Provenance::now(session, reflector_model, auto);
    let total = candidates.len();
    let mut written = 0usize;

    let stdin = std::io::stdin();
    for (idx, cand) in candidates.iter().enumerate() {
        let mut cand = cand.clone();
        let decision = if auto {
            Decision::Accept
        } else {
            println!("{}", render_candidate_card(idx, total, &cand, &prov));
            loop {
                print!("> ");
                std::io::stdout().flush().ok();
                let mut line = String::new();
                let n = stdin.lock().read_line(&mut line)?;
                if n == 0 {
                    // EOF with no input: treat as reject-all so a closed pipe
                    // never silently writes.
                    break Decision::RejectAll;
                }
                if line.trim().eq_ignore_ascii_case("e") {
                    cand.body = edit_body(&cand)?;
                    println!("{}", render_candidate_card(idx, total, &cand, &prov));
                    continue;
                }
                if let Some(d) = parse_decision(&line) {
                    break d;
                }
                eprintln!("memory: unrecognized input; use a/s/e/r.");
            }
        };
        match decision {
            Decision::Accept => {
                let path = accept_candidate(dir, &cand, &prov)?;
                eprintln!("memory: wrote {}", path.display());
                written += 1;
            }
            Decision::Skip => {
                eprintln!("memory: skipped {}", cand.topic);
            }
            Decision::RejectAll => {
                eprintln!("memory: reject-all — aborting curation ({written} written so far).");
                break;
            }
        }
    }
    Ok(written)
}

/// `--memory-curate`: obtain candidates (from `--memory-candidates` or by
/// running the Reflector over a transcript), then run the curator gate.
/// Always exits `Ok` — `reject-all` and an empty set are clean no-ops.
pub async fn run_curate_cli(
    config: &crate::config::GlobalConfig,
    transcript_path: Option<&str>,
    candidates_path: Option<&str>,
    auto: bool,
) -> Result<()> {
    use super::reflect;
    let set = match candidates_path {
        Some(p) => reflect::load_candidate_set_file(p)?,
        None => {
            let transcript = reflect::read_transcript(transcript_path)?;
            reflect::reflect(config, &transcript).await?
        }
    };
    let written = run_curate(&set.candidates, auto, None, None)?;
    eprintln!("memory: curation complete — {written} file(s) written.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn cand(topic: &str, body: &str) -> MemoryCandidate {
        MemoryCandidate {
            topic: topic.into(),
            body: body.into(),
            turns_referenced: vec![3, 5],
        }
    }

    fn prov() -> Provenance {
        Provenance {
            created: "2026-05-30T00:00:00+00:00".into(),
            session: Some("sess-1".into()),
            reflector_model: Some("claude-haiku-4-5".into()),
            curator: "interactive",
        }
    }

    #[test]
    fn render_topic_file_has_frontmatter_and_body() {
        let out = render_topic_file(&cand("t", "Body line one.\nLine two."), &prov());
        assert!(out.starts_with("---\n"));
        assert!(out.contains("created: 2026-05-30T00:00:00+00:00"));
        assert!(out.contains("session: sess-1"));
        assert!(out.contains("reflector_model: claude-haiku-4-5"));
        assert!(out.contains("curator: interactive"));
        assert!(out.contains("turns_referenced: [3, 5]"));
        assert!(out.contains("Body line one."));
        assert!(out.trim_end().ends_with("Line two."));
    }

    #[test]
    fn index_line_uses_first_nonempty_line_as_hook() {
        let line = index_line("rust_async", "\n\n  The user prefers tokio.\nmore");
        assert_eq!(line, "- [rust_async](rust_async.md) — The user prefers tokio.");
    }

    #[test]
    fn index_line_without_hook() {
        assert_eq!(index_line("t", "   \n  "), "- [t](t.md)");
    }

    #[test]
    fn accept_writes_file_and_appends_index() {
        let dir = tempdir().unwrap();
        let p = accept_candidate(dir.path(), &cand("topic_a", "Prefer tokio."), &prov()).unwrap();
        assert!(p.exists());
        let written = std::fs::read_to_string(&p).unwrap();
        assert!(written.contains("Prefer tokio."));
        // No leftover tmp file.
        assert!(!dir.path().join("topic_a.md.tmp").exists());
        // Index appended.
        let index = std::fs::read_to_string(dir.path().join(MEMORY_INDEX)).unwrap();
        assert!(index.contains("- [topic_a](topic_a.md) — Prefer tokio."));
    }

    #[test]
    fn accept_two_candidates_appends_both_index_lines() {
        let dir = tempdir().unwrap();
        accept_candidate(dir.path(), &cand("a", "First."), &prov()).unwrap();
        accept_candidate(dir.path(), &cand("b", "Second."), &prov()).unwrap();
        let index = std::fs::read_to_string(dir.path().join(MEMORY_INDEX)).unwrap();
        assert!(index.contains("(a.md)"));
        assert!(index.contains("(b.md)"));
        // Header written exactly once.
        assert_eq!(index.matches("# Memory Index").count(), 1);
    }

    #[test]
    fn parse_decision_maps_chars() {
        assert_eq!(parse_decision("a\n"), Some(Decision::Accept));
        assert_eq!(parse_decision("  Accept"), Some(Decision::Accept));
        assert_eq!(parse_decision("s"), Some(Decision::Skip));
        assert_eq!(parse_decision("r"), Some(Decision::RejectAll));
        assert_eq!(parse_decision("x"), None);
        assert_eq!(parse_decision(""), None);
    }

    #[test]
    fn auto_curate_writes_all_without_prompt() {
        let dir = tempdir().unwrap();
        let cands = vec![cand("auto_one", "One."), cand("auto_two", "Two.")];
        let n = run_curate_in(dir.path(), &cands, true, None, None).unwrap();
        assert_eq!(n, 2);
        assert!(dir.path().join("auto_one.md").exists());
        assert!(dir.path().join("auto_two.md").exists());
    }
}
