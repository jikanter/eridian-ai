//! Phase 10B: Content-addressable cache for pipeline stage outputs.
//!
//! A stage's `(role, model, input)` fully determines its deterministic LLM
//! output. On a re-run with identical inputs we can replay the prior output
//! and skip the LLM call entirely. Entries expire after a configurable TTL
//! so long-running sessions don't serve stale results after role/prompt edits.
//!
//! Not cached: tool-using stages (non-deterministic side effects), dry-run
//! invocations, and stages the caller explicitly opts out of via `--no-cache`.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub struct StageCache {
    dir: PathBuf,
    ttl: Duration,
}

impl StageCache {
    pub fn new(dir: PathBuf, ttl_secs: u64) -> Self {
        Self {
            dir,
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Content-addressable key over the triple that determines output.
    /// Null-byte delimiters prevent `("ab","c","") == ("a","bc","")` collisions.
    pub fn key(role: &str, model: &str, input: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(role.as_bytes());
        hasher.update(b"\0");
        hasher.update(model.as_bytes());
        hasher.update(b"\0");
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let path = self.dir.join(format!("{key}.out"));
        let metadata = fs::metadata(&path).ok()?;
        let mtime = metadata.modified().ok()?;
        if SystemTime::now().duration_since(mtime).ok()? > self.ttl {
            return None;
        }
        fs::read_to_string(&path).ok()
    }

    pub fn put(&self, key: &str, output: &str) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("Failed to create cache dir {}", self.dir.display()))?;
        let path = self.dir.join(format!("{key}.out"));
        fs::write(&path, output)
            .with_context(|| format!("Failed to write cache entry {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn key_is_deterministic() {
        let k1 = StageCache::key("analyst", "gpt-4o", "hello");
        let k2 = StageCache::key("analyst", "gpt-4o", "hello");
        assert_eq!(k1, k2);
    }

    #[test]
    fn key_varies_by_role_model_and_input() {
        let base = StageCache::key("analyst", "gpt-4o", "hello");
        assert_ne!(base, StageCache::key("reviewer", "gpt-4o", "hello"));
        assert_ne!(base, StageCache::key("analyst", "gpt-5", "hello"));
        assert_ne!(base, StageCache::key("analyst", "gpt-4o", "hi"));
    }

    #[test]
    fn key_delimiter_prevents_concatenation_collision() {
        // Without a delimiter, both hash the concatenation "abc".
        let k1 = StageCache::key("ab", "c", "");
        let k2 = StageCache::key("a", "bc", "");
        assert_ne!(k1, k2);
    }

    #[test]
    fn get_returns_none_on_miss() {
        let dir = tempdir().unwrap();
        let cache = StageCache::new(dir.path().to_path_buf(), 3600);
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn put_then_get_roundtrip() {
        let dir = tempdir().unwrap();
        let cache = StageCache::new(dir.path().to_path_buf(), 3600);
        cache.put("abc", "the output").unwrap();
        assert_eq!(cache.get("abc").as_deref(), Some("the output"));
    }

    #[test]
    fn get_returns_none_after_ttl_expiry() {
        let dir = tempdir().unwrap();
        let cache = StageCache::new(dir.path().to_path_buf(), 0);
        cache.put("abc", "out").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert!(cache.get("abc").is_none());
    }

    #[test]
    fn put_creates_dir_if_missing() {
        let parent = tempdir().unwrap();
        let dir = parent.path().join("does/not/exist");
        let cache = StageCache::new(dir.clone(), 3600);
        cache.put("abc", "out").unwrap();
        assert!(dir.exists());
    }

    #[test]
    fn put_preserves_multiline_unicode() {
        let dir = tempdir().unwrap();
        let cache = StageCache::new(dir.path().to_path_buf(), 3600);
        let payload = "line 1\nline 2 — ñ café\n";
        cache.put("unicode", payload).unwrap();
        assert_eq!(cache.get("unicode").as_deref(), Some(payload));
    }
}
