//! What the helper puts on disk (`docs/agent-desktop.md` 5.3): the shared folder, the host's
//! copy and its launch agent in the account, the daemon's copy and its plist, `state.json`.

use koyo_desk_wire::helper::SetupState;
use koyo_desk_wire::paths;

use crate::plist;
use crate::work::{Outcome, Work};

/// The shared folder, root's; `link/` the person's, entered by the account's group; `staging/`
/// root's, written by the group, and by the person through an access entry; `host/` root's.
pub fn shared_folder(work: &mut Work, person_uid: u32, person: &str, gid: u32) -> Outcome<()> {
    work.make_dir("making the shared folder", paths::SHARED, 0o755)?;
    work.chown("owning the shared folder", "root:wheel", paths::SHARED)?;
    work.make_dir("making the host's folder", paths::HOST_DIR, 0o755)?;
    work.make_dir("making the link's folder", paths::LINK_DIR, 0o750)?;
    work.chown(
        "owning the link's folder",
        &format!("{person_uid}:{gid}"),
        paths::LINK_DIR,
    )?;
    work.make_dir("making the staging folder", paths::STAGING, 0o1770)?;
    work.chown(
        "owning the staging folder",
        &format!("root:{gid}"),
        paths::STAGING,
    )?;
    // Cleared first, so setting up again adds no second entry.
    work.run("/bin/chmod", &["-N", paths::STAGING]);
    work.must(
        "letting the person stage files",
        "/bin/chmod",
        &[
            "+a",
            &format!("user:{person} allow list,search,add_file,add_subdirectory,delete_child"),
            paths::STAGING,
        ],
    )?;
    work.did(format!("made {} (host, link, staging)", paths::SHARED));
    Ok(())
}

/// The host bundle's version, when `bundle` is one: the loader and the library beside it, and
/// the host's identifier.
pub fn host_version(work: &mut Work, bundle: &str) -> Option<String> {
    let info = format!("{bundle}/Contents/Info.plist");
    // Both keys in one run: it prints the identifier, then the version, a line each.
    let ran = work.run(
        "/usr/libexec/PlistBuddy",
        &[
            "-c",
            "Print :CFBundleIdentifier",
            "-c",
            "Print :CFBundleShortVersionString",
            &info,
        ],
    );
    let mut said = ran.out.lines().map(str::trim);
    let identifier = said.next()?;
    let version = said.next().filter(|_| ran.ok())?.to_owned();
    let complete = work.exists(&format!("{bundle}/{}", paths::HOST_EXECUTABLE))
        && work.exists(&library_beside(bundle));
    (identifier == paths::HOST_BUNDLE_ID && complete).then_some(version)
}

/// The host's library, beside its bundle.
fn library_beside(bundle: &str) -> String {
    let folder = std::path::Path::new(bundle)
        .parent()
        .map_or_else(String::new, |folder| folder.display().to_string());
    format!("{folder}/{}", paths::HOST_LIBRARY)
}

/// The installed host's version.
pub fn installed_host(work: &mut Work) -> Option<String> {
    host_version(work, &format!("{}/{}", paths::HOST_DIR, paths::HOST_APP))
}

/// Copies the host bundle at `bundle` into the shared folder, root's and read-only, and gives
/// the account its launch agent, started now when the account has a session.
pub fn install_host(
    work: &mut Work,
    state: &SetupState,
    bundle: &str,
    session: bool,
) -> Outcome<String> {
    let version = host_version(work, bundle)
        .ok_or_else(|| format!("{bundle} isn't a Koyo Agent Desktop bundle"))?;
    // The person asks; the bundle must be theirs (a build of theirs) or root's (an installed app).
    let owner = work.run("/usr/bin/stat", &["-f", "%u", bundle]);
    let owner: Option<u32> = owner.out.trim().parse().ok();
    if owner != Some(state.person_uid) && owner != Some(0) {
        return Err(format!("{bundle} belongs to neither the person nor root"));
    }
    let target = format!("{}/{}", paths::HOST_DIR, paths::HOST_APP);
    let library = format!("{}/{}", paths::HOST_DIR, paths::HOST_LIBRARY);
    work.remove("replacing the installed host", &target)?;
    work.must("copying the host", "/usr/bin/ditto", &[bundle, &target])?;
    work.must(
        "copying the host's library",
        "/usr/bin/ditto",
        &[&library_beside(bundle), &library],
    )?;
    for copied in [&target, &library] {
        work.must(
            "owning the host",
            "/usr/sbin/chown",
            &["-R", "root:wheel", copied],
        )?;
        work.must("closing the host", "/bin/chmod", &["-R", "go-w", copied])?;
    }
    work.did(format!(
        "installed the host {version} in {}",
        paths::HOST_DIR
    ));
    let home = paths::ACCOUNT_HOME;
    let account = paths::ACCOUNT;
    work.run_as(
        account,
        "/bin/mkdir",
        &[
            "-p",
            &format!("{home}/Library/LaunchAgents"),
            &format!("{home}/Library/Logs/Koyo Agent Desktop"),
        ],
    );
    let agent = format!("{home}/{}", paths::HOST_AGENT_PLIST);
    work.write_as(
        "writing the host's launch agent",
        account,
        &agent,
        &plist::host_agent(home),
    )?;
    work.did("gave the account the host's launch agent");
    if session {
        restart_host(work, state.uid, &agent);
    }
    Ok(version)
}

/// Loads the host's launch agent in the account's session afresh and starts it.
pub fn restart_host(work: &mut Work, uid: u32, agent: &str) {
    let domain = format!("gui/{uid}");
    work.run(
        "/bin/launchctl",
        &["bootout", &format!("{domain}/{}", paths::HOST_LABEL)],
    );
    work.run("/bin/launchctl", &["bootstrap", &domain, agent]);
    work.run(
        "/bin/launchctl",
        &[
            "kickstart",
            "-k",
            &format!("{domain}/{}", paths::HOST_LABEL),
        ],
    );
    work.did("started the host in the account's session");
}

/// The daemon: the helper's copy, root's, and its plist, loaded.
pub fn install_daemon(work: &mut Work, helper: &str) -> Outcome<()> {
    work.make_dir(
        "making the helper tools folder",
        "/Library/PrivilegedHelperTools",
        0o755,
    )?;
    work.must(
        "copying the helper",
        "/usr/bin/ditto",
        &[helper, paths::HELPER_BIN],
    )?;
    work.chown("owning the helper", "root:wheel", paths::HELPER_BIN)?;
    work.chmod("closing the helper", "755", paths::HELPER_BIN)?;
    work.write(
        "writing the daemon's plist",
        paths::HELPER_PLIST,
        plist::daemon().as_bytes(),
        0o644,
    )?;
    work.chown(
        "owning the daemon's plist",
        "root:wheel",
        paths::HELPER_PLIST,
    )?;
    work.run(
        "/bin/launchctl",
        &["bootout", &format!("system/{}", paths::HELPER_LABEL)],
    );
    work.must(
        "loading the daemon",
        "/bin/launchctl",
        &["bootstrap", "system", paths::HELPER_PLIST],
    )?;
    work.did(format!(
        "installed and loaded the daemon {}",
        paths::HELPER_LABEL
    ));
    Ok(())
}

pub fn load_state(work: &mut Work) -> Option<SetupState> {
    let bytes = work.mac.read(std::path::Path::new(paths::STATE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save_state(work: &mut Work, state: &SetupState) -> Outcome<()> {
    let json = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    work.write("recording the setup", paths::STATE, &json, 0o600)
}
