//! Moving frame surfaces between processes on macOS.
//!
//! A Unix socket cannot carry mach ports, so the app checks in a bootstrap
//! service under a random name, passes the name to the host it starts, and
//! the host sends each pane's surface ring there as one mach message with a
//! port per surface. The name is no secret: any process of the same user can
//! list it (`launchctl print gui/<uid>`), and the random part only keeps one
//! from checking the name in first. What keeps another process from injecting
//! surfaces is the audit token: the app takes only messages whose trailer
//! carries the token of the process at the other end of its host's socket
//! ([`Process`]).

use std::ffi::{CString, c_char};
use std::io;
use std::os::fd::AsRawFd as _;
use std::os::unix::net::UnixStream;

type KernReturn = i32;
type MachPort = u32;

const KERN_SUCCESS: KernReturn = 0;
const MACH_RCV_TIMED_OUT: KernReturn = 0x1000_4003;
const MACH_RCV_TOO_LARGE: KernReturn = 0x1000_4004;
const MACH_RCV_INTERRUPTED: KernReturn = 0x1000_4005;
const MACH_RCV_HEADER_ERROR: KernReturn = 0x1000_400b;
const MACH_RCV_BODY_ERROR: KernReturn = 0x1000_400c;
/// The bits some receive errors add to say where they happened.
const MACH_MSG_MASK: KernReturn = 0x3e00;

const MACH_SEND_MSG: i32 = 0x1;
const MACH_RCV_MSG: i32 = 0x2;
const MACH_SEND_TIMEOUT: i32 = 0x10;
const MACH_RCV_TIMEOUT: i32 = 0x100;
/// `MACH_RCV_TRAILER_TYPE(MACH_MSG_TRAILER_FORMAT_0) |
/// MACH_RCV_TRAILER_ELEMENTS(MACH_RCV_TRAILER_AUDIT)`.
const MACH_RCV_TRAILER_AUDIT: i32 = 3 << 24;

const MACH_MSG_TYPE_MOVE_SEND: u32 = 17;
const MACH_MSG_TYPE_COPY_SEND: u32 = 19;
const MACH_MSG_TYPE_MAKE_SEND: u32 = 20;
const MACH_MSGH_BITS_COMPLEX: u32 = 0x8000_0000;
const MACH_MSG_PORT_DESCRIPTOR: u8 = 0;
const MACH_PORT_RIGHT_RECEIVE: u32 = 1;

/// The message id of a surface ring.
const SURFACES_ID: i32 = i32::from_be_bytes(*b"MCBR");

unsafe extern "C" {
    static bootstrap_port: MachPort;
    static mach_task_self_: MachPort;
    fn bootstrap_check_in(bp: MachPort, name: *const c_char, sp: *mut MachPort) -> KernReturn;
    fn bootstrap_look_up(bp: MachPort, name: *const c_char, sp: *mut MachPort) -> KernReturn;
    fn mach_msg(
        msg: *mut Header,
        option: i32,
        send_size: u32,
        rcv_size: u32,
        rcv_name: MachPort,
        timeout: u32,
        notify: MachPort,
    ) -> KernReturn;
    fn mach_port_deallocate(task: MachPort, name: MachPort) -> KernReturn;
    fn mach_port_mod_refs(task: MachPort, name: MachPort, right: u32, delta: i32) -> KernReturn;
    fn mach_port_allocate(task: MachPort, right: u32, name: *mut MachPort) -> KernReturn;
    fn mach_port_insert_right(
        task: MachPort,
        name: MachPort,
        right: MachPort,
        disposition: u32,
    ) -> KernReturn;
}

fn task() -> MachPort {
    // SAFETY: the kernel sets this before main runs and never changes it.
    unsafe { mach_task_self_ }
}

fn check(kr: KernReturn, what: &str) -> io::Result<()> {
    if kr == KERN_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::other(format!("{what} failed: {kr:#x}")))
    }
}

