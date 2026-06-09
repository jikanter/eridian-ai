//! SPEC-001 §6: the record-time redaction gate.
//!
//! The trace MUST NOT contain plaintext secrets. This module builds the
//! `session.start.env_subset` map — restricted to an allowlist of
//! behavior-relevant vars — and scrubs any value that looks like an API key
//! *before* it is handed to the writer (acceptance criterion 5). Secrets never
//! reach disk because they never enter the event in the first place.

use indexmap::IndexMap;

/// The only env vars that may appear in `session.start.env_subset` (SPEC §6).
/// API-key vars are deliberately absent: `env_subset` never carries secrets.
pub const ENV_ALLOWLIST: &[&str] = &[
    "AICHAT_CONFIG_DIR",
    "AICHAT_FIXTURE_ID",
    "AICHAT_TRACE",
    "AICHAT_TRACE_DIR",
    "AICHAT_TRACE_VERBOSE",
    "OPENAI_API_BASE",
    "ANTHROPIC_API_BASE",
    "HOME",
    "PWD",
    "USER",
    "LANG",
    "LC_ALL",
];

/// Key-name suffixes that mark a secret-bearing variable (SPEC §6).
const SECRET_KEY_SUFFIXES: &[&str] = &["_API_KEY", "_TOKEN", "_SECRET", "_PASSWORD"];

/// Value prefixes that mark secret material regardless of the key name.
const SECRET_VALUE_PREFIXES: &[&str] = &["sk-", "xai-", "pk_", "Bearer "];

/// True if `(key, value)` looks like secret material per the SPEC §6 patterns.
pub fn is_secret(key: &str, value: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    if SECRET_KEY_SUFFIXES.iter().any(|s| upper.ends_with(s)) {
        return true;
    }
    SECRET_VALUE_PREFIXES.iter().any(|p| value.starts_with(p))
}

/// Redact a value if it (or its key) is secret-bearing, else pass it through.
pub fn redact_value(key: &str, value: &str) -> String {
    if is_secret(key, value) {
        format!("<redacted:{key}>")
    } else {
        value.to_string()
    }
}

/// Build the redacted `env_subset` from a generic getter (injected for tests).
/// Only allowlisted, present vars are included; each value is run through the
/// redaction gate.
pub fn build_env_subset<F>(get: F) -> IndexMap<String, String>
where
    F: Fn(&str) -> Option<String>,
{
    let mut out = IndexMap::new();
    for &key in ENV_ALLOWLIST {
        if let Some(val) = get(key) {
            out.insert(key.to_string(), redact_value(key, &val));
        }
    }
    out
}

/// Build `env_subset` from the real process environment.
pub fn env_subset_from_process() -> IndexMap<String, String> {
    build_env_subset(|k| std::env::var(k).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_non_allowlisted_vars() {
        let env: std::collections::HashMap<&str, &str> =
            [("HOME", "/home/u"), ("SOME_RANDOM_VAR", "value")].into();
        let subset = build_env_subset(|k| env.get(k).map(|v| v.to_string()));
        assert!(subset.contains_key("HOME"));
        assert!(!subset.contains_key("SOME_RANDOM_VAR"));
    }

    #[test]
    fn redacts_secret_valued_allowlisted_var() {
        // Even an allowlisted base-url var is scrubbed if its value looks like a key.
        let env: std::collections::HashMap<&str, &str> =
            [("OPENAI_API_BASE", "sk-abc123secret")].into();
        let subset = build_env_subset(|k| env.get(k).map(|v| v.to_string()));
        assert_eq!(subset["OPENAI_API_BASE"], "<redacted:OPENAI_API_BASE>");
    }

    #[test]
    fn no_subset_value_contains_known_key_prefix() {
        // Acceptance criterion 5: env_subset carries no plaintext key material.
        let env: std::collections::HashMap<&str, &str> = [
            ("HOME", "/home/u"),
            ("OPENAI_API_BASE", "Bearer xyz"),
            ("ANTHROPIC_API_BASE", "xai-9999"),
        ]
        .into();
        let subset = build_env_subset(|k| env.get(k).map(|v| v.to_string()));
        for v in subset.values() {
            for p in SECRET_VALUE_PREFIXES {
                assert!(!v.starts_with(p), "leaked secret-prefixed value: {v}");
            }
        }
    }

    #[test]
    fn key_name_suffix_triggers_redaction() {
        assert!(is_secret("OPENAI_API_KEY", "anything"));
        assert!(is_secret("github_token", "ghp_xxx")); // case-insensitive
        assert!(is_secret("DB_PASSWORD", "hunter2"));
        assert!(!is_secret("OPENAI_API_BASE", "http://localhost:1234"));
        assert_eq!(
            redact_value("OPENAI_API_KEY", "sk-real"),
            "<redacted:OPENAI_API_KEY>"
        );
    }

    #[test]
    fn passes_through_benign_values() {
        assert_eq!(redact_value("HOME", "/home/u"), "/home/u");
        assert_eq!(
            redact_value("OPENAI_API_BASE", "http://localhost:1234"),
            "http://localhost:1234"
        );
    }
}
