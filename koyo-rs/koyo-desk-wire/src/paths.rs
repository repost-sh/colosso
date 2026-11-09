//! Where the app, the helper, the holder and the host meet on disk (`docs/agent-desktop.md`
//! 5.3), and the names launchd and the directory know them by.

/// The agent account's short name, and its group's.
pub const ACCOUNT: &str = "koyo-agents";
/// The account's home.
pub const ACCOUNT_HOME: &str = "/Users/koyo-agents";
/// The account's full name, as Users & Groups would show it.
pub const ACCOUNT_FULL_NAME: &str = "Koyo Agents";
/// The first group id tried for the account's group; the first free one from here is taken.
pub const FIRST_GROUP_ID: u32 = 600;

/// The shared folder, root's, which both accounts can enter.
pub const SHARED: &str = "/Library/Application Support/Koyo/Agent Desktop";
/// When the account's last session ended (its time), root's: the helper's, which ends every
/// session, so the rule of [`AFTER_AN_END`] holds whoever asks.
pub const LAST_END: &str = "/Library/Application Support/Koyo/Agent Desktop/last-end";
/// How long after a session ends the next login may start (`docs/agent-desktop.md` 6.1, the
/// freeze of 2026-11-09).
pub const AFTER_AN_END: std::time::Duration = std::time::Duration::from_secs(60);
/// The helper's record of the setup: the person's uid, the account, its password.
pub const STATE: &str = "/Library/Application Support/Koyo/Agent Desktop/state.json";
/// Where the host the account runs is installed, root's and read-only to everyone else.
pub const HOST_DIR: &str = "/Library/Application Support/Koyo/Agent Desktop/host";
/// The host bundle's name, inside `Koyo.app/Contents/Helpers` and in [`HOST_DIR`].
pub const HOST_APP: &str = "Koyo Agent Desktop.app";
/// The folder of the app's link socket: the person's, the account's group may enter it.
pub const LINK_DIR: &str = "/Library/Application Support/Koyo/Agent Desktop/link";
/// The app's link socket, which the host connects to.
pub const LINK_SOCKET: &str = "/Library/Application Support/Koyo/Agent Desktop/link/desk.sock";
/// Copy-on-write sends both ways (`docs/agent-desktop.md` 7.8).
pub const STAGING: &str = "/Library/Application Support/Koyo/Agent Desktop/staging";

/// The helper daemon's launchd label.
pub const HELPER_LABEL: &str = "dev.koyo.agent-desktop.helper";
/// The helper's installed copy.
pub const HELPER_BIN: &str = "/Library/PrivilegedHelperTools/dev.koyo.agent-desktop.helper";
/// The helper daemon's plist.
pub const HELPER_PLIST: &str = "/Library/LaunchDaemons/dev.koyo.agent-desktop.helper.plist";
/// The helper daemon's socket, which answers only the person's uid.
pub const HELPER_SOCKET: &str = "/var/run/dev.koyo.agent-desktop.helper.sock";

/// The host's launchd label in the account's GUI domain.
pub const HOST_LABEL: &str = "dev.koyo.agent-desktop.host";
/// The host bundle's identifier.
pub const HOST_BUNDLE_ID: &str = "dev.koyo.agent-desktop.host";
/// The host's launch agent plist, relative to the account's home.
pub const HOST_AGENT_PLIST: &str = "Library/LaunchAgents/dev.koyo.agent-desktop.host.plist";
/// The host's library, beside the bundle, never in it: the bundle's seal covers what's inside, so
/// a new library inside would change the loader's signature and void its privacy grants. The
/// loader loads it from its own folder only, and only when root owns it and nobody else can write
/// to it or its folder.
pub const HOST_LIBRARY: &str = "libkoyo_desk_host.dylib";
/// The loader's executable, inside the bundle.
pub const HOST_EXECUTABLE: &str = "Contents/MacOS/Koyo Agent Desktop";

/// The pf anchor that keeps Screen Sharing on the loopback interface.
pub const PF_ANCHOR: &str = "com.apple/900.Koyo";
/// The anchor's one rule.
pub const PF_RULE: &str =
    "block drop in quick on ! lo0 proto { tcp udp } from any to any port { 5900, 3283 }\n";

/// Screen Sharing's port, reached on the loopback interface only.
pub const SCREEN_SHARING_PORT: u16 = 5900;

/// The oldest macOS whose `screensharingd` has no known pre-authentication bypass
/// (CVE-2026-65400 is fixed in 26.6.1).
pub const MIN_MACOS: (u32, u32, u32) = (26, 6, 1);

/// Work folders in the account's home, one per area: `Work.noindex/agent-<n>`, which Spotlight
/// leaves alone.
pub const WORK: &str = "Work.noindex";

/// Why `version` refuses agent desktops, when it does: older than [`MIN_MACOS`], unless a
/// developer accepted that risk at setup.
pub fn macos_refusal(version: &str, accepted_risk: bool) -> Option<String> {
    let (major, minor, patch) = MIN_MACOS;
    (!macos_is_safe(version) && !accepted_risk).then(|| {
        format!(
            "macOS {version} has a Screen Sharing flaw that lets anyone in without a password \
             (CVE-2026-65400, fixed in {major}.{minor}.{patch}); update macOS first"
        )
    })
}

/// Whether `version` ("26.4", "26.6.1") is at least [`MIN_MACOS`]. A version that doesn't
/// read as numbers isn't.
pub fn macos_is_safe(version: &str) -> bool {
    let mut parts = version.trim().split('.').map(str::parse::<u32>);
    let mut next = || parts.next().unwrap_or(Ok(0));
    match (next(), next(), next()) {
        (Ok(major), Ok(minor), Ok(patch)) => (major, minor, patch) >= MIN_MACOS,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_refusal_names_the_fix_unless_the_risk_was_accepted() {
        assert!(macos_refusal("26.4", false).unwrap().contains("26.6.1"));
        assert_eq!(macos_refusal("26.4", true), None);
        assert_eq!(macos_refusal("27.2", false), None);
    }

    #[test]
    fn macos_older_than_the_fix_is_refused() {
        assert!(!macos_is_safe("26.4"));
        assert!(!macos_is_safe("26.6"));
        assert!(macos_is_safe("26.6.1"));
        assert!(macos_is_safe("26.7.2"));
        assert!(macos_is_safe("27.0"));
        assert!(!macos_is_safe("15.7.3"));
        assert!(!macos_is_safe("26.x"));
        assert!(!macos_is_safe(""));
    }

    #[test]
    fn the_shared_paths_are_under_the_shared_folder() {
        for path in [STATE, HOST_DIR, LINK_DIR, LINK_SOCKET, STAGING] {
            assert!(path.starts_with(SHARED), "{path}");
        }
        assert!(LINK_SOCKET.starts_with(LINK_DIR));
        // A socket address holds 104 bytes on macOS.
        assert!(LINK_SOCKET.len() < 104 && HELPER_SOCKET.len() < 104);
    }
}
