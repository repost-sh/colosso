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
/// threads started by then let the socket go, so the host hears it end at once: the writer
/// with `tell`, and the reader, which only the socket's end stops, once the pictures, started
/// last, fail to start and shut it.
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
    let pictures = match mach::Process::at_other_end(&stream) {
        Ok(host) => Some((host.pid(), move || pictures.receive(host, 200))),
        Err(error) => {
            log::info!("a host went before it could be named ({name}): {error}");
            None
        }
    };
    link(stream, pictures, name, sink, &thread)
}

/// Starts a thread named `name` that runs `work`.
type Spawn<'a> = &'a dyn Fn(String, Box<dyn FnOnce() + Send>) -> io::Result<()>;

fn thread(name: String, work: Box<dyn FnOnce() + Send>) -> io::Result<()> {
    std::thread::Builder::new().name(name).spawn(work).map(drop)
}

/// [`run`], the host's pictures as `pictures` receives them from its process, by its pid, and
/// the link's threads started with `spawn`.
fn link<Tell, Said>(
    stream: UnixStream,
    pictures: Option<(i32, impl FnMut() -> io::Result<Option<mach::Ring>> + Send + 'static)>,
    name: &str,
    sink: Sink<Said>,
    spawn: Spawn<'_>,
) -> io::Result<Running<Tell>>
where
    Tell: Serialize + Send + 'static,
    Said: DeserializeOwned + Send + 'static,
{
    let sink = sink_of(sink);
    let stop = Arc::new(AtomicBool::new(false));
    let tell = writer(stream.try_clone()?, name, spawn)?;
    let link = stream.try_clone()?;
    reader(stream, Arc::clone(&stop), name, sink.clone(), spawn)?;
    let running = Running {
        pid: pictures.as_ref().map(|(pid, _)| *pid),
        tell,
        stop: Arc::clone(&stop),
    };
    if let Some((_, receive)) = pictures {
        let started = link.try_clone().and_then(|ours| {
            let end = move || drop(ours.shutdown(Shutdown::Write));
            rings(receive, end, stop, name, sink, spawn)
        });
        if let Err(error) = started {
            let _ = link.shutdown(Shutdown::Both);
            return Err(error);
        }
    }
    Ok(running)
}

/// Puts what the host says in `sink`; a ring nobody is there to take gives its rights back.
fn sink_of<Said>(sink: Sink<Said>) -> impl Fn(Incoming<Said>) + Clone + Send + 'static
where
    Said: Send + 'static,
{
    move |incoming| {
        if let Err(unsent) = sink.unbounded_send(incoming)
            && let Incoming::Ring(ring) = unsent.into_inner()
        {
            ring.ports.into_iter().filter(|port| *port != 0).for_each(mach::release);
        }
    }
}

/// The socket's writes, off the main thread.
fn writer<Tell: Serialize + Send + 'static>(
    mut stream: impl io::Write + Send + 'static,
    name: &str,
    spawn: Spawn<'_>,
) -> io::Result<Sender<Tell>> {
    let (tell, told) = channel::<Tell>();
    let work = move || {
        while let Ok(message) = told.recv() {
            if write_frame(&mut stream, &message).is_err() {
                return;
            }
        }
    };
    spawn(format!("{name}-tell"), Box::new(work))?;
    Ok(tell)
}

/// What the host says, until its link closes, which ends the pictures too.
fn reader<Said: DeserializeOwned + 'static>(
    stream: impl io::Read + Send + 'static,
    stop: Arc<AtomicBool>,
    name: &str,
    sink: impl Fn(Incoming<Said>) + Send + 'static,
    spawn: Spawn<'_>,
) -> io::Result<()> {
    let work = move || {
        for said in koyo_frame::frames(stream, read_frame::<Said>) {
            sink(Incoming::Said(said));
        }
        stop.store(true, Ordering::Relaxed);
        sink(Incoming::Closed);
    };
    spawn(format!("{name}-said"), Box::new(work))
}

