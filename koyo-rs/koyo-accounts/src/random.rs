//! The crate's one source of randomness, for what cswap draws from `random` and `secrets`: the
//! poll plans' jitter (`poll_policy::random_unit`), the claim ids of the usage store, the stash's
//! entry nonces, the atomic writer's temp names and the lock waits' jitter. None of them is a
//! secret, so no cryptographic generator is needed.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher as _, Hasher as _};
use std::sync::atomic::{AtomicU64, Ordering};

/// 64 random bits: the standard library's per-process random hash keys over a counter.
pub(crate) fn random_u64() -> u64 {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNT.fetch_add(1, Ordering::Relaxed));
    hasher.write_u32(std::process::id());
    hasher.finish()
}
