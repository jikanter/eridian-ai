//! `--demo <feature>`: ask a model to pick the showboat demo under `docs/demos/`
//! that best matches a feature. The prompt is tuned for this repo's demo
//! conventions (showboat markdown, `demo-*`/`phase-*` naming).
//!
//! Scanning + prompt construction are pure/IO-isolated so they can be tested
//! without a model; the caller runs the prompt through the normal chat path.

use std::path::Path;

/// Standard location of showboat demos in the aichat repo.
pub const DEMOS_DIR: &str = "docs/demos";

/// A demo file: its filename and a human title (first H1, else the stem).
#[derive(Debug, Clone, PartialEq)]
pub struct DemoEntry {
    pub filename: String,
    pub title: String,
}

/// A demo's title: the first `# ` heading in its body, else the filename stem.
pub fn title_of(filename: &str, contents: &str) -> String {
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            return rest.trim().to_string();
        }
    }
    filename.strip_suffix(".md").unwrap_or(filename).to_string()
}

/// Scan a demos directory for `*.md` files, sorted by filename, each with its
/// extracted title. Missing/unreadable dir yields an empty list.
pub fn scan_demos(dir: &Path) -> Vec<DemoEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<DemoEntry> = read
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("md") {
                return None;
            }
            let filename = path.file_name()?.to_str()?.to_string();
            let contents = std::fs::read_to_string(&path).unwrap_or_default();
            let title = title_of(&filename, &contents);
            Some(DemoEntry { filename, title })
        })
        .collect();
    entries.sort_by(|a, b| a.filename.cmp(&b.filename));
    entries
}

/// Build the model prompt that selects the best-matching demo, tuned for the
/// aichat repo.
pub fn build_demo_prompt(feature: &str, demos: &[DemoEntry]) -> String {
    let mut listing = String::new();
    for d in demos {
        listing.push_str(&format!("- {} — {}\n", d.filename, d.title));
    }
    format!(
        "You are a demo locator for the `aichat` repository — a command-line \
multi-tool for AI applications. Its showboat demos live in `{dir}/` as markdown \
files (showboat = executable demo documents mixing commentary, code blocks, and \
captured output). Demo files are named like `demo-<topic>.md` or \
`phase-<n>-<topic>.md`.\n\n\
The user wants a demo for this feature:\n\
  \"{feature}\"\n\n\
Available demos (filename — title):\n\
{listing}\n\
Pick the SINGLE best-matching demo. Choose ONLY from the filenames listed above \
— do not invent filenames. Respond in exactly this form:\n\
  path: {dir}/<filename>\n\
  why: <one sentence on why it matches>\n\n\
If nothing matches, respond exactly:\n\
  NO MATCH — <closest available topic>\n",
        dir = DEMOS_DIR,
        feature = feature,
        listing = listing,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_uses_first_h1_heading() {
        let body = "some preamble\n# MCP Client Demo\nmore text\n# Second\n";
        assert_eq!(title_of("demo-mcp-client.md", body), "MCP Client Demo");
    }

    #[test]
    fn title_falls_back_to_stem_when_no_heading() {
        assert_eq!(title_of("phase-34-auto-memory.md", "no headings here"), "phase-34-auto-memory");
    }

    #[test]
    fn scan_returns_only_markdown_sorted_by_filename() {
        let dir = std::env::temp_dir().join(format!("aichat-demo-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b-demo.md"), "# Beta\n").unwrap();
        std::fs::write(dir.join("a-demo.md"), "# Alpha\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "ignore me").unwrap();

        let demos = scan_demos(&dir);
        let _ = std::fs::remove_dir_all(&dir);

        let names: Vec<&str> = demos.iter().map(|d| d.filename.as_str()).collect();
        assert_eq!(names, vec!["a-demo.md", "b-demo.md"]);
        assert_eq!(demos[0].title, "Alpha");
    }

    #[test]
    fn scan_missing_dir_is_empty() {
        let dir = std::env::temp_dir().join("aichat-demo-does-not-exist-xyz");
        assert!(scan_demos(&dir).is_empty());
    }

    #[test]
    fn prompt_names_feature_demos_dir_and_each_file() {
        let demos = vec![
            DemoEntry { filename: "demo-mcp-client.md".into(), title: "MCP Client".into() },
            DemoEntry { filename: "phase-34-auto-memory.md".into(), title: "Auto Memory".into() },
        ];
        let prompt = build_demo_prompt("memory", &demos);

        assert!(prompt.contains("memory"), "must include the feature");
        assert!(prompt.contains("docs/demos"), "must reference the demos dir");
        assert!(prompt.contains("demo-mcp-client.md"));
        assert!(prompt.contains("phase-34-auto-memory.md"));
        assert!(prompt.contains("NO MATCH"), "must instruct a no-match response");
    }

    #[test]
    fn prompt_constrains_choice_to_listed_files() {
        let demos = vec![DemoEntry { filename: "discovery.md".into(), title: "Discovery".into() }];
        let prompt = build_demo_prompt("discovery", &demos);
        // The instruction must forbid inventing filenames.
        assert!(prompt.to_lowercase().contains("only"), "prompt: {prompt}");
    }
}
