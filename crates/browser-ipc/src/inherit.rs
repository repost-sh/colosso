//! The app's socket reaches the host as an inherited file descriptor, so it
//! never has a path: no folder to make, no `sun_path` limit to fit, nothing
//! left behind after a crash.
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// The descriptor the host finds its end of the socket on.
pub const HOST_FD: RawFd = 3;

/// Hands `stream` to the program `command` starts, as [`HOST_FD`], and to it
/// alone: the copy in this process stays close-on-exec, so other programs
/// the app starts never hold the host's end open.
pub(crate) fn pass(command: &mut Command, stream: &UnixStream) {
    let fd = stream.as_raw_fd();
    // SAFETY: the closure only calls `dup2` and `fcntl`, which are
    // async-signal-safe, and touches no memory shared with the parent.
    unsafe {
        command.pre_exec(move || {
            if fd == HOST_FD {
                // `dup2` onto itself keeps close-on-exec set; clear it.
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(io::Error::last_os_error());
                }
            } else if libc::dup2(fd, HOST_FD) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// The host's end of the app's socket, named by `value` from
/// [`ENV_SOCKET_FD`](crate::ENV_SOCKET_FD). It is made close-on-exec, so the
/// engine's own helper processes do not inherit it.
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
    // SAFETY: `fstat` and `fcntl` only inspect and flag the descriptor; it is
    // owned from here on only when it is a socket this process was given.
    unsafe {
        let mut stat: libc::stat = std::mem::zeroed();
        if libc::fstat(fd, &raw mut stat) < 0 {
            return Err(io::Error::last_os_error());
        }
        if stat.st_mode & libc::S_IFMT != libc::S_IFSOCK {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a socket"));
        }
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(UnixStream::from_raw_fd(fd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn a_passed_socket_reaches_the_child_and_only_its_end_closes_with_it() {
        let (mut ours, theirs) = UnixStream::pair().expect("pair");
        let mut command = Command::new("/bin/sh");
        // The child writes through descriptor 3 and exits.
        command.args(["-c", "printf hello >&3"]);
        pass(&mut command, &theirs);
        let mut child = command.spawn().expect("spawn");
        drop(theirs);
        let mut got = String::new();
        ours.read_to_string(&mut got)
            .expect("read until the child's end closes");
        assert_eq!(got, "hello");
        assert!(child.wait().expect("wait").success());
        // With the child gone, nothing else holds its end open.
        assert!(ours.write_all(b"x").is_err());
    }

    #[test]
    fn only_a_socket_descriptor_is_taken() {
        assert!(take("x").is_err());
        let file = std::fs::File::open("/dev/null").expect("open");
        assert!(take(&file.as_raw_fd().to_string()).is_err());
    }
}
