//! Claude Code accounts, with no UI (`docs/claude-accounts.md`): several logins kept in Koyo's
//! store, one of them made the Mac's login, each one's usage, and auto-switch.
//!
//! A port of claude-swap (`cswap`) at commit 0fe22a8, not a redesign: its rules, its numbers
//! and the order of its steps, each module opening with the cswap module it ports. Where a
//! rule couldn't be ported as it is, the module says so. The app wires it in
//! `koyo/src/accounts/`.
//!
//! Everything it reads from the Mac comes through an [`Env`]: the home folder, Claude Code's
//! variables, the store, `security`, HTTP and the clock. Tests run on a sandbox
//! (`testing::Sandbox`), whose writes the guard holds inside its folder and off the real login.

pub mod claude_locks;
pub mod credentials;
pub mod env;
pub mod error;
pub mod fsutil;
pub mod guard;
pub mod http;
pub mod keychain;
pub mod locking;
pub mod paths;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use env::{Clock, Env, Vars};
pub use error::{AccountsError, Result};
