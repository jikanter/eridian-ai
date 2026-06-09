//! ULID generation for SPEC-001 trace session ids.
//!
//! A ULID is a 128-bit value: a 48-bit big-endian millisecond timestamp
//! followed by 80 bits of randomness, rendered as 26 Crockford base32
//! characters. The encoding is lexicographically sortable, so trace files
//! named `turn-<ulid>.jsonl` sort chronologically (SPEC-001 §1).
//!
//! No new dependency: the timestamp comes from `SystemTime` and the 80 random
//! bits are drawn from a `uuid::Uuid::new_v4()` (already a crate dependency),
//! which is itself backed by the OS CSPRNG.

use std::time::{SystemTime, UNIX_EPOCH};

/// Crockford base32 alphabet (excludes I, L, O, U to avoid ambiguity).
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Encode a 48-bit millisecond timestamp + 80-bit randomness as a 26-char ULID.
///
/// Only the low 48 bits of `timestamp_ms` and the low 80 bits of `randomness`
/// are used; higher bits are masked off so the result is always a canonical
/// 128-bit ULID.
pub fn encode_ulid(timestamp_ms: u64, randomness: u128) -> String {
    let ts = (timestamp_ms as u128) & ((1u128 << 48) - 1);
    let rand = randomness & ((1u128 << 80) - 1);
    let value = (ts << 80) | rand;

    // 26 chars * 5 bits = 130 bits; the top 2 bits are always zero.
    let mut out = [0u8; 26];
    for (i, slot) in out.iter_mut().enumerate() {
        let shift = 5 * (25 - i);
        let idx = ((value >> shift) & 0x1f) as usize;
        *slot = CROCKFORD[idx];
    }
    // SAFETY: every byte is from CROCKFORD, which is ASCII.
    String::from_utf8(out.to_vec()).expect("CROCKFORD is ASCII")
}

/// Generate a fresh ULID from the current wall clock and OS randomness.
pub fn new_ulid() -> String {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let randomness = uuid::Uuid::new_v4().as_u128();
    encode_ulid(timestamp_ms, randomness)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_to_26_chars() {
        assert_eq!(encode_ulid(0, 0).len(), 26);
        assert_eq!(new_ulid().len(), 26);
    }

    #[test]
    fn all_zero_is_all_zero_chars() {
        assert_eq!(encode_ulid(0, 0), "00000000000000000000000000");
    }

    #[test]
    fn only_crockford_chars() {
        let s = new_ulid();
        for c in s.bytes() {
            assert!(
                CROCKFORD.contains(&c),
                "char {} not in Crockford alphabet",
                c as char
            );
        }
    }

    #[test]
    fn lexicographically_sorts_by_timestamp() {
        // Same randomness, increasing timestamp => increasing string order.
        let earlier = encode_ulid(1_000, 42);
        let later = encode_ulid(2_000, 42);
        assert!(later > earlier, "{later} should sort after {earlier}");
    }

    #[test]
    fn randomness_lands_in_low_bits() {
        // Two ULIDs with the same timestamp but different randomness differ.
        let a = encode_ulid(1_700_000_000_000, 1);
        let b = encode_ulid(1_700_000_000_000, 2);
        assert_ne!(a, b);
        // ...but share the timestamp-derived prefix (first 10 chars = 48 bits + 2 pad).
        assert_eq!(&a[..10], &b[..10]);
    }

    #[test]
    fn masks_oversized_inputs() {
        // Timestamp beyond 48 bits and randomness beyond 80 bits must not panic
        // and must still produce a canonical 26-char ULID.
        let s = encode_ulid(u64::MAX, u128::MAX);
        assert_eq!(s.len(), 26);
        for c in s.bytes() {
            assert!(CROCKFORD.contains(&c));
        }
    }
}
