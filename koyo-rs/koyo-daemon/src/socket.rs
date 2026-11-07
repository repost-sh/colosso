//! Reaching a socket whatever folder it's in. A socket's address holds at most 103 bytes of path
//! on macOS, and a daemon's folder can be deeper than that leaves room for: a checkout's own
//! scratch folder, or a long `KOYO_DATA_DIR`. The way there from the working directory is often
//! short when the absolute path isn't, and the system takes it the same: a daemon runs in its
//! folder (`start`), so it binds its socket by name alone, and a test runs in its package, inside
//! the checkout, so the way to the checkout's scratch folder is short.

use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Component, Path, PathBuf};

/// The most bytes of path a socket address holds, its terminating zero left out.
pub const SOCKET_PATH_MAX: usize = 103;

/// How to reach the socket at `path`: `path` itself when an address holds it, else the way there
/// from the working directory when an address holds that, else `path`, which the system refuses.
pub fn socket_address(path: &Path) -> PathBuf {
    if path.as_os_str().len() <= SOCKET_PATH_MAX {
        return path.to_path_buf();
    }
    let from_here = std::env::current_dir()
        .ok()
        .and_then(|here| way(&here, &physical(path)));
    match from_here {
        Some(way) if way.as_os_str().len() <= SOCKET_PATH_MAX => way,
        _ => path.to_path_buf(),
    }
}

/// Connects to the socket at `path`, however deep its folder.
pub fn connect(path: &Path) -> io::Result<UnixStream> {
    UnixStream::connect(socket_address(path))
}

/// Listens at `path`, however deep its folder.
pub fn bind(path: &Path) -> io::Result<UnixListener> {
    UnixListener::bind(socket_address(path))
}

/// `path` with its longest ancestor that exists resolved, links and all, as the working
/// directory always is; what's left of it, not made yet, is added as it is.
fn physical(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut at = path;
    loop {
        if let Ok(real) = at.canonicalize() {
            return rest.iter().rev().fold(real, |whole, part| whole.join(part));
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                at = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// The way from the folder `from` to `to`, both absolute and with no `..` in them: up to where
/// they part, then down.
fn way(from: &Path, to: &Path) -> Option<PathBuf> {
    let plain = |path: &Path| {
        path.is_absolute()
            && path
                .components()
                .all(|part| !matches!(part, Component::ParentDir | Component::CurDir))
    };
    if !plain(from) || !plain(to) {
        return None;
    }
    let (from, to): (Vec<_>, Vec<_>) = (from.components().collect(), to.components().collect());
    let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut way = PathBuf::new();
    for _ in shared..from.len() {
        way.push("..");
    }
    for part in &to[shared..] {
        way.push(part);
    }
    Some(way)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_way_goes_up_to_where_the_paths_part_then_down() {
        let up_and_down = way(Path::new("/a/b/c"), Path::new("/a/d/e.sock"));
        assert_eq!(up_and_down, Some(PathBuf::from("../../d/e.sock")));
        let inside = way(Path::new("/a/b"), Path::new("/a/b/c/d.sock"));
        assert_eq!(inside, Some(PathBuf::from("c/d.sock")));
        assert_eq!(way(Path::new("a/b"), Path::new("/a/d.sock")), None);
        assert_eq!(way(Path::new("/a/../b"), Path::new("/a/d.sock")), None);
    }

    #[test]
    fn a_short_path_is_its_own_address() {
        let short = Path::new("/data/agents/host.sock");
        assert_eq!(socket_address(short), short);
    }
}
