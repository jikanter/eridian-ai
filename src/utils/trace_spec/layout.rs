//! SPEC-001 §1: on-disk file layout + the per-parent `manifest.jsonl`.
//!
//! ```text
//! <base>/                         # $XDG_STATE_HOME/aichat by default
//! ├── traces/
//! │   ├── manifest.jsonl          # one line per turn: parent ↔ turn binding
//! │   └── turn-<turn_id>.jsonl    # one file per conversational turn
//! └── blobs/                      # content-addressed, sharded (see blob.rs)
//! ```
//!
//! The manifest is tailable: a consumer watching a multi-turn conversation
//! sees each turn's binding appear as the turn starts.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Resolves the SPEC-001 §1 paths under a base directory.
pub struct TraceLayout {
    base: PathBuf,
}

impl TraceLayout {
    pub fn new<P: Into<PathBuf>>(base: P) -> Self {
        Self { base: base.into() }
    }

    /// Resolve the default base: `$AICHAT_TRACE_DIR`, else
    /// `$XDG_STATE_HOME/aichat`, else `$HOME/.local/state/aichat`.
    pub fn from_env() -> Self {
        if let Ok(dir) = std::env::var("AICHAT_TRACE_DIR") {
            return Self::new(dir);
        }
        let state = std::env::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                Path::new(&home).join(".local").join("state")
            });
        Self::new(state.join("aichat"))
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    /// `<base>/traces/`.
    pub fn traces_dir(&self) -> PathBuf {
        self.base.join("traces")
    }

    /// `<base>/blobs/` — the blob-store root.
    pub fn blobs_dir(&self) -> PathBuf {
        self.base.join("blobs")
    }

    /// `<base>/traces/turn-<turn_id>.jsonl`.
    pub fn turn_path(&self, turn_id: &str) -> PathBuf {
        self.traces_dir().join(format!("turn-{turn_id}.jsonl"))
    }

    /// `<base>/traces/manifest.jsonl`.
    pub fn manifest_path(&self) -> PathBuf {
        self.traces_dir().join("manifest.jsonl")
    }
}

/// Append one `{parent_session_id, turn_session_id, ts_ns}` binding to the
/// manifest (SPEC §1). Creates the file and parent dirs on first write. Each
/// line is a single `write_all` + `flush` so tailing consumers never see a
/// partial line.
pub fn append_manifest(
    manifest_path: &Path,
    parent_session_id: Option<&str>,
    turn_session_id: &str,
    ts_ns: u64,
) -> std::io::Result<()> {
    if let Some(dir) = manifest_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let line = serde_json::json!({
        "parent_session_id": parent_session_id,
        "turn_session_id": turn_session_id,
        "ts_ns": ts_ns,
    });
    let mut bytes = serde_json::to_vec(&line).unwrap_or_default();
    bytes.push(b'\n');
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(manifest_path)?;
    f.write_all(&bytes)?;
    f.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_base(tag: &str) -> PathBuf {
        let id = format!("{:?}", std::thread::current().id());
        let dir = std::env::temp_dir()
            .join("aichat-layout-test")
            .join(format!("{tag}-{}", id.replace(['(', ')', ' '], "")));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn resolves_spec_paths() {
        let l = TraceLayout::new("/state/aichat");
        assert_eq!(l.traces_dir(), Path::new("/state/aichat/traces"));
        assert_eq!(l.blobs_dir(), Path::new("/state/aichat/blobs"));
        assert_eq!(
            l.turn_path("01HTURN"),
            Path::new("/state/aichat/traces/turn-01HTURN.jsonl")
        );
        assert_eq!(
            l.manifest_path(),
            Path::new("/state/aichat/traces/manifest.jsonl")
        );
    }

    #[test]
    fn append_manifest_writes_binding_line() {
        let base = temp_base("manifest");
        let l = TraceLayout::new(&base);
        append_manifest(&l.manifest_path(), Some("PARENT"), "TURN1", 123).unwrap();
        let content = std::fs::read_to_string(l.manifest_path()).unwrap();
        let v: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(v["parent_session_id"], "PARENT");
        assert_eq!(v["turn_session_id"], "TURN1");
        assert_eq!(v["ts_ns"], 123);
    }

    #[test]
    fn append_manifest_null_parent_for_oneshot() {
        let base = temp_base("manifest-null");
        let l = TraceLayout::new(&base);
        append_manifest(&l.manifest_path(), None, "TURN1", 1).unwrap();
        let content = std::fs::read_to_string(l.manifest_path()).unwrap();
        let v: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert!(v["parent_session_id"].is_null());
    }

    #[test]
    fn append_manifest_is_append_only() {
        let base = temp_base("manifest-append");
        let l = TraceLayout::new(&base);
        append_manifest(&l.manifest_path(), None, "TURN1", 1).unwrap();
        append_manifest(&l.manifest_path(), None, "TURN2", 2).unwrap();
        let content = std::fs::read_to_string(l.manifest_path()).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        let v2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(v2["turn_session_id"], "TURN2");
    }
}
