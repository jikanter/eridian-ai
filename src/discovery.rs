//! Phase 53: aichat discovery surface.
//!
//! Two read-only introspection helpers used by the pi REPL bridge (and the
//! `/v1/discovery/*` server routes) to let users discover what aichat can do
//! without leaving the REPL:
//!
//! * [`discover_flags`] walks the live Clap command tree so the flag list is
//!   always in lock-step with `src/cli.rs` — no hand-maintained catalog to
//!   drift.
//! * [`list_docs`] / [`read_doc`] surface the user-facing feature docs, which
//!   are embedded into the binary at build time so discovery works for an
//!   installed aichat with no source tree on disk.

use clap::{ArgAction, CommandFactory};
use rust_embed::Embed;
use serde::Serialize;

use crate::cli::Cli;

/// User-facing feature docs, embedded at build time. Source of truth is the
/// checked-in `docs/features/*.md`; bundling them means `read_doc` works for an
/// installed binary with no repo on disk.
#[derive(Embed)]
#[folder = "docs/features/"]
struct FeatureDocs;

/// One CLI flag, projected from a Clap `Arg` into a serializable shape the
/// bridge can render.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct FlagInfo {
    /// Long form without the leading `--` (e.g. `model`), if any.
    pub long: Option<String>,
    /// Short form without the leading `-` (e.g. `m`), if any.
    pub short: Option<String>,
    /// Help text, flattened to a single line.
    pub help: String,
    /// Whether the flag consumes a value (`--model X`) vs. being a bare
    /// switch (`--list-roles`).
    pub takes_value: bool,
}

/// Walk the Clap command and project every documented option into a
/// [`FlagInfo`]. When `query` is `Some`, keep only flags whose long name,
/// short name, or help text contains the query (case-insensitive).
pub fn discover_flags(query: Option<&str>) -> Vec<FlagInfo> {
    let cmd = Cli::command();
    let needle = query.map(|q| q.to_lowercase());
    let mut out = Vec::new();
    for arg in cmd.get_arguments() {
        // Skip positionals and the auto-generated help/version switches —
        // discovery is about the flag surface users actually pass.
        if arg.is_positional() {
            continue;
        }
        let long = arg.get_long().map(|s| s.to_string());
        let short = arg.get_short().map(|c| c.to_string());
        if long.is_none() && short.is_none() {
            continue;
        }
        let help = arg
            .get_help()
            .map(|h| h.to_string().replace('\n', " "))
            .unwrap_or_default();
        let takes_value = matches!(arg.get_action(), ArgAction::Set | ArgAction::Append);

        if let Some(needle) = &needle {
            let hay = format!(
                "{} {} {}",
                long.as_deref().unwrap_or(""),
                short.as_deref().unwrap_or(""),
                help
            )
            .to_lowercase();
            if !hay.contains(needle) {
                continue;
            }
        }
        out.push(FlagInfo {
            long,
            short,
            help,
            takes_value,
        });
    }
    out
}

/// One embedded feature doc.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DocInfo {
    /// Slug (filename without the `.md` extension), used by `read_doc`.
    pub name: String,
    /// Original filename including `.md`.
    pub file: String,
    /// First Markdown `# ` heading, or the slug if the doc has none.
    pub title: String,
}

/// List every embedded feature doc, sorted by slug for stable output.
pub fn list_docs() -> Vec<DocInfo> {
    let mut docs: Vec<DocInfo> = FeatureDocs::iter()
        .filter(|f| f.ends_with(".md"))
        .map(|file| {
            let name = file.trim_end_matches(".md").to_string();
            let title = FeatureDocs::get(&file)
                .and_then(|f| std::str::from_utf8(&f.data).ok().map(doc_title))
                .flatten()
                .unwrap_or_else(|| name.clone());
            DocInfo {
                name,
                file: file.to_string(),
                title,
            }
        })
        .collect();
    docs.sort_by(|a, b| a.name.cmp(&b.name));
    docs
}

/// Read one embedded feature doc by slug (`server`) or filename (`server.md`).
/// Returns `None` if no such doc is bundled.
pub fn read_doc(name: &str) -> Option<String> {
    let file = if name.ends_with(".md") {
        name.to_string()
    } else {
        format!("{name}.md")
    };
    FeatureDocs::get(&file).and_then(|f| String::from_utf8(f.data.to_vec()).ok())
}

/// Extract the first Markdown `# ` heading from doc content.
fn doc_title(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.strip_prefix("# ")
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- discover_flags ----

    #[test]
    fn discovers_the_model_flag_with_both_forms() {
        let flags = discover_flags(None);
        let model = flags
            .iter()
            .find(|f| f.long.as_deref() == Some("model"))
            .expect("model flag present");
        assert_eq!(model.short.as_deref(), Some("m"));
        assert!(model.takes_value, "--model consumes a value");
        assert!(!model.help.is_empty());
    }

    #[test]
    fn boolean_switches_do_not_take_a_value() {
        let flags = discover_flags(None);
        let list_roles = flags
            .iter()
            .find(|f| f.long.as_deref() == Some("list-roles"))
            .expect("--list-roles present");
        assert!(!list_roles.takes_value, "--list-roles is a bare switch");
    }

    #[test]
    fn query_filters_to_matching_flags_only() {
        let flags = discover_flags(Some("role"));
        assert!(!flags.is_empty());
        for f in &flags {
            let hay = format!(
                "{} {}",
                f.long.as_deref().unwrap_or(""),
                f.help.to_lowercase()
            )
            .to_lowercase();
            assert!(hay.contains("role"), "every result mentions the query: {f:?}");
        }
        // The `--role` flag itself must be in there.
        assert!(flags.iter().any(|f| f.long.as_deref() == Some("role")));
    }

    #[test]
    fn query_matches_help_text_not_just_names() {
        // "session" appears in help text of session-related flags.
        let flags = discover_flags(Some("session"));
        assert!(flags.iter().any(|f| f.long.as_deref() == Some("session")));
    }

    #[test]
    fn unmatched_query_returns_empty() {
        assert!(discover_flags(Some("zzz-no-such-flag-xyz")).is_empty());
    }

    // ---- list_docs / read_doc ----

    #[test]
    fn lists_embedded_feature_docs_with_titles() {
        let docs = list_docs();
        let server = docs
            .iter()
            .find(|d| d.name == "server")
            .expect("server.md is embedded");
        assert_eq!(server.file, "server.md");
        assert!(!server.title.is_empty());
        assert!(!server.title.ends_with(".md"));
    }

    #[test]
    fn docs_are_sorted_by_slug() {
        let docs = list_docs();
        let names: Vec<&str> = docs.iter().map(|d| d.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    #[test]
    fn reads_a_doc_by_slug() {
        let body = read_doc("server").expect("server doc readable by slug");
        assert!(body.contains("# "));
    }

    #[test]
    fn reads_a_doc_by_filename() {
        assert!(read_doc("server.md").is_some());
    }

    #[test]
    fn missing_doc_returns_none() {
        assert!(read_doc("no-such-doc").is_none());
    }

    #[test]
    fn doc_title_extracts_first_heading() {
        assert_eq!(
            doc_title("intro\n# Real Title\n# Second"),
            Some("Real Title".to_string())
        );
        assert_eq!(doc_title("no heading here"), None);
    }
}
