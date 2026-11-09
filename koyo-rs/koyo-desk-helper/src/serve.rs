//! `koyo-desk-helper serve`: the daemon launchd keeps (`docs/agent-desktop.md` 5.4). At each
//! load it takes Screen Sharing's boot override back before anything else, then puts the
//! firewall rule back; it answers JSON lines on its socket, from the person who set it up
//! alone; and it ends the account's session when the holder's lease drops or the person stops
//! being the console user (6.1).

use std::io::{self, BufRead as _, BufReader, Write as _};
use std::os::fd::AsRawFd as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use koyo_desk_wire::helper::{HelperAnswer, HelperAsk, HelperStatus, SetupState};
use koyo_desk_wire::holder::line;
use koyo_desk_wire::paths;

use crate::mac::{Mac, Real};
use crate::remove::Admin;
use crate::work::{Outcome, Work};
use crate::{account, console, files, firewall, note, remove, session, setup, sharing};

/// Whether a connection from `peer` may ask anything: the person, or root.
pub fn allowed(peer: u32, state: &SetupState) -> bool {
    peer == state.person_uid || peer == 0
}

fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut uid = u32::MAX;
    let mut gid = u32::MAX;
    // SAFETY: a connected socket and two valid out-pointers.
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &raw mut uid, &raw mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

/// What one request comes to; `true` with it when the daemon should go once it has answered.
pub fn answer(mac: &mut dyn Mac, state: &mut SetupState, ask: HelperAsk) -> (HelperAnswer, bool) {
    let mut work = Work::new(mac);
    let result: Outcome<Option<HelperAnswer>> = match ask {
        HelperAsk::Status => Ok(Some(HelperAnswer::Status(status(&mut work, state)))),
        HelperAsk::PrepareLogin => prepare_login(&mut work, state).map(Some),
        HelperAsk::LoginDone => {
            sharing::disable_at_boot(&mut work);
            Ok(None)
        }
        // The lease itself is the connection; `serve_one` keeps it.
        HelperAsk::Hold => Ok(None),
        // Screen Sharing goes off whether or not the session went: it's never left on for want
        // of an ending.
        HelperAsk::EndSession => end_and_off(&mut work, state.uid).map(|()| None),
        HelperAsk::InstallHost { bundle } => {
            let live = session::has_session(&mut work, state.uid);
            files::install_host(&mut work, state, &bundle, live).map(|_| None)
        }
        HelperAsk::RestartHost => {
            let agent = format!("{}/{}", paths::ACCOUNT_HOME, paths::HOST_AGENT_PLIST);
            files::restart_host(&mut work, state.uid, &agent);
            Ok(None)
        }
        HelperAsk::Remove { admin, password } => {
            let admin = Admin {
                name: &admin,
                password: &password,
            };
            let removed = remove::run(&mut work, state, Some(&admin));
            let gone = removed.is_ok();
            return (finish(work, removed.map(|()| None)), gone);
        }
    };
    (finish(work, result), false)
}

fn finish(work: Work, result: Outcome<Option<HelperAnswer>>) -> HelperAnswer {
    match result {
        Ok(Some(answer)) => answer,
        Ok(None) => HelperAnswer::Done { steps: work.steps },
        Err(message) => HelperAnswer::Failed {
            message,
            steps: work.steps,
        },
    }
}

/// Ends the account's session and turns Screen Sharing off, whether or not the session went:
/// it's never left on for want of an ending.
fn end_and_off(work: &mut Work, uid: u32) -> Outcome<()> {
    let ended = session::end(work, uid);
    sharing::off(work);
    ended
}

pub fn status(work: &mut Work, state: &SetupState) -> HelperStatus {
    let macos = setup::macos(work).0;
    HelperStatus {
        helper_version: env!("CARGO_PKG_VERSION").into(),
        refusal: paths::macos_refusal(&macos, state.accepted_risk),
        macos,
        account: paths::ACCOUNT.into(),
        uid: state.uid,
        screen_sharing_on: sharing::is_on(work),
        anchor_loaded: firewall::loaded(work),
        session: session::has_session(work, state.uid),
        host_version: files::installed_host(work),
    }
}

/// Not yet, while the account's last session is too near (6.1); else the trim written again, the
/// rule in place, Screen Sharing on, and the account's name and password for the holder.
fn prepare_login(work: &mut Work, state: &mut SetupState) -> Outcome<HelperAnswer> {
    if let Some(refused) = paths::macos_refusal(&setup::macos(work).0, state.accepted_risk) {
        return Err(refused);
    }
    if let Some((wait, why)) = session::not_yet(work, state.uid) {
        return Ok(HelperAnswer::NotYet {
            wait_ms: u64::try_from(wait.as_millis()).unwrap_or(u64::MAX),
            why,
        });
    }
    account::trim(work, state.uid);
    if !firewall::loaded(work) {
        state.pf_token = firewall::apply(work)?;
        files::save_state(work, state)?;
    }
    sharing::on(work)?;
    Ok(HelperAnswer::Login {
        account: paths::ACCOUNT.into(),
        password: state.password.clone(),
    })
}

