//! The app's socket reaches the host as an inherited file descriptor, so it
//! never has a path: no folder to make, no `sun_path` limit to fit, nothing
//! left behind after a crash.
use std::io;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};

/// The descriptor the host finds its end of the socket on: its standard input.
pub const HOST_FD: RawFd = 0;

/// Starts the program `command` runs with `stream` as its standard input,
/// [`HOST_FD`], and its alone: the app's own copy is close-on-exec and goes
/// with `command` here, so no other program the app starts holds the host's
/// end open, and the app's end closes when the host does. The browser's host
/// and a device's host are both started this way.
///
/// As its standard input because that is a descriptor the standard library
/// puts in place as it spawns a program. Any other it can only hand over
/// with a step run in the child before it becomes the program, and for that
/// it forks the app, which holds the browser engine and can't be forked
/// (`docs/native-pages.md` 11).
///
/// # Errors
/// When the program can't be started.
pub fn spawn(mut command: Command, stream: UnixStream) -> io::Result<Child> {
    command.stdin(Stdio::from(OwnedFd::from(stream)));
    command.spawn()
}

/// The host's end of the app's socket, named by `value` from
/// [`ENV_SOCKET_FD`](crate::ENV_SOCKET_FD). It is moved off the host's
/// standard input, close-on-exec, and the null device put there in its
/// place: the engine starts its helper processes with its own standard
/// input, whatever that is flagged, and they don't get the socket.
///
/// # Errors
/// When `value` is not a descriptor number or names no socket.
pub fn take(value: &str) -> io::Result<UnixStream> {
    let fd: RawFd = value.parse().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a descriptor number: {error}"),
        )
    })?;
    // SAFETY: `fstat` only inspects the descriptor; the calls after it act on
    // it only once it is a socket this process was given, and the copy is
    // owned from here on.
    unsafe {
        let mut stat: libc::stat = std::mem::zeroed();
        if libc::fstat(fd, &raw mut stat) < 0 {
            return Err(io::Error::last_os_error());
        }
        if stat.st_mode & libc::S_IFMT != libc::S_IFSOCK {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a socket"));
        }
        // Past the standard three, wherever it came in.
        let moved = libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3);
        if moved < 0 {
            return Err(io::Error::last_os_error());
        }
        let socket = UnixStream::from_raw_fd(moved);
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC);
        if null < 0 || libc::dup2(null, fd) < 0 {
            return Err(io::Error::last_os_error());
        }
        libc::close(null);
        Ok(socket)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;

    thread_local! {
        /// How often this thread has forked the process.
        static FORKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    extern "C" fn forking() {
        FORKS.set(FORKS.get() + 1);
    }

    #[test]
    fn a_passed_socket_reaches_the_child_and_only_its_end_closes_with_it() {
        // SAFETY: the handler only counts, on the thread that forks.
        unsafe { libc::pthread_atfork(Some(forking), None, None) };
        let (mut ours, theirs) = UnixStream::pair().expect("pair");
        let mut command = Command::new("/bin/sh");
        // The child writes through its standard input, the socket, and exits.
        command.args(["-c", "printf hello >&0"]);
        let mut child = spawn(command, theirs).expect("spawn");
        let mut got = String::new();
        ours.read_to_string(&mut got)
            .expect("read until the child's end closes");
        assert_eq!(got, "hello");
        assert!(child.wait().expect("wait").success());
        // With the child gone, nothing else holds its end open.
        assert!(ours.write_all(b"x").is_err());
        assert_eq!(FORKS.get(), 0, "the app was forked to start it");
    }

    #[test]
    fn a_taken_socket_leaves_its_place_to_the_null_device() {
        let (mut ours, theirs) = UnixStream::pair().expect("pair");
        let came_in = theirs.as_raw_fd();
        // As the host finds it: on a descriptor it was started with.
        let mut taken = take(&came_in.to_string()).expect("take");
        std::mem::forget(theirs);
        assert!(taken.as_raw_fd() > 2 && taken.as_raw_fd() != came_in);
        taken.write_all(b"hi").expect("write");
        let mut got = [0; 2];
        ours.read_exact(&mut got).expect("read");
        assert_eq!(&got, b"hi");
        // What it came in on is no socket now, and closing it closes nothing of the link's.
        // SAFETY: the descriptor is this test's own, the null device since `take`.
        unsafe {
            let mut stat: libc::stat = std::mem::zeroed();
            assert_eq!(libc::fstat(came_in, &raw mut stat), 0);
            assert_eq!(stat.st_mode & libc::S_IFMT, libc::S_IFCHR);
            libc::close(came_in);
        }
        taken.write_all(b"ok").expect("still linked");
    }

    #[test]
    fn only_a_socket_descriptor_is_taken() {
        assert!(take("x").is_err());
        let file = std::fs::File::open("/dev/null").expect("open");
        assert!(take(&file.as_raw_fd().to_string()).is_err());
    }
}
