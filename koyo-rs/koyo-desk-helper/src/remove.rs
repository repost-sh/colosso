//! Removing everything setup made (`docs/agent-desktop.md` 5.5), in the one safe order: the
//! session first, and the account only once no process of it is left.

use koyo_desk_wire::helper::SetupState;
use koyo_desk_wire::paths;

use crate::mac::Ran;
use crate::work::{Outcome, Work};
use crate::{account, firewall, session, sharing};

/// An administrator's name and password: root can't delete a user record on macOS 26.4.
pub struct Admin<'a> {
    pub name: &'a str,
    pub password: &'a str,
}

impl Admin<'_> {
    /// Runs `dscl` as the administrator, answering its password prompt.
    fn dscl(&self, work: &mut Work, what: &str, args: &[&str]) -> Outcome<Ran> {
        let mut all = vec!["-u", self.name];
        all.extend_from_slice(args);
        work.answering(what, "/usr/bin/dscl", &all, "Password:", self.password)
    }
}

/// Checks `admin` is an administrator whose password this is, before anything is touched.
pub fn check(work: &mut Work, admin: &Admin) -> Outcome<()> {
    if !account::is_admin(work, admin.name) {
        return Err(format!("{} isn't an administrator of this Mac", admin.name));
    }
    let checked = work.answering(
        "checking the administrator's password",
        "/usr/bin/dscl",
        &[".", "-authonly", admin.name],
        "Password:",
        admin.password,
    )?;
    if !checked.ok() {
        return Err(format!("that isn't {}'s password", admin.name));
    }
    Ok(())
}

/// Removes it all. Without `admin`, the user record stays and the message says how to finish.
pub fn run(work: &mut Work, state: &SetupState, admin: Option<&Admin>) -> Outcome<()> {
    let name = paths::ACCOUNT;
    if let Some(admin) = admin {
        check(work, admin)?;
    }
    session::end_all(work, state.uid)?;
    sharing::disallow(work, name);
    sharing::off(work);
    let home = account::home(work, name);
    if let Some(home) = &home {
        account::unshare_public(work, home);
    }
    work.run("/usr/sbin/sysadminctl", &["-deleteUser", name]);
    let record = format!("/Users/{name}");
    if account::uid(work, name).is_some() {
        let Some(admin) = admin else {
            return Err(format!(
                "everything but the account's user record is gone; finish with \
                 sudo dscl -u <you> . -delete {record}"
            ));
        };
        let deleted = admin.dscl(work, "deleting the user record", &[".", "-delete", &record])?;
        if account::uid(work, name).is_some() {
            return Err(format!(
                "the user record {record} stayed: dscl said {}",
                deleted.said()
            ));
        }
    }
    work.did(format!("deleted the account {name}"));
    // Its home, if sysadminctl left it, but only the account's own path.
    if home.as_deref() == Some(paths::ACCOUNT_HOME) {
        work.remove("removing the account's home", paths::ACCOUNT_HOME)?;
    }
    // What `account::trim` switched off, gone with it.
    work.remove(
        "removing the account's launchd overrides",
        &format!("/var/db/com.apple.xpc.launchd/disabled.{}.plist", state.uid),
    )?;
    let group = format!("/Groups/{name}");
    if !work.run("/usr/bin/dscl", &[".", "-delete", &group]).ok()
        && let Some(admin) = admin
    {
        admin.dscl(work, "deleting the group", &[".", "-delete", &group])?;
    }
    work.did(format!("deleted the group {name}"));
    work.run("/usr/bin/dscacheutil", &["-flushcache"]);
    firewall::release(work, state.pf_token.as_deref());
    work.run("/usr/bin/tccutil", &["reset", "All", paths::HOST_BUNDLE_ID]);
    work.did("reset the host's privacy grants");
    work.remove("removing the shared folder", paths::SHARED)?;
    work.remove("removing the daemon's plist", paths::HELPER_PLIST)?;
    work.remove("removing the helper", paths::HELPER_BIN)?;
    if state.made_helper_tools {
        // Only if empty: another app's tools may have come since.
        work.run("/bin/rmdir", &["/Library/PrivilegedHelperTools"]);
    }
    work.did(format!(
        "removed {}, the daemon and the helper",
        paths::SHARED
    ));
    Ok(())
}
