//! `koyo-desk-helper setup`, run as root once under the person's administrator approval
//! (`docs/agent-desktop.md` 5.1). Idempotent: run again, it brings back what's missing.

use koyo_desk_wire::helper::SetupState;
use koyo_desk_wire::paths;

use crate::work::{Outcome, Work};
use crate::{account, files, firewall, session, sharing};

pub struct Setup {
    /// The person setting up, the one user the daemon will answer.
    pub person_uid: u32,
    /// The helper's own executable, copied for the daemon.
    pub helper: String,
    /// A host bundle to install now.
    pub host: Option<String>,
    /// Go on below macOS 26.6.1, whose `screensharingd` has a pre-authentication bypass.
    /// Development only.
    pub accept_risk: bool,
}

/// The running macOS: its version ("26.4") and build ("25E246").
pub fn macos(work: &mut Work) -> (String, String) {
    let version = work.run("/usr/bin/sw_vers", &["-productVersion"]);
    let build = work.run("/usr/bin/sw_vers", &["-buildVersion"]);
    (version.out.trim().to_owned(), build.out.trim().to_owned())
}

/// The checks that refuse a setup before anything is touched.
fn check(
    work: &mut Work,
    setup: &Setup,
    macos: &str,
    existing: Option<&SetupState>,
) -> Outcome<()> {
    if setup.person_uid == 0 {
        return Err("set up from the person's own account, not root's".into());
    }
    if let Some(refused) = paths::macos_refusal(macos, setup.accept_risk) {
        return Err(refused);
    }
    if !firewall::conf_loads_anchors(work) {
        return Err(
            "/etc/pf.conf doesn't load com.apple anchors, where the firewall rule goes".into(),
        );
    }
    match existing {
        Some(state) if state.person_uid != setup.person_uid => {
            Err("Koyo's agent desktop is set up for another user of this Mac".into())
        }
        None if account::uid(work, paths::ACCOUNT).is_some() => Err(format!(
            "an account named {} is already on this Mac, and Koyo didn't make it",
            paths::ACCOUNT
        )),
        None if sharing::is_on(work) => Err(
            "Screen Sharing is on. Agent desktops keep it to this Mac alone, which would cut off \
             sharing from other computers: turn it off in System Settings → General → Sharing \
             first"
                .into(),
        ),
        _ => Ok(()),
    }
}

/// The account's name, as `chmod +a` wants it.
fn user_name(work: &mut Work, uid: u32) -> Outcome<String> {
    let ran = work.must(
        "reading the person's name",
        "/usr/bin/id",
        &["-un", &uid.to_string()],
    )?;
    Ok(ran.out.trim().to_owned())
}

pub fn run(work: &mut Work, setup: &Setup) -> Outcome<()> {
    let (macos, build) = macos(work);
    let existing = files::load_state(work);
    check(work, setup, &macos, existing.as_ref())?;
    let person = user_name(work, setup.person_uid)?;
    work.make_dir("making the shared folder", paths::SHARED, 0o755)?;
    if let Some(token) = existing
        .as_ref()
        .and_then(|state| state.pf_token.as_deref())
    {
        firewall::release(work, Some(token));
    }
    let pf_token = firewall::apply(work)?;
    let gid = account::ensure_group(work)?;
    let made_helper_tools = existing
        .as_ref()
        .is_some_and(|state| state.made_helper_tools)
        || !work.exists("/Library/PrivilegedHelperTools");
    // The account Koyo made before, or a new one.
    let kept = existing.filter(|state| account::uid(work, paths::ACCOUNT) == Some(state.uid));
    let fresh = kept.is_none();
    let (uid, password) = if let Some(state) = kept {
        (state.uid, state.password)
    } else {
        let password = account::new_password();
        (account::add(work, &password)?, password)
    };
    let state = SetupState {
        person_uid: setup.person_uid,
        uid,
        gid,
        password,
        pf_token,
        macos,
        accepted_risk: setup.accept_risk,
        made_helper_tools,
    };
    if fresh {
        // Recorded the moment it exists, so a setup stopped from here on finds the account its
        // own when it runs again.
        files::save_state(work, &state)?;
        account::check_password(work, &state.password)?;
    }
    account::close_off(work, gid)?;
    account::preferences(work, &state.macos, &build);
    account::trim(work, uid);
    sharing::allow(work, paths::ACCOUNT)?;
    files::shared_folder(work, setup.person_uid, &person, gid)?;
    files::save_state(work, &state)?;
    if let Some(host) = &setup.host {
        let session = session::has_session(work, uid);
        files::install_host(work, &state, host, session)?;
    }
    files::install_daemon(work, &setup.helper)?;
    // Off until a login needs it (5.4).
    if !session::has_session(work, uid) {
        sharing::off(work);
    }
    work.did("set up");
    Ok(())
}
