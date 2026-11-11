//! The usage sentinels of claude-swap's `json_output.py` (0fe22a8): the strings a collector
//! yields in place of a usage reading, kept in one place so every view and the store agree.
//! Sentinels are derived fresh on every pass and never written to the usage store.

/// The slot holds no credential.
pub const USAGE_NO_CREDENTIALS: &str = "no credentials";
/// The access token is past its expiry; Claude Code (or the refresh gate) may renew it.
pub const USAGE_TOKEN_EXPIRED: &str = "token expired";
/// An API-key account: no subscription quota, nothing to fetch.
pub const USAGE_API_KEY: &str = "api key";
/// The active account's Keychain was unreadable (locked, denied, timed out): distinct from an
/// empty slot, so nobody is sent to log in again for nothing.
pub const USAGE_KEYCHAIN_UNAVAILABLE: &str = "keychain unavailable";
/// The stored refresh-token lineage is dead (`invalid_grant`): quarantined until a re-login
/// replaces the credential, which only the person can do.
pub const USAGE_RELOGIN_REQUIRED: &str = "re-login needed";
/// The live credential was proven to belong to another account than the slot's: its quota is
/// not this slot's, and a switch repairs the drift.
pub const USAGE_FOREIGN_CREDENTIAL: &str = "foreign credential";
