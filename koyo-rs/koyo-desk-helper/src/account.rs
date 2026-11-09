//! The agent account (`docs/agent-desktop.md` 4): hidden, standard, a group of its own and
//! out of `staff`, its home closed to others, first login skipped, trimmed.

use koyo_desk_wire::paths;

use crate::work::{Outcome, Work};

/// The per-user launch agents switched off for the account: what costs a session memory and
/// CPU and serves no agent (the `PoC`'s `trim-labels.txt`). Dock, Finder, `WindowManager`,
/// `ControlCenter`, `SystemUIServer`, text input, spelling, icon services, `replayd`, `trustd`,
/// fonts and the pasteboard stay.
pub const TRIMMED: &[&str] = &[
    "com.apple.Siri.agent",
    "com.apple.assistantd",
    "com.apple.assistant_service",
    "com.apple.sirittsd",
    "com.apple.siriactionsd",
    "com.apple.siriinferenced",
    "com.apple.siriknowledged",
    "com.apple.SiriTTSTrainingAgent",
    "com.apple.siri.context.service",
    "com.apple.knowledgeconstructiond",
    "com.apple.knowledge-agent",
    "com.apple.BiomeAgent",
    "com.apple.biomesyncd",
    "com.apple.duetexpertd",
    "com.apple.ContextStoreAgent",
    "com.apple.linkd",
    "com.apple.suggestd",
    "com.apple.proactiveeventtrackerd",
    "com.apple.intelligenceplatformd",
    "com.apple.intelligencecontextd",
    "com.apple.generativeexperiencesd",
    "com.apple.spotlightknowledged",
    "com.apple.spotlightknowledged.updater",
    "com.apple.spotlightknowledged.importer",
    "com.apple.Spotlight",
    "com.apple.chronod",
    "com.apple.notificationcenterui.agent",
    "com.apple.mediaanalysisd",
    "com.apple.photoanalysisd",
    "com.apple.photolibraryd",
    "com.apple.mediastream.mstreamd",
    "com.apple.AMPLibraryAgent",
    "com.apple.AMPArtworkAgent",
    "com.apple.amsengagementd",
    "com.apple.homed",
    "com.apple.routined",
    "com.apple.telephonyutilities.callservicesd",
    "com.apple.imagent",
    "com.apple.identityservicesd",
    "com.apple.icloud.searchpartyuseragent",
    "com.apple.icloud.findmydeviced.findmydevice-user-agent",
    "com.apple.passd",
    "com.apple.remindd",
    "com.apple.CalendarAgent",
    "com.apple.cloudd",
    "com.apple.bird",
    "com.apple.studentd",
    "com.apple.gamed",
    "com.apple.newsd",
    "com.apple.tipsd",
    "com.apple.ScreenTimeAgent",
    "com.apple.familycircled",
    "com.apple.sharingd",
    "com.apple.rapportd",
    "com.apple.followupd",
    "com.apple.parsecd",
    "com.apple.weatherd",
    "com.apple.geod",
    "com.apple.Maps.mapssyncd",
    "com.apple.contactsd",
    "com.apple.AddressBook.SourceSync",
    "com.apple.CallHistoryPluginHelper",
    "com.apple.ap.adprivacyd",
    "com.apple.ap.promotedcontentd",
    "com.apple.UsageTrackingAgent",
    "com.apple.TrustedPeersHelper",
    "com.apple.wallpaper.agent",
];