#[repr(C, packed(4))]
#[derive(Clone, Copy, Default)]
struct Header {
    bits: u32,
    size: u32,
    remote_port: MachPort,
    local_port: MachPort,
    voucher_port: MachPort,
    id: i32,
}

#[repr(C, packed(4))]
#[derive(Clone, Copy, Default)]
struct PortDescriptor {
    name: MachPort,
    pad1: u32,
    pad2: u16,
    disposition: u8,
    kind: u8,
}

#[repr(C, packed(4))]
#[derive(Clone, Copy, Default)]
struct SurfacesMessage {
    header: Header,
    descriptor_count: u32,
    ports: [PortDescriptor; crate::RING as usize],
    pane: u64,
    generation: u32,
    count: u32,
}

#[repr(C, packed(4))]
#[derive(Clone, Copy, Default)]
struct AuditTrailer {
    kind: u32,
    size: u32,
    seqno: u32,
    sender: [u32; 2],
    audit: [u32; 8],
}

#[repr(C, packed(4))]
#[derive(Clone, Copy, Default)]
struct Received {
    message: SurfacesMessage,
    trailer: AuditTrailer,
}

/// One pane's ring, as received: send rights to the surfaces, owned by the
/// caller (hand each to `IOSurfaceLookupFromMachPort`, then [`release`]).
#[derive(Debug, PartialEq, Eq)]
pub struct Ring {
    /// The pane.
    pub pane: u64,
    /// The ring's generation.
    pub generation: u32,
    /// One port per slot.
    pub ports: Vec<u32>,
}

/// Drops a send right this process holds.
pub fn release(port: u32) {
    // SAFETY: deallocating a name we own; an invalid name only returns an error.
    unsafe {
        mach_port_deallocate(task(), port);
    }
}

/// A process as the kernel vouches for it: its audit token, which a socket's peer has
/// (`LOCAL_PEERTOKEN`) and every mach message carries in its trailer. A process started later
/// under the same pid has another token: the pid's version in it differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Process {
    token: [u32; 8],
}

impl Process {
    /// The process at the other end of `stream`, as the kernel has it. On a socket pair that is
    /// the one that used the other end last: ask once it has spoken.
    ///
    /// # Errors
    /// When the socket isn't connected, or the process at its other end is gone.
    pub fn at_other_end(stream: &UnixStream) -> io::Result<Self> {
        let mut token = [0u32; 8];
        let mut size = libc::socklen_t::try_from(size_of_val(&token)).map_err(io::Error::other)?;
        // SAFETY: a live socket, and an out-pointer of the size the option is said to have.
        let got = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERTOKEN,
                (&raw mut token).cast(),
                &raw mut size,
            )
        };
        if got == 0 {
            Ok(Self { token })
        } else {
            let error = io::Error::last_os_error();
            Err(io::Error::new(
                error.kind(),
                format!("no process at the other end of the socket to name: {error}"),
            ))
        }
    }

    /// Its pid.
    #[must_use]
    pub fn pid(&self) -> i32 {
        self.token[5].cast_signed()
    }
}

/// What a receive got, by the code it returned and what it left in the buffer.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// A ring from the sender.
    Ring,
    /// A message to give back with all it brought: not a ring, not the sender's, or one the
    /// kernel copied out only in part (`MACH_RCV_BODY_ERROR`, after it moved in the rights it
    /// could).
    Refused,
    /// No message: none came in time, or the one taken off the queue went with all it brought
    /// (too large for the buffer, since `MACH_RCV_LARGE` isn't asked for, or its header not
    /// copied out).
    Nothing,
    /// The receiver can't receive any more.
    Dead,
}

fn verdict(kr: KernReturn, buffer: &Received, sender: Process) -> Verdict {
    if kr != KERN_SUCCESS {
        return match kr & !MACH_MSG_MASK {
            MACH_RCV_BODY_ERROR => Verdict::Refused,
            MACH_RCV_TIMED_OUT
            | MACH_RCV_TOO_LARGE
            | MACH_RCV_INTERRUPTED
            | MACH_RCV_HEADER_ERROR => Verdict::Nothing,
            _ => Verdict::Dead,
        };
    }
    let audit = buffer.trailer.audit;
    if wellformed(&buffer.message) && audit == sender.token {
        Verdict::Ring
    } else {
        Verdict::Refused
    }
}

