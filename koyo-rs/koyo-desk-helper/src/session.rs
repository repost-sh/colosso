//! The account's login session: whether it has one, and ending it. Only the account's own
//! processes are ever touched (by uid, never by name), and an account is never deleted while
//! one of them is left (`docs/agent-desktop.md` 5.5).
//!
//! A login session is the account's GUI domain (`gui/<uid>`), which exists only while it's
//! logged in. Its per-user background agents (`cfprefsd`, `distnoted`, `lsd`, `trustd`…) are
//! no session: anything run as the account starts them, launchd keeps them in its `user/<uid>`
//! domain and starts them again when killed, so only removal, which takes that domain down too,
//! waits for every process to go. Every end is noted in [`paths::LAST_END`], so the next login
//! waits out [`paths::AFTER_AN_END`] whoever asks for it (6.1).

use std::time::Duration;

use koyo_desk_wire::paths;

use crate::console;
use crate::work::{Outcome, Work};

/// How long a session's processes get to go after a logout, then after a kill.
const DRAIN: Duration = Duration::from_secs(10);
/// How long the session's own logout gets before it's booted out: short, as a restart may be
/// waiting on it (6.1).
const LOGOUT: Duration = Duration::from_secs(3);
/// How often a drain is looked at.
const DRAIN_STEP: Duration = Duration::from_millis(500);
/// How often the logout is looked at: it's on the way of a quit or a restart.
const LOGOUT_STEP: Duration = Duration::from_millis(100);

/// The processes whose real user is `uid`.
pub fn pids(work: &mut Work, uid: u32) -> Vec<i32> {
    let ran = work.run("/usr/bin/pgrep", &["-U", &uid.to_string()]);
    ran.out
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// Whether the account is logged in: its GUI domain exists.
pub fn has_session(work: &mut Work, uid: u32) -> bool {
    work.run("/bin/launchctl", &["print", &format!("gui/{uid}")])
        .ok()
}

/// Whether `done` holds within `limit`, looked at every `step`.
fn within(
    work: &mut Work,
    limit: Duration,
    step: Duration,
    done: &mut impl FnMut(&mut Work) -> bool,
) -> bool {
    let mut waited = Duration::ZERO;
    loop {
        if done(work) {
            return true;
        }
        if waited >= limit {
            return false;
        }
        work.mac.pause(step);
        waited += step;
    }
}

/// Notes that a session ended now, for [`paths::AFTER_AN_END`].
fn noted(work: &mut Work) {
    if let Err(error) = work.write("noting the session's end", paths::LAST_END, b"", 0o644) {
        work.did(error);
    }
}

/// Logs the account out: its own logout first, else its GUI domain booted out, then killed if it
/// stays.
///
/// # Errors
/// When the session is still there after both waits.
pub fn end(work: &mut Work, uid: u32) -> Outcome<()> {
    if !has_session(work, uid) {
        return Ok(());
    }
    let mut gone = |work: &mut Work| !has_session(work, uid);
    // Its own logout first, as the account inside its session: loginwindow takes the session
    // off the system's lists (utmpx, the console user record) as a logout does, which a kill
    // leaves behind. Only ever for a session the kernel lists off the console, so it can never
    // be the person's.
    let sessions = work.mac.console_sessions();
    if console::only_off_console(&sessions, uid) {
        let as_account = format!("#{uid}");
        work.run(
            "/bin/launchctl",
            &[
                "asuser",
                &uid.to_string(),
                "/usr/bin/sudo",
                "-u",
                &as_account,
                "/usr/bin/osascript",
                "-e",
                "tell application \"loginwindow\" to «event aevtrlgo»",
            ],
        );
        if within(work, LOGOUT, LOGOUT_STEP, &mut gone) {
            work.did(format!("logged the account out (uid {uid})"));
            noted(work);
            return Ok(());
        }
    }
    work.run("/bin/launchctl", &["bootout", &format!("gui/{uid}")]);
    if !within(work, DRAIN, DRAIN_STEP, &mut gone) {
        work.run("/usr/bin/pkill", &["-9", "-U", &uid.to_string()]);
        if !within(work, DRAIN, DRAIN_STEP, &mut gone) {
            noted(work);
            return Err(format!(
                "the account's session (uid {uid}) is still there after a logout and a kill"
            ));
        }
    }
    work.did(format!("ended the account's session (uid {uid})"));
    noted(work);
    Ok(())
}

/// Ends the session and every process of the account, its background domain included: what
/// removal waits for before it deletes the account.
///
/// # Errors
/// Names the processes still there after both waits.
pub fn end_all(work: &mut Work, uid: u32) -> Outcome<()> {
    end(work, uid)?;
    if pids(work, uid).is_empty() {
        return Ok(());
    }
    work.run("/bin/launchctl", &["bootout", &format!("user/{uid}")]);
    work.run("/usr/bin/pkill", &["-9", "-U", &uid.to_string()]);
    let mut drained = |work: &mut Work| pids(work, uid).is_empty();
    if !within(work, DRAIN, DRAIN_STEP, &mut drained) {
        let left = pids(work, uid);
        return Err(format!(
            "processes of the account (uid {uid}) are still there after a logout and a kill: {left:?}"
        ));
    }
    work.did(format!("stopped every process of the account (uid {uid})"));
    Ok(())
}

/// Why a login has to wait, and for how long: the kernel still lists the account's last
/// session, or that ended less than [`paths::AFTER_AN_END`] ago.
pub fn not_yet(work: &mut Work, uid: u32) -> Option<(Duration, String)> {
    if console::listed(&work.mac.console_sessions(), uid) {
        return Some((
            Duration::from_secs(1),
            "the kernel still lists the account's last session".into(),
        ));
    }
    let since = work.mac.age(std::path::Path::new(paths::LAST_END))?;
    let wait = paths::AFTER_AN_END.saturating_sub(since);
    (!wait.is_zero()).then(|| {
        (
            wait,
            format!("the account's last session ended {} s ago", since.as_secs()),
        )
    })
}
