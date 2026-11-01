//! Who holds the profiles. Chromium lets one browser process at a time use a profile root,
//! and marks it with a `SingletonLock` link naming the holder as `<hostname>-<pid>`. A new
//! host finding a holder alive asks it to take over and waits up to 20 seconds for an
//! answer, which a holder that is stuck or shutting down never gives: the new host then
//! misses the app's hello. So before a host starts, a host left without its app is
//! stopped, and one another app still runs is reported at once.

use std::io;
use std::path::Path;

/// The link Chromium keeps in a profile root while a browser process uses it.
const LOCK: &str = "SingletonLock";

/// Who holds a profile root.
#[derive(Debug, PartialEq, Eq)]
pub enum Holder {
    /// No live process on this machine holds it: Chromium takes over a stale lock itself.
    Free,
    /// A host that serves no one: its app is gone, or is this process, which only starts a
    /// host once the last is done with.
    Orphan(i32),
    /// A host another running app started, by that app's process id.
    Other { host: i32, app: i32 },
}

/// The pid in a lock's target `<hostname>-<pid>`, when it is this machine's.
fn locked_by(target: &str, hostname: &str) -> Option<i32> {
    let (host, pid) = target.rsplit_once('-')?;
    (host == hostname).then(|| pid.parse().ok()).flatten()
}

/// Whether a holder whose parent is `parent` serves no app: launchd adopts the children
/// of a process that died, and this process is starting the host that replaces it.
fn orphaned(parent: i32) -> bool {
    parent == 1 || u32::try_from(parent).is_ok_and(|parent| parent == std::process::id())
}

/// Who holds `root`, when its holder is a host named `host_name`. A live process of
/// another name holds a reused pid, not the lock.
#[cfg(target_os = "macos")]
#[must_use]
pub fn holder(root: &Path, host_name: &str) -> Holder {
    let Ok(target) = std::fs::read_link(root.join(LOCK)) else {
        return Holder::Free;
    };
    let Some(pid) = hostname().and_then(|hostname| locked_by(&target.to_string_lossy(), &hostname))
    else {
        return Holder::Free;
    };
    let Some((name, parent)) = process(pid) else {
        return Holder::Free;
    };
    if name != host_name {
        Holder::Free
    } else if orphaned(parent) {
        Holder::Orphan(pid)
    } else {
        Holder::Other {
            host: pid,
            app: parent,
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn holder(_: &Path, _: &str) -> Holder {
    Holder::Free
}

/// Makes `root` free for a host named `host_name` to start on: an orphaned host is killed,
/// as it would not answer the new one in time, and waited for.
///
/// # Errors
/// When another running app's host holds it, or an orphan outlives being killed.
pub fn free(root: &Path, host_name: &str) -> io::Result<()> {
    match holder(root, host_name) {
        Holder::Free => Ok(()),
        Holder::Orphan(pid) => stop(pid),
        Holder::Other { host, app } => Err(io::Error::new(
            io::ErrorKind::ResourceBusy,
            format!(
                "another Colosso (process {app}) is running the browser on these profiles, \
                 as process {host}"
            ),
        )),
    }
}

/// Kills `pid` and waits up to two seconds for it to exit, which releases the lock.
#[cfg(target_os = "macos")]
fn stop(pid: i32) -> io::Result<()> {
    let would_not = || {
        io::Error::other(format!(
            "an earlier browser host (process {pid}) would not stop"
        ))
    };
    // SAFETY: plain syscalls; a closed or failed queue only makes the wait fail.
    unsafe {
        let queue = libc::kqueue();
        if queue < 0 {
            return Err(io::Error::last_os_error());
        }
        let watch = libc::kevent {
            ident: usize::try_from(pid).map_err(io::Error::other)?,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ONESHOT,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // Watched before it's killed, so its exit can't be missed.
        let watched = libc::kevent(
            queue,
            &raw const watch,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        );
        if watched < 0 {
            libc::close(queue);
            // Gone already.
            return Ok(());
        }
        libc::kill(pid, libc::SIGKILL);
        let mut exited = watch;
        let patience = libc::timespec {
            tv_sec: 2,
            tv_nsec: 0,
        };
        let seen = libc::kevent(
            queue,
            std::ptr::null(),
            0,
            &raw mut exited,
            1,
            &raw const patience,
        );
        libc::close(queue);
        if seen == 1 { Ok(()) } else { Err(would_not()) }
    }
}

#[cfg(not(target_os = "macos"))]
fn stop(_: i32) -> io::Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn hostname() -> Option<String> {
    let mut name = [0u8; 256];
    // SAFETY: the buffer and its length match.
    let ok = unsafe { libc::gethostname(name.as_mut_ptr().cast(), name.len()) } == 0;
    let end = name.iter().position(|&b| b == 0)?;
    ok.then(|| String::from_utf8_lossy(&name[..end]).into_owned())
}

/// A live process's executable file name and its parent's pid.
#[cfg(target_os = "macos")]
fn process(pid: i32) -> Option<(String, i32)> {
    let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer and its length match.
    let len = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
    let len = usize::try_from(len).ok().filter(|&len| len > 0)?;
    let path = std::path::PathBuf::from(String::from_utf8_lossy(&path[..len]).into_owned());
    let name = path.file_name()?.to_string_lossy().into_owned();
    // SAFETY: an all-zero `proc_bsdinfo` is valid, and the size passed is its own.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = i32::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: `info` is as large as the size passed.
    let read =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    (read == size).then(|| (name, i32::try_from(info.pbi_ppid).unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lock_names_its_holder_on_this_machine_only() {
        assert_eq!(locked_by("mac.local-4242", "mac.local"), Some(4242));
        assert_eq!(locked_by("my-mac.local-7", "my-mac.local"), Some(7));
        assert_eq!(locked_by("other.local-4242", "mac.local"), None);
        assert_eq!(locked_by("mac.local-x", "mac.local"), None);
        assert_eq!(locked_by("nonsense", "mac.local"), None);
    }

    #[test]
    fn a_host_launchd_adopted_has_lost_its_app() {
        assert!(orphaned(1));
        assert!(orphaned(i32::try_from(std::process::id()).expect("pid")));
        assert!(!orphaned(i32::MAX));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_root_without_a_lock_or_with_a_dead_holder_is_free() {
        // Scratch inside the checkout, as the smoke tests keep theirs.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../.superpowers/t/ipc-profiles-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch");
        assert_eq!(holder(&root, "Colosso Browser Host"), Holder::Free);
        let hostname = hostname().expect("hostname");
        // No process has the largest pid, and this test's own process isn't a host.
        std::os::unix::fs::symlink(format!("{hostname}-{}", i32::MAX), root.join(LOCK))
            .expect("lock");
        assert_eq!(holder(&root, "Colosso Browser Host"), Holder::Free);
        std::fs::remove_file(root.join(LOCK)).expect("unlock");
        std::os::unix::fs::symlink(
            format!("{hostname}-{}", std::process::id()),
            root.join(LOCK),
        )
        .expect("lock");
        assert_eq!(holder(&root, "Colosso Browser Host"), Holder::Free);
        let name = process(i32::try_from(std::process::id()).expect("pid"))
            .expect("self")
            .0;
        assert!(matches!(holder(&root, &name), Holder::Other { .. }));
        let _ = std::fs::remove_dir_all(&root);
    }
}