/// Whether `message` is a ring as [`Sender::send`] makes one: only then is the trailer where
/// the buffer has it, and the ports what the message says. It brings a send right or nothing
/// in each slot, and no reply port or voucher.
fn wellformed(message: &SurfacesMessage) -> bool {
    let header = message.header;
    header.size as usize == size_of::<SurfacesMessage>()
        && header.id == SURFACES_ID
        && header.bits & MACH_MSGH_BITS_COMPLEX != 0
        && header.remote_port == 0
        && header.voucher_port == 0
        && message.descriptor_count == u32::from(crate::RING)
        && message.ports.iter().all(PortDescriptor::send_right_or_none)
        && message.count <= u32::from(crate::RING)
}

impl PortDescriptor {
    /// Whether it's a port descriptor as received with a send right, or with no port.
    fn send_right_or_none(&self) -> bool {
        let Self {
            name,
            disposition,
            kind,
            ..
        } = *self;
        kind == MACH_MSG_PORT_DESCRIPTOR
            && (name == 0 || u32::from(disposition) == MACH_MSG_TYPE_MOVE_SEND)
    }
}

/// The app's end: a checked-in service that receives surface rings.
#[derive(Debug)]
pub struct Receiver {
    port: MachPort,
    name: String,
}

impl Receiver {
    /// Checks in a service named `prefix` and a random suffix with the bootstrap server, so no
    /// other process can check the name in before this one does. It doesn't hide the name:
    /// [`Receiver::receive`] tells the sender by its audit token.
    ///
    /// # Errors
    /// When there's no randomness to be had or the bootstrap server refuses the name.
    pub fn check_in(prefix: &str) -> io::Result<Self> {
        let name = format!("{prefix}.{}", crate::client::token()?);
        let c = CString::new(name.as_str()).map_err(io::Error::other)?;
        let mut port = 0;
        // SAFETY: valid C string and out pointer.
        check(
            unsafe { bootstrap_check_in(bootstrap_port, c.as_ptr(), &raw mut port) },
            "bootstrap_check_in",
        )?;
        Ok(Self { port, name })
    }

    /// The service name to give the host.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Waits up to `timeout_ms` for a ring. Messages from any process other
    /// than `sender` (a later one under its pid too), any that isn't a ring,
    /// and any the kernel copied out only in part, are dropped with the
    /// rights they brought.
    ///
    /// # Errors
    /// When the receiver can't receive any more: an error is never one
    /// message's.
    pub fn receive(&self, sender: Process, timeout_ms: u32) -> io::Result<Option<Ring>> {
        let mut buffer = Received::default();
        // SAFETY: the buffer is large enough for the message and the trailer
        // requested, and lives across the call.
        let kr = unsafe {
            mach_msg(
                (&raw mut buffer).cast(),
                MACH_RCV_MSG | MACH_RCV_TIMEOUT | MACH_RCV_TRAILER_AUDIT,
                0,
                u32::try_from(size_of::<Received>()).map_err(io::Error::other)?,
                self.port,
                timeout_ms,
                0,
            )
        };
        match verdict(kr, &buffer, sender) {
            Verdict::Nothing => Ok(None),
            Verdict::Dead => Err(io::Error::other(format!(
                "mach_msg receive failed: {kr:#x}"
            ))),
            Verdict::Refused => {
                // SAFETY: a refusal follows only a receive that copied a message out to the
                // buffer, whole or in part.
                unsafe { refused::give_back(&mut buffer) };
                Ok(None)
            }
            Verdict::Ring => {
                let message = buffer.message;
                let mut ports: Vec<u32> = message.ports.iter().map(|p| p.name).collect();
                let count = message.count as usize;
                ports.drain(count..).filter(|p| *p != 0).for_each(release);
                Ok(Some(Ring {
                    pane: message.pane,
                    generation: message.generation,
                    ports,
                }))
            }
        }
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        // SAFETY: dropping the receive right we checked in ends the service.
        unsafe {
            mach_port_mod_refs(task(), self.port, MACH_PORT_RIGHT_RECEIVE, -1);
        }
    }
}