/// Setup Assistant's panes, marked seen so the first login stops on none
/// (Rocuronium's `provision-agent-account.sh` has the list).
const SEEN: &[&str] = &[
    "DidSeeAccessibility",
    "DidSeeActivationLock",
    "DidSeeAppearanceSetup",
    "DidSeeApplePaySetup",
    "DidSeeAppStore",
    "DidSeeCloudSetup",
    "DidSeeiCloudLoginForStorageServices",
    "DidSeeLockdownMode",
    "DidSeePrivacy",
    "DidSeeScreenTime",
    "DidSeeSiriSetup",
    "DidSeeSyncSetup",
    "DidSeeSyncSetup2",
    "DidSeeTermsOfAddress",
    "DidSeeTouchIDSetup",
];
const SEEN_VERSION: &[&str] = &[
    "LastSeenAgeRangeSelectionProductVersion",
    "LastSeenCloudProductVersion",
    "LastSeenDiagnosticsProductVersion",
    "LastSeeniCloudStorageServicesProductVersion",
    "LastSeenIntelligenceProductVersion",
    "LastSeenSiriProductVersion",
    "LastSeenSyncProductVersion",
];

/// `key`'s value in a directory record (`/Users/x`, `/Groups/y`), as `dscl . -read` says it.
fn read(work: &mut Work, record: &str, key: &str) -> Option<String> {
    let ran = work.run("/usr/bin/dscl", &[".", "-read", record, key]);
    let prefix = format!("{key}:");
    ran.ok()
        .then(|| {
            ran.out
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .map(|value| value.trim().to_owned())
        })
        .flatten()
}

pub fn uid(work: &mut Work, name: &str) -> Option<u32> {
    read(work, &format!("/Users/{name}"), "UniqueID")?
        .parse()
        .ok()
}

pub fn home(work: &mut Work, name: &str) -> Option<String> {
    read(work, &format!("/Users/{name}"), "NFSHomeDirectory")
}

pub fn group_id(work: &mut Work, name: &str) -> Option<u32> {
    read(work, &format!("/Groups/{name}"), "PrimaryGroupID")?
        .parse()
        .ok()
}

/// The first group id from `from` no group has.
pub fn free_group_id(work: &mut Work, from: u32) -> u32 {
    let ran = work.run(
        "/usr/bin/dscl",
        &[".", "-list", "/Groups", "PrimaryGroupID"],
    );
    let taken: Vec<u32> = ran
        .out
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1)?.parse().ok())
        .collect();
    (from..from.saturating_add(10_000))
        .find(|id| !taken.contains(id))
        .unwrap_or(from)
}

/// The group, made when missing. Its id.
pub fn ensure_group(work: &mut Work) -> Outcome<u32> {
    if let Some(gid) = group_id(work, paths::ACCOUNT) {
        return Ok(gid);
    }
    let gid = free_group_id(work, paths::FIRST_GROUP_ID);
    work.must(
        "making the group",
        "/usr/sbin/dseditgroup",
        &[
            "-o",
            "create",
            "-r",
            paths::ACCOUNT_FULL_NAME,
            "-i",
            &gid.to_string(),
            paths::ACCOUNT,
        ],
    )?;
    work.did(format!("made the group {} ({gid})", paths::ACCOUNT));
    Ok(gid)
}

/// 32 random letters and digits, from the kernel's generator.
pub fn new_password() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut bytes = [0u8; 32];
    // SAFETY: a valid buffer of its stated length.
    unsafe { libc::arc4random_buf(bytes.as_mut_ptr().cast(), bytes.len()) };
    bytes
        .iter()
        .map(|byte| char::from(ALPHABET[usize::from(*byte) % ALPHABET.len()]))
        .collect()
}

/// Makes the account with `password`. Its uid; the caller records it before anything else, so
/// a setup stopped after this finds it its own when run again.
pub fn add(work: &mut Work, password: &str) -> Outcome<u32> {
    let name = paths::ACCOUNT;
    let ran = work.answering(
        "making the account",
        "/usr/sbin/sysadminctl",
        &[
            "-addUser",
            name,
            "-fullName",
            paths::ACCOUNT_FULL_NAME,
            "-password",
            "-",
        ],
        "User password:",
        password,
    )?;
    let uid = uid(work, name)
        .ok_or_else(|| format!("making the account: sysadminctl said {}", ran.said()))?;
    work.did(format!("made the account {name} (uid {uid})"));
    Ok(uid)
}

