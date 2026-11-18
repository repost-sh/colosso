//! A host's link, as the app keeps it (`docs/devices.md` 3, `docs/agent-desktop.md` 8): once
//! the host has said hello (`koyo_frame::hello`), what the app tells it is written on a thread
//! of the link's own, what it says is read on another, and the pictures it sends by mach port
//! are received on a third, all of it to one sink, `Closed` last. A device's host and the agent
//! desktop's speak messages of their own in the same frames.

use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};

use futures::channel::mpsc;
use koyo_browser_ipc::mach;
use koyo_desk_wire::{read_frame, write_frame};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// What a host says, in the order it came on each channel.
pub enum Incoming<Said> {
    Said(Said),
    /// Pictures by mach port: a device's ring, or a window's frames on the agent desktop
    /// (`pane` is then the window's number, each port a frame's surface).
    Ring(mach::Ring),
    /// The link closed: the host went.
    Closed,
}

/// Where a host's link puts what the host says.
pub type Sink<Said> = mpsc::UnboundedSender<Incoming<Said>>;

/// A host that said hello.
pub struct Running<Tell> {
    /// The host's process, as the kernel named it at the link's other end once it had said
    /// hello: none for a host that had gone by then.
    pub pid: Option<i32>,
    /// What the app tells it, written on a thread of the link's own.
    pub tell: Sender<Tell>,
    /// Set once this is dropped or the link closes: the host's pictures are no longer received.
    stop: Arc<AtomicBool>,
}

impl<Tell> Drop for Running<Tell> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl<Tell> Running<Tell> {
    /// A host that isn't there, for the stand-in tests and the off-screen pictures: what is
    /// told to it comes out of the receiver.
    #[cfg(any(test, feature = "visual-check"))]
    pub fn stand_in(tell: Sender<Tell>) -> Self {
        Running {
            pid: None,
            tell,
            stop: Arc::default(),
        }
    }
}

/// The link to the host on `stream`, once it has said hello: its threads named `name-…`, its
/// pictures received on `pictures`, from the host's process alone, as the kernel names it at
/// the other end of `stream` (`mach::Process`). A host that has gone already, so can't be
/// named, sends no picture again: what it said before it went is read, then `Closed`.
///
/// # Errors
/// When the socket can't be shared among the link's threads or a thread can't be started. The
/// threads started by then let the socket go, so the host hears it end: the writer with
/// `tell`, the pictures with the `Running` a failed reader drops, and the reader, which only
/// the socket's end stops, starts last.
pub fn run<Tell, Said>(
    stream: UnixStream,
    pictures: mach::Receiver,
    name: &str,
    sink: Sink<Said>,
) -> io::Result<Running<Tell>>
where
    Tell: Serialize + Send + 'static,
    Said: DeserializeOwned + Send + 'static,
{
    let sink = move |incoming| {
        let _ = sink.unbounded_send(incoming);
    };
    let host = mach::Process::at_other_end(&stream);
    let stop = Arc::new(AtomicBool::new(false));
    let tell = writer(stream.try_clone()?, name)?;
    let pid = match host {
        Ok(host) => {
            let receive = move || pictures.receive(host, 200);
            rings(receive, stream.try_clone()?, Arc::clone(&stop), name, sink.clone())?;
            Some(host.pid())
        }
        Err(error) => {
            log::info!("a host went before it could be named ({name}): {error}");
            None
        }
    };
    let running = Running {
        pid,
        tell,
        stop: Arc::clone(&stop),
    };
    reader(stream, stop, name, sink)?;
    Ok(running)
}

/// The socket's writes, off the main thread.
fn writer<Tell: Serialize + Send + 'static>(
    mut stream: UnixStream,
    name: &str,
) -> io::Result<Sender<Tell>> {
    let (tell, told) = channel::<Tell>();
    std::thread::Builder::new()
        .name(format!("{name}-tell"))
        .spawn(move || {
            while let Ok(message) = told.recv() {
                if write_frame(&mut stream, &message).is_err() {
                    return;
                }
            }
        })?;
    Ok(tell)
}

/// What the host says, until its link closes, which ends the pictures too.
fn reader<Said: DeserializeOwned + 'static>(
    stream: UnixStream,
    stop: Arc<AtomicBool>,
    name: &str,
    sink: impl Fn(Incoming<Said>) + Send + 'static,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!("{name}-said"))
        .spawn(move || {
            for said in koyo_frame::frames(stream, read_frame::<Said>) {
                sink(Incoming::Said(said));
            }
            stop.store(true, Ordering::Relaxed);
            sink(Incoming::Closed);
        })?;
    Ok(())
}

