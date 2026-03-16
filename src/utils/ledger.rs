use anyhow::{Context, Result};
use serde_json::Value;
use std::io::Write;
use std::path::Path;

/// Append one JSONL record to the run log file.
pub fn append_run_log(path: &Path, record: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create run log directory: {}", parent.display()))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open run log: {}", path.display()))?;
    let line = serde_json::to_string(record)?;
    writeln!(file, "{line}")?;
    Ok(())
}
