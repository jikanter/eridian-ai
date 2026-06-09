//! SPEC-001 §4: the content-addressed blob store.
//!
//! Large payloads (full prompts, RAG contexts, tool stdout, request/response
//! bodies) live under `blobs/<sha256>` and are referenced from events via
//! `*_hash` fields. Properties enforced here:
//!
//! - **Content-addressed.** SHA-256 of the bytes; identical content
//!   deduplicates across events and sessions.
//! - **Sharded.** The first four hex chars become two directory levels
//!   (`blobs/ab/cd/abcd…`) so a single directory never holds millions of
//!   entries.
//! - **Write-once.** A blob whose hash already exists is never rewritten
//!   (`create_new` / `O_EXCL`); the content is immutable by construction.
//! - **No GC.** The store grows monotonically; pruning is a future `trace gc`.

use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The SHA-256 of `bytes` as lowercase hex (64 chars). This is the blob's
/// address and the raw value behind an event's `*_hash` field.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Format a hex digest as a SPEC-001 hash reference (`sha256:<hex>`), the form
/// events carry in their `*_hash` fields.
pub fn hash_ref(hex: &str) -> String {
    format!("sha256:{hex}")
}

/// A sharded, write-once, content-addressed blob store rooted at a directory.
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    /// Open (or lazily create) a blob store rooted at `root` (the `blobs/`
    /// directory). Directories are created on demand at `put` time.
    pub fn new<P: Into<PathBuf>>(root: P) -> Self {
        Self { root: root.into() }
    }

    /// The on-disk path for a hex digest: `root/ab/cd/abcd…`.
    pub fn path_for(&self, hex: &str) -> PathBuf {
        // Digests are 64 hex chars; the first four shard into two levels.
        let l1 = &hex[0..2];
        let l2 = &hex[2..4];
        self.root.join(l1).join(l2).join(hex)
    }

    /// Store `bytes`, returning their hex digest. Write-once: if a blob with
    /// the same hash already exists, the existing file is left untouched and no
    /// rewrite happens.
    pub fn put(&self, bytes: &[u8]) -> std::io::Result<String> {
        let hex = sha256_hex(bytes);
        let path = self.path_for(&hex);
        if path.exists() {
            return Ok(hex);
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // `create_new` is O_EXCL: if two writers race, the loser sees
        // AlreadyExists and treats the blob as already stored.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                f.write_all(bytes)?;
                f.flush()?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        Ok(hex)
    }

    /// Read back a blob by hex digest.
    pub fn get(&self, hex: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.path_for(hex))
    }

    /// True if a blob with this hex digest is already stored.
    pub fn contains(&self, hex: &str) -> bool {
        self.path_for(hex).exists()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        // Unique-per-test dir under the OS temp dir. No Date/rand (forbidden in
        // this crate's test sandbox); the tag plus thread id disambiguate.
        let id = format!("{:?}", std::thread::current().id());
        let dir = std::env::temp_dir()
            .join("aichat-blob-test")
            .join(format!("{tag}-{}", id.replace(['(', ')', ' '], "")));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn sha256_hex_known_vector() {
        // Empty input -> the well-known SHA-256 of "".
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hash_ref_prefixes_sha256() {
        assert_eq!(hash_ref("abcd"), "sha256:abcd");
    }

    #[test]
    fn put_writes_two_level_sharded_path() {
        let store = BlobStore::new(temp_root("shard"));
        let hex = store.put(b"hello world").unwrap();
        let expected = store
            .root()
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(&hex);
        assert!(expected.exists(), "blob must live at sharded path {expected:?}");
        assert_eq!(store.path_for(&hex), expected);
    }

    #[test]
    fn get_round_trips_bytes() {
        let store = BlobStore::new(temp_root("roundtrip"));
        let hex = store.put(b"payload-bytes").unwrap();
        assert_eq!(store.get(&hex).unwrap(), b"payload-bytes");
    }

    #[test]
    fn put_is_write_once_and_idempotent() {
        let store = BlobStore::new(temp_root("writeonce"));
        let h1 = store.put(b"same").unwrap();
        let path = store.path_for(&h1);
        // Tamper with the stored file, then put the same content again: the
        // store must NOT overwrite it (write-once / immutable address).
        std::fs::write(&path, b"TAMPERED").unwrap();
        let h2 = store.put(b"same").unwrap();
        assert_eq!(h1, h2);
        assert_eq!(std::fs::read(&path).unwrap(), b"TAMPERED");
    }

    #[test]
    fn distinct_content_distinct_address() {
        let store = BlobStore::new(temp_root("dedup"));
        let a = store.put(b"alpha").unwrap();
        let b = store.put(b"beta").unwrap();
        assert_ne!(a, b);
        assert!(store.contains(&a) && store.contains(&b));
    }
}