/// The host's pictures as `receive` gets them, until `stop`. A failure to receive is the
/// receiver's end: rather than spin on it, or leave the host running with no picture, the
/// thread shuts `link` down, which the reader then hears as the link's end.
fn rings<Said: 'static>(
    mut receive: impl FnMut() -> io::Result<Option<mach::Ring>> + Send + 'static,
    link: UnixStream,
    stop: Arc<AtomicBool>,
    name: &str,
    sink: impl Fn(Incoming<Said>) + Send + 'static,
) -> io::Result<()> {
    let thread = format!("{name}-rings");
    std::thread::Builder::new()
        .name(thread.clone())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match receive() {
                    Ok(Some(ring)) => sink(Incoming::Ring(ring)),
                    Ok(None) => {}
                    Err(error) => {
                        log::warn!("a host's pictures can't be received ({thread}): {error}");
                        let _ = link.shutdown(Shutdown::Both);
                        return;
                    }
                }
            }
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_failed_receive_ends_the_pictures_thread() {
        let tries = Arc::new(AtomicUsize::new(0));
        let counts = Arc::clone(&tries);
        let (sunk, ended) = channel::<()>();
        let stop = Arc::new(AtomicBool::new(false));
        let receive = move || {
            counts.fetch_add(1, Ordering::Relaxed);
            Err(io::Error::other("the port is gone"))
        };
        let (link, _host) = UnixStream::pair().unwrap();
        rings(receive, link, Arc::clone(&stop), "test", move |_: Incoming<()>| {
            let _ = sunk.send(());
        })
        .unwrap();
        let ended = ended.recv_timeout(Duration::from_secs(1));
        stop.store(true, Ordering::Relaxed);
        assert_eq!(
            ended,
            Err(RecvTimeoutError::Disconnected),
            "the thread still ran a second later, after {} receives",
            tries.load(Ordering::Relaxed)
        );
        assert_eq!(tries.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_receiver_that_failed_closes_the_link() {
        let (link, _host) = UnixStream::pair().unwrap();
        let (said, heard) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let sink = move |incoming: Incoming<()>| {
            let _ = said.send(matches!(incoming, Incoming::Closed));
        };
        reader(link.try_clone().unwrap(), Arc::clone(&stop), "test", sink.clone()).unwrap();
        let receive = || Err(io::Error::other("the receiver is gone"));
        rings(receive, link, Arc::clone(&stop), "test", sink).unwrap();
        let closed = heard.recv_timeout(Duration::from_secs(1));
        assert_eq!(closed, Ok(true), "the host still ran, its pictures gone");
        assert!(stop.load(Ordering::Relaxed));
    }

    #[test]
    fn what_a_host_said_before_it_went_is_heard_then_the_close() {
        let (mut app, host) = UnixStream::pair().unwrap();
        let mut said = Vec::new();
        for word in ["hello", "unavailable"] {
            write_frame(&mut said, &word).unwrap();
        }
        let mut cat = std::process::Command::new("/bin/cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::os::fd::OwnedFd::from(host))
            .spawn()
            .unwrap();
        io::Write::write_all(&mut cat.stdin.take().unwrap(), &said).unwrap();
        assert!(cat.wait().unwrap().success());
        let hello = koyo_frame::hello(
            &mut app,
            koyo_frame::HELLO_WITHIN,
            koyo_frame::End::App,
            1,
            |frame| Ok(read_frame::<String>(frame)?.map(|said| (1, said))),
        );
        assert_eq!(hello.unwrap(), "hello");
        let (sink, heard) = mpsc::unbounded();
        let pictures = mach::Receiver::check_in("codes.koyo.link-test").unwrap();
        let running = run::<(), String>(app, pictures, "test", sink)
            .expect("the link to a host that went");
        assert_eq!(running.pid, None);
        let heard: Vec<_> = futures::executor::block_on_stream(heard)
            .map(|incoming| match incoming {
                Incoming::Said(said) => said,
                Incoming::Ring(_) => "a ring".into(),
                Incoming::Closed => "closed".into(),
            })
            .collect();
        assert_eq!(heard, ["unavailable", "closed"]);
    }
}