/// Whether `password` logs the account in, as the holder's login will ask.
pub fn check_password(work: &mut Work, password: &str) -> Outcome<()> {
    let name = paths::ACCOUNT;
    let checked = work.answering(
        "checking the account's password",
        "/usr/bin/dscl",
        &[".", "-authonly", name],
        "Password:",
        password,
    )?;
    if !checked.ok() {
        return Err(format!(
            "the account {name} was made, but its password doesn't log in ({}); remove it and set up again",
            checked.said()
        ));
    }
    work.did("checked the account's password");
    Ok(())
}

/// Hidden, its own group, out of `staff`, its home its own and closed to others, no shared
/// Public folder.
pub fn close_off(work: &mut Work, gid: u32) -> Outcome<()> {
    let name = paths::ACCOUNT;
    let record = format!("/Users/{name}");
    work.must(
        "hiding the account",
        "/usr/bin/dscl",
        &[".", "-create", &record, "IsHidden", "1"],
    )?;
    work.must(
        "giving the account its group",
        "/usr/bin/dscl",
        &[".", "-create", &record, "PrimaryGroupID", &gid.to_string()],
    )?;
    // Not a member is fine: there's nothing to take it out of.
    work.run(
        "/usr/sbin/dseditgroup",
        &["-o", "edit", "-d", name, "-t", "user", "staff"],
    );
    work.run("/usr/sbin/createhomedir", &["-c", "-u", name]);
    let home = home(work, name).ok_or("the account has no home")?;
    // Some of a new account's Library refuses even root's chown ("Autosave Information");
    // what matters is the home itself.
    work.run("/usr/sbin/chown", &["-R", &format!("{name}:{name}"), &home]);
    work.chown("owning the home", &format!("{name}:{name}"), &home)?;
    work.chmod("closing the home", "750", &home)?;
    work.did(format!(
        "hid {name}, gave it the group {name}, its home {home} mode 750"
    ));
    unshare_public(work, &home);
    Ok(())
}

