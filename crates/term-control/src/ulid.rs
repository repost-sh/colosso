//! Handles are `term_` and a ULID: 48 bits of milliseconds since the epoch, 80 random bits,
//! as 26 characters of Crockford base32, so they sort by creation and never repeat.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::terminal::Handle;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Milliseconds since the epoch, the clock every record is stamped with.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// A new ULID for the current time.
pub fn ulid() -> String {
    ulid_at(now_ms(), fastrand::u128(..))
}

/// The ULID for a time and 80 bits of randomness (the top 48 of `random` are ignored).
pub fn ulid_at(ms: u64, random: u128) -> String {
    let value = (u128::from(ms & 0xFFFF_FFFF_FFFF) << 80) | (random & ((1 << 80) - 1));
    let mut out = [b'0'; 26];
    for (i, slot) in out.iter_mut().enumerate() {
        let shift = (25 - i) * 5;
        *slot = ALPHABET[((value >> shift) & 0x1f) as usize];
    }
    // The alphabet is ASCII, so this is always valid UTF-8.
    String::from_utf8_lossy(&out).into_owned()
}

impl Handle {
    /// A handle no terminal has had.
    pub fn generate() -> Handle {
        Handle(format!("term_{}", ulid()))
    }

    /// Whether `text` has a handle's shape, before looking it up.
    pub fn looks_valid(text: &str) -> bool {
        text.strip_prefix("term_")
            .is_some_and(|id| id.len() == 26 && id.bytes().all(|b| ALPHABET.contains(&b)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_the_known_vector() {
        // The ULID spec's example time, 2016-07-30T23:54:10.259Z, with zero randomness.
        assert_eq!(ulid_at(1_469_922_850_259, 0), "01ARZ3NDEK0000000000000000");
        assert_eq!(ulid_at(0, u128::MAX), "0000000000ZZZZZZZZZZZZZZZZ");
    }

    #[test]
    fn handles_sort_by_time_and_look_valid() {
        let earlier = Handle(format!("term_{}", ulid_at(1_000, 5)));
        let later = Handle(format!("term_{}", ulid_at(2_000, 0)));
        assert!(earlier < later);
        assert!(Handle::looks_valid(later.as_str()));
        assert!(Handle::looks_valid(Handle::generate().as_str()));
        assert!(!Handle::looks_valid("term_short"));
        assert!(!Handle::looks_valid("sess_01ARYZ6S410000000000000000"));
    }
}