/// The host's end: a send right to the app's service.
#[derive(Debug)]
pub struct Sender {
    port: MachPort,
}

impl Sender {
    /// Looks up the app's service.
    ///
    /// # Errors
    /// When no such service is checked in.
    pub fn look_up(name: &str) -> io::Result<Self> {
        let c = CString::new(name).map_err(io::Error::other)?;
        let mut port = 0;
        // SAFETY: valid C string and out pointer.
        check(
            unsafe { bootstrap_look_up(bootstrap_port, c.as_ptr(), &raw mut port) },
            "bootstrap_look_up",
        )?;
        Ok(Self { port })
    }

    /// Sends a ring. The send rights in `ports` move to the app: the caller
    /// must not release them, whether or not sending succeeds.
    ///
    /// # Errors
    /// When there are more ports than [`crate::RING`] or the send fails.
    pub fn send(&self, pane: u64, generation: u32, ports: &[u32]) -> io::Result<()> {
        if ports.len() > crate::RING as usize {
            ports.iter().copied().for_each(release);
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "too many surfaces",
            ));
        }
        let mut message = message(self.port, pane, generation, ports)?;
        // SAFETY: the message is fully initialized and sized in its header.
        let kr = unsafe {
            mach_msg(
                (&raw mut message).cast(),
                MACH_SEND_MSG | MACH_SEND_TIMEOUT,
                message.header.size,
                0,
                0,
                1_000,
                0,
            )
        };
        check(kr, "mach_msg send")
    }
}

/// A ring as it goes to the service at `remote`: one descriptor a slot, which moves the send
/// right in `ports` there, or none.
fn message(
    remote: MachPort,
    pane: u64,
    generation: u32,
    ports: &[u32],
) -> io::Result<SurfacesMessage> {
    let mut message = SurfacesMessage {
        header: Header {
            bits: MACH_MSGH_BITS_COMPLEX | MACH_MSG_TYPE_COPY_SEND,
            size: u32::try_from(size_of::<SurfacesMessage>()).map_err(io::Error::other)?,
            remote_port: remote,
            id: SURFACES_ID,
            ..Header::default()
        },
        descriptor_count: u32::from(crate::RING),
        pane,
        generation,
        count: u32::try_from(ports.len()).map_err(io::Error::other)?,
        ..SurfacesMessage::default()
    };
    for (slot, port) in message.ports.iter_mut().zip(ports) {
        *slot = PortDescriptor {
            name: *port,
            disposition: u8::try_from(MACH_MSG_TYPE_MOVE_SEND).unwrap_or_default(),
            kind: MACH_MSG_PORT_DESCRIPTOR,
            ..PortDescriptor::default()
        };
    }
    for slot in message.ports.iter_mut().skip(ports.len()) {
        slot.kind = MACH_MSG_PORT_DESCRIPTOR;
    }
    Ok(message)
}

impl Drop for Sender {
    fn drop(&mut self) {
        release(self.port);
    }
}

/// A fresh port with a send right, for tests and placeholders.
///
/// # Errors
/// When the kernel refuses a new port.
pub fn new_port() -> io::Result<u32> {
    let mut name = 0;
    // SAFETY: out pointer is valid; the right we insert is for that name.
    unsafe {
        check(
            mach_port_allocate(task(), MACH_PORT_RIGHT_RECEIVE, &raw mut name),
            "mach_port_allocate",
        )?;
        check(
            mach_port_insert_right(task(), name, name, MACH_MSG_TYPE_MAKE_SEND),
            "mach_port_insert_right",
        )?;
    }
    Ok(name)
}

mod refused;

#[cfg(test)]
mod tests;