/// The share point macOS makes of a new account's Public folder, and its group.
pub fn unshare_public(work: &mut Work, home: &str) {
    let listed = work.run("/usr/sbin/sharing", &["-l"]);
    let public = format!("{home}/Public");
    let mut name = None;
    for line in listed.out.lines() {
        if let Some(value) = line.strip_prefix("name:") {
            name = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("path:")
            && value.trim() == public
            && let Some(share) = name.take()
        {
            work.run("/usr/sbin/sharing", &["-r", &share]);
            work.did(format!("removed the share point “{share}”"));
            for group in share_groups(work, &share) {
                work.run(
                    "/usr/bin/dscl",
                    &[".", "-delete", &format!("/Groups/{group}")],
                );
                work.did(format!("deleted the share group {group}"));
            }
        }
    }
}

/// The `com.apple.sharepoint.group.*` groups named `share`.
fn share_groups(work: &mut Work, share: &str) -> Vec<String> {
    let listed = work.run("/usr/bin/dscl", &[".", "-list", "/Groups"]);
    listed
        .out
        .lines()
        .filter(|group| group.starts_with("com.apple.sharepoint.group"))
        .filter(|group| {
            read(work, &format!("/Groups/{group}"), "RealName")
                .is_some_and(|name| name.contains(share))
        })
        .map(str::to_owned)
        .collect()
}

/// The account's preferences besides Setup Assistant's: nothing reopened at login, no lock
/// for its screen saver, Siri and its menu off, the Dock hidden, no widgets on the desktop.
const PREFERENCES: &[(&str, &str, bool)] = &[
    (
        "com.apple.SetupAssistant",
        "SkipFirstLoginOptimization",
        true,
    ),
    (
        "com.apple.SetupAssistant",
        "MiniBuddyShouldLaunchToResumeSetup",
        false,
    ),
    ("com.apple.assistant.support", "Assistant Enabled", false),
    ("com.apple.Siri", "StatusMenuVisible", false),
    ("com.apple.screensaver", "askForPassword", false),
    ("com.apple.loginwindow", "TALLogoutSavesState", false),
    (
        "com.apple.loginwindow",
        "LoginwindowLaunchesRelaunchApps",
        false,
    ),
    ("com.apple.dock", "autohide", true),
    ("com.apple.dock", "show-recents", false),
    ("com.apple.WindowManager", "StandardHideWidgets", true),
];

/// First login skipped, nothing reopened, no idle lock (it would curtain the session), Siri
/// and suggestions off: written as the account.
pub fn preferences(work: &mut Work, macos: &str, build: &str) {
    let mut write = |args: &[&str]| {
        let mut all = vec!["write"];
        all.extend_from_slice(args);
        work.run_as(paths::ACCOUNT, "/usr/bin/defaults", &all);
    };
    for key in SEEN {
        write(&["com.apple.SetupAssistant", key, "-bool", "true"]);
    }
    for key in SEEN_VERSION {
        write(&["com.apple.SetupAssistant", key, "-string", macos]);
    }
    write(&[
        "com.apple.SetupAssistant",
        "LastSeenBuddyBuildVersion",
        "-string",
        build,
    ]);
    for (domain, key, on) in PREFERENCES {
        write(&[domain, key, "-bool", if *on { "true" } else { "false" }]);
    }
    work.run_as(
        paths::ACCOUNT,
        "/usr/bin/defaults",
        &[
            "-currentHost",
            "write",
            "com.apple.screensaver",
            "idleTime",
            "-int",
            "0",
        ],
    );
    work.did("skipped the account's first login, no screen saver, nothing reopened");
}

/// The trimmed launch agents switched off for `uid`, with every third-party agent installed for
/// all users (`/Library/LaunchAgents`), which serve no agent either. Through its `user/<uid>`
/// domain, before each login: `gui/<uid>` exists only while the account is logged in, and a
/// `disable` there without it fails (`125`, 27.2), while `user/<uid>` writes the account's
/// overrides file, which its next session reads. The two domains share that file and each writes
/// only its own view, so it's written again before every login (`docs/agent-desktop.md` 4).
/// Removal deletes the file.
pub fn trim(work: &mut Work, uid: u32) {
    let listed = work.run("/bin/ls", &["/Library/LaunchAgents"]);
    let third_party = listed
        .out
        .lines()
        .filter_map(|file| file.strip_suffix(".plist"))
        .filter(|label| !label.starts_with("com.apple."))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let labels = TRIMMED
        .iter()
        .copied()
        .chain(third_party.iter().map(String::as_str));
    let mut count = 0;
    for label in labels {
        work.run(
            "/bin/launchctl",
            &["disable", &format!("user/{uid}/{label}")],
        );
        count += 1;
    }
    work.did(format!(
        "switched off {count} launch agents for the account"
    ));
}

/// Whether `name` is an administrator.
pub fn is_admin(work: &mut Work, name: &str) -> bool {
    let ran = work.run(
        "/usr/sbin/dseditgroup",
        &["-o", "checkmember", "-m", name, "admin"],
    );
    ran.ok() && ran.out.starts_with("yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_long_and_never_the_same() {
        let (a, b) = (new_password(), new_password());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn values_read_from_dscl() {
        use crate::script::Script;
        let mut mac = Script::default()
            .on("dscl . -read /Users/a UniqueID", 0, "UniqueID: 503\n")
            .on(
                "dscl . -read /Users/b UniqueID",
                56,
                "No such key: UniqueID\n",
            );
        let mut work = Work::new(&mut mac);
        assert_eq!(uid(&mut work, "a"), Some(503));
        assert_eq!(uid(&mut work, "b"), None);
    }
}