/// The host's pictures as `receive` gets them, until `stop`. A failure to receive is the
/// receiver's end: rather than spin on it, or leave the host running with no picture, the
/// thread ends the link with `end`, which shuts the app's side for writing: the host hears the
/// link end and goes, and the reader hears it go once it has read all the host said.
fn rings<Said: 'static>(
    mut receive: impl FnMut() -> io::Result<Option<mach::Ring>> + Send + 'static,
    end: impl FnOnce() + Send + 'static,
    stop: Arc<AtomicBool>,
    name: &str,
    sink: impl Fn(Incoming<Said>) + Send + 'static,
    spawn: Spawn<'_>,
) -> io::Result<()> {
    let thread = format!("{name}-rings");
    let named = thread.clone();
    let work = move || {
        while !stop.load(Ordering::Relaxed) {
            match receive() {
                Ok(Some(ring)) => sink(Incoming::Ring(ring)),
                Ok(None) => {}
                Err(error) => {
                    log::warn!("a host's pictures can't be received ({named}): {error}");
                    end();
                    return;
                }
            }
        }
    };
    spawn(thread, Box::new(work))
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
        let (told, ended) = channel::<()>();
        let stop = Arc::new(AtomicBool::new(false));
        let receive = move || {
            counts.fetch_add(1, Ordering::Relaxed);
            Err(io::Error::other("the port is gone"))
        };
        let sink = move |_: Incoming<()>| {
            let _ = told.send(());
        };
        rings(receive, || {}, Arc::clone(&stop), "test", sink, &thread).unwrap();
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

    /// What `heard` got once the link's threads let its sink go, each said message by its
    /// words.
    fn heard(heard: mpsc::UnboundedReceiver<Incoming<String>>) -> Vec<String> {
        futures::executor::block_on_stream(heard)
            .map(|incoming| match incoming {
                Incoming::Said(said) => said,
                Incoming::Ring(_) => "a ring".into(),
                Incoming::Closed => "closed".into(),
            })
            .collect()
    }

    #[test]
    fn a_link_that_fails_to_start_lets_its_host_go_at_once() {
        let mut waited_on = Vec::new();
        for failing in ["test-tell", "test-said", "test-rings"] {
            let (app, mut host) = UnixStream::pair().unwrap();
            host.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
            let (sink, _heard) = mpsc::unbounded::<Incoming<String>>();
            let (receiving, received) = channel();
            let (never, waits) = channel::<()>();
            let receive = move || {
                let _ = receiving.send(());
                let _ = waits.recv_timeout(Duration::from_millis(200));
                Ok(None)
            };
            let spawn = |name: String, work: Box<dyn FnOnce() + Send>| {
                if name != failing {
                    return thread(name, work);
                }
                let _ = received.recv_timeout(Duration::from_millis(100));
                Err(io::Error::other("no thread to be had"))
            };
            let started = link::<(), String>(app, Some((1, receive)), "test", sink, &spawn);
            assert!(started.is_err(), "{failing} didn't start, yet the link did");
            if !matches!(io::Read::read(&mut host, &mut [0; 1]), Ok(0)) {
                waited_on.push(failing);
            }
            drop(never);
        }
        assert_eq!(waited_on, [""; 0], "the host still waited on the link once these failed");
    }

    #[test]
    fn what_a_host_said_is_heard_though_its_pictures_failed_at_once() {
        let (app, mut host) = UnixStream::pair().unwrap();
        host.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        for word in ["one", "two"] {
            write_frame(&mut host, &word).unwrap();
        }
        let (sink, heard_by_app) = mpsc::unbounded();
        let receive = || Err(io::Error::other("the receiver is gone"));
        let running = link::<(), String>(app, Some((1, receive)), "test", sink, &thread).unwrap();
        let mut rest = Vec::new();
        let ended = io::Read::read_to_end(&mut host, &mut rest).map_err(|error| error.kind());
        assert_eq!(ended, Ok(0), "the host heard the link end");
        drop(host);
        assert_eq!(heard(heard_by_app), ["one", "two", "closed"]);
        assert!(running.stop.load(Ordering::Relaxed), "the pictures went on");
    }

    unsafe extern "C" {
        static mach_task_self_: u32;
        fn mach_port_get_refs(task: u32, name: u32, right: u32, refs: *mut u32) -> i32;
    }

    #[test]
    fn a_ring_nobody_takes_gives_back_its_rights() {
        const MACH_PORT_RIGHT_SEND: u32 = 0;
        let port = mach::new_port().unwrap();
        let (sink, heard) = mpsc::unbounded::<Incoming<()>>();
        drop(heard);
        let ports = vec![port];
        sink_of(sink)(Incoming::Ring(mach::Ring { pane: 1, generation: 1, ports }));
        let (mut refs, right) = (0, MACH_PORT_RIGHT_SEND);
        // SAFETY: an out pointer for the count of a name this process holds the receive right
        // to, so the name stays this process's whatever its send rights.
        let kr = unsafe { mach_port_get_refs(mach_task_self_, port, right, &raw mut refs) };
        assert_eq!((kr, refs), (0, 0), "the ring's send right was kept");
    }
}