/// At load, in this order: Screen Sharing's boot override taken back (launchd keeps it across a
/// restart; pf doesn't keep the rule), the service stopped unless a session holds it (a daemon
/// restarted under a live session mustn't curtain it), then the rule back with a fresh pf
/// reference.
fn start(state: &mut SetupState) -> Outcome<()> {
    let mut mac = Real;
    let mut work = Work::new(&mut mac);
    sharing::disable_at_boot(&mut work);
    if !session::has_session(&mut work, state.uid) {
        sharing::off(&mut work);
    }
    if let Some(token) = state.pf_token.take() {
        firewall::release(&mut work, Some(&token));
    }
    state.pf_token = firewall::apply(&mut work)?;
    files::save_state(&mut work, state)?;
    for step in work.steps {
        note(&step);
    }
    Ok(())
}

fn serve_one(stream: UnixStream, shared: &Arc<Mutex<SetupState>>) -> io::Result<()> {
    let peer = peer_uid(&stream)?;
    let mut writer = stream.try_clone()?;
    {
        let state = shared.lock().unwrap_or_else(PoisonError::into_inner);
        if !allowed(peer, &state) {
            note(&format!("refused a connection from uid {peer}"));
            return Ok(());
        }
    }
    let mut leased = false;
    let lines = BufReader::new(stream).lines();
    for text in lines {
        let Ok(text) = text else { break };
        let (reply, gone) = match serde_json::from_str::<HelperAsk>(&text) {
            Ok(ask) => {
                leased |= ask == HelperAsk::Hold;
                // `remove`'s password is never logged; what was asked is.
                let what = text.split(',').next().unwrap_or_default().to_owned();
                note(&format!("uid {peer} asked {what}"));
                let mut state = shared.lock().unwrap_or_else(PoisonError::into_inner);
                answer(&mut Real, &mut state, ask)
            }
            Err(error) => (
                HelperAnswer::Failed {
                    message: format!("not a request: {error}"),
                    steps: Vec::new(),
                },
                false,
            ),
        };
        writer.write_all(line(&reply).map_err(io::Error::other)?.as_bytes())?;
        writer.flush()?;
        if gone {
            note("removed; the daemon goes");
            let _ = std::fs::remove_file(paths::HELPER_SOCKET);
            let _ = std::process::Command::new("/bin/launchctl")
                .args(["bootout", &format!("system/{}", paths::HELPER_LABEL)])
                .status();
            #[expect(
                clippy::disallowed_methods,
                reason = "the daemon has removed itself and launchd is booting it out; nothing is left to flush"
            )]
            std::process::exit(0);
        }
    }
    if leased {
        note("the holder's lease ended: ending the account's session");
        end_session(shared, "the holder's lease ended");
    }
    Ok(())
}

/// Ends the account's session, if it has one, and turns Screen Sharing off.
fn end_session(shared: &Arc<Mutex<SetupState>>, why: &str) {
    let state = shared.lock().unwrap_or_else(PoisonError::into_inner);
    let mut mac = Real;
    let mut work = Work::new(&mut mac);
    if !session::has_session(&mut work, state.uid) && !sharing::is_on(&mut work) {
        return;
    }
    match end_and_off(&mut work, state.uid) {
        Ok(()) => note(&format!("ended the account's session: {why}")),
        Err(error) => note(&format!(
            "couldn't end the account's session ({why}): {error}"
        )),
    }
}

/// How often the console is looked at.
const CONSOLE_EVERY: Duration = Duration::from_secs(2);

/// Ends the session once the person has stopped being the console user two looks running.
fn watch_console(shared: &Arc<Mutex<SetupState>>) {
    let person = shared
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .person_uid;
    let mut away = 0;
    loop {
        let (_wake, waiting) = std::sync::mpsc::channel::<()>();
        let _ = waiting.recv_timeout(CONSOLE_EVERY);
        let sessions = Real.console_sessions();
        // Nothing listed is nothing known: only a list without the person on the console counts.
        away = if sessions.is_empty() || console::on_console(&sessions, person) {
            0
        } else {
            away + 1
        };
        if away == 2 {
            end_session(shared, "the person isn't at the console");
        }
    }
}

pub fn main() -> Outcome<()> {
    let mut state = {
        let mut mac = Real;
        files::load_state(&mut Work::new(&mut mac)).ok_or("not set up: no state.json")?
    };
    start(&mut state)?;
    let _ = std::fs::remove_file(paths::HELPER_SOCKET);
    let listener = UnixListener::bind(paths::HELPER_SOCKET).map_err(|error| error.to_string())?;
    {
        use std::os::unix::fs::PermissionsExt as _;
        // Anyone may connect; only the person is answered (`allowed`).
        std::fs::set_permissions(paths::HELPER_SOCKET, std::fs::Permissions::from_mode(0o666))
            .map_err(|error| error.to_string())?;
    }
    note(&format!(
        "serving uid {} on {}",
        state.person_uid,
        paths::HELPER_SOCKET
    ));
    let shared = Arc::new(Mutex::new(state));
    {
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || watch_console(&shared));
    }
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            if let Err(error) = serve_one(stream, &shared) {
                note(&format!("a connection ended: {error}"));
            }
        });
    }
    Ok(())
}
