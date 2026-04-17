# Phase 10B: Pipeline Stage Output Cache

*2026-04-17T00:41:24Z by Showboat 0.6.1*
<!-- showboat-id: 2f91e075-ca87-44e0-aa55-094d7bbf6c4a -->

Phase 10B adds a content-addressable cache for pipeline stage outputs. Key = SHA-256 of (role_name ++ \0 ++ model_id ++ \0 ++ input_text). Entries live in `$CONFIG/.cache/stages/<key>.out` with a TTL of 3600s by default. On re-run of a stage whose triple matches, the LLM call is skipped entirely and the cached output is replayed as if fresh. The cache is a hard dependency for Phase 25 (Knowledge Compilation), which reuses this cache layer for compiled knowledge bases.

## StageCache module

```bash
grep -nE "pub fn (key|get|put|new)" src/cache.rs
```

```output
23:    pub fn new(dir: PathBuf, ttl_secs: u64) -> Self {
32:    pub fn key(role: &str, model: &str, input: &str) -> String {
42:    pub fn get(&self, key: &str) -> Option<String> {
52:    pub fn put(&self, key: &str, output: &str) -> Result<()> {
```

## Unit tests: determinism, delimiter safety, TTL, roundtrip

```bash
cargo test --bin aichat -- cache::tests 2>&1 | grep -E "^test cache|^test result" | sed "s/finished in [0-9.]*s/finished in Xs/" | sort
```

```output
test cache::tests::get_returns_none_after_ttl_expiry ... ok
test cache::tests::get_returns_none_on_miss ... ok
test cache::tests::key_delimiter_prevents_concatenation_collision ... ok
test cache::tests::key_is_deterministic ... ok
test cache::tests::key_varies_by_role_model_and_input ... ok
test cache::tests::put_creates_dir_if_missing ... ok
test cache::tests::put_preserves_multiline_unicode ... ok
test cache::tests::put_then_get_roundtrip ... ok
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 223 filtered out; finished in Xs
```

## Integration points in pipe.rs

```bash
grep -n "Phase 10B\|StageCache\|cache_key" src/pipe.rs
```

```output
1:use crate::cache::StageCache;
191:    // Phase 10B: content-addressable stage output cache. Skips when caching is
197:    let cache_key = if cache_enabled {
198:        Some(StageCache::key(
206:    if let Some(key) = &cache_key {
207:        let cache = StageCache::new(
315:    // Phase 10B: persist successful output to the cache. Written before
318:    if let Some(key) = &cache_key {
319:        let cache = StageCache::new(
```

Cache is bypassed for: tool-using stages (non-deterministic), dry-run (no output), and when the user passes `--no-cache` on the CLI.

## CLI flag

```bash
./target/debug/aichat --help 2>&1 | grep -A1 "no-cache"
```

```output
      --no-cache
          Bypass the pipeline stage output cache (Phase 10B)
```

## Full test suite

```bash
cargo test --bin aichat 2>&1 | grep "^test result" | tail -1 | sed "s/finished in [0-9.]*s/finished in Xs/"
```

```output
test result: ok. 231 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in Xs
```
