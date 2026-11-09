//! Screen Sharing: on only while an agent session needs it, for the account alone
//! (`docs/agent-desktop.md` 5.1, 5.4).

use crate::work::{Outcome, Work};

const SERVICE: &str = "system/com.apple.screensharing";
const PLIST: &str = "/System/Library/LaunchDaemons/com.apple.screensharing.plist";
const ACCESS: &str = "com.apple.access_screensharing";

/// Whether Screen Sharing is loaded.
pub fn is_on(work: &mut Work) -> bool {
    work.run("/bin/launchctl", &["print", SERVICE]).ok()
}

pub fn on(work: &mut Work) -> Outcome<()> {
    work.must(
        "enabling Screen Sharing",
        "/bin/launchctl",
        &["enable", SERVICE],
    )?;
    if !is_on(work) {
        // Already loaded is no failure; anything else shows in `is_on` below.
        work.run("/bin/launchctl", &["bootstrap", "system", PLIST]);
    }
    if !is_on(work) {
        return Err("Screen Sharing didn't start".into());
    }
    work.did("Screen Sharing on");
    Ok(())
}

/// The override that would start Screen Sharing at the next boot, taken back; a running
/// service is left as it is.
pub fn disable_at_boot(work: &mut Work) {
    work.run("/bin/launchctl", &["disable", SERVICE]);
    work.did("Screen Sharing won't start at the next boot");
}

pub fn off(work: &mut Work) {
    work.run("/bin/launchctl", &["bootout", SERVICE]);
    work.run("/bin/launchctl", &["disable", SERVICE]);
    work.did("Screen Sharing off");
}

/// Whether the access list exists: without it, Screen Sharing lets every user in, as the
/// person chose, and there's no list to add the account to.
pub fn has_access_list(work: &mut Work) -> bool {
    work.run(
        "/usr/bin/dscl",
        &[".", "-read", &format!("/Groups/{ACCESS}")],
    )
    .ok()
}

pub fn allow(work: &mut Work, name: &str) -> Outcome<()> {
    if !has_access_list(work) {
        work.did("Screen Sharing lets every user in, as it was set; no access list to add to");
        return Ok(());
    }
    work.must(
        "adding the account to Screen Sharing's access list",
        "/usr/sbin/dseditgroup",
        &["-o", "edit", "-a", name, "-t", "user", ACCESS],
    )?;
    work.did(format!("{name} in Screen Sharing's access list"));
    Ok(())
}

pub fn disallow(work: &mut Work, name: &str) {
    work.run(
        "/usr/sbin/dseditgroup",
        &["-o", "edit", "-d", name, "-t", "user", ACCESS],
    );
    work.did(format!("{name} out of Screen Sharing's access list"));
}
