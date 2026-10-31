//! Moving frame surfaces between processes on macOS.
//!
//! A Unix socket cannot carry mach ports, so the app checks in a bootstrap
//! service under a random name, passes the name to the host it starts, and
//! the host sends each pane's surface ring there as one mach message with a
//! port per surface. The app only accepts messages whose audit token names
//! the host it started, so another process that guesses the name cannot
//! inject surfaces.

use std::ffi::{CString, c_char};
use std::io;

type KernReturn = i32;
type MachPort = u32;

const KERN_SUCCESS: KernReturn = 0;
const MACH_RCV_TIMED_OUT: KernReturn = 0x1000_4003;

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
const SURFACES_ID: i32 = 0x4d43_4252; // "MCBR"

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

/// The app's end: a checked-in service that receives surface rings.
#[derive(Debug)]
pub struct Receiver {
    port: MachPort,
    name: String,
}

impl Receiver {
    /// Checks in `name` with the bootstrap server.
    ///
    /// # Errors
    /// When the name is taken or the bootstrap server refuses it.
    pub fn check_in(name: &str) -> io::Result<Self> {
        let c = CString::new(name).map_err(io::Error::other)?;
        let mut port = 0;
        // SAFETY: valid C string and out pointer.
        check(
            unsafe { bootstrap_check_in(bootstrap_port, c.as_ptr(), &raw mut port) },
            "bootstrap_check_in",
        )?;
        Ok(Self {
            port,
            name: name.into(),
        })
    }

    /// The service name to give the host.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Waits up to `timeout_ms` for a ring. Messages from any process other
    /// than `sender` are dropped and their ports released.
    ///
    /// # Errors
    /// When receiving fails for another reason than the timeout.
    pub fn receive(&self, sender: u32, timeout_ms: u32) -> io::Result<Option<Ring>> {
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
        if kr == MACH_RCV_TIMED_OUT {
            return Ok(None);
        }
        check(kr, "mach_msg receive")?;
        let message = buffer.message;
        let ports: Vec<u32> = message.ports.iter().map(|p| p.name).collect();
        // The audit token's sixth word is the sender's pid.
        let pid = buffer.trailer.audit[5];
        let header = message.header;
        let wellformed = header.id == SURFACES_ID
            && header.bits & MACH_MSGH_BITS_COMPLEX != 0
            && message.descriptor_count == u32::from(crate::RING)
            && message.count <= u32::from(crate::RING);
        if pid != sender || !wellformed {
            ports.iter().copied().filter(|p| *p != 0).for_each(release);
            return Ok(None);
        }
        let count = message.count as usize;
        ports[count..]
            .iter()
            .copied()
            .filter(|p| *p != 0)
            .for_each(release);
        Ok(Some(Ring {
            pane: message.pane,
            generation: message.generation,
            ports: ports[..count].to_vec(),
        }))
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
        let mut message = SurfacesMessage {
            header: Header {
                bits: MACH_MSGH_BITS_COMPLEX | MACH_MSG_TYPE_COPY_SEND,
                size: u32::try_from(size_of::<SurfacesMessage>()).map_err(io::Error::other)?,
                remote_port: self.port,
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
        // Unused descriptors carry the null port.
        for slot in message.ports.iter_mut().skip(ports.len()) {
            slot.kind = MACH_MSG_PORT_DESCRIPTOR;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A name no other test uses: the clock alone repeats within a
    /// microsecond when tests run in parallel.
    fn service() -> String {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        format!(
            "dev.colosso.browser-ipc-test.{}.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }

    #[test]
    fn a_ring_reaches_the_service_with_its_ports() {
        let name = service();
        let receiver = Receiver::check_in(&name).expect("check in");
        let sender = Sender::look_up(receiver.name()).expect("look up");
        let ports = [new_port().expect("port"), new_port().expect("port")];
        sender.send(9, 4, &ports).expect("send");
        let ring = receiver
            .receive(std::process::id(), 1_000)
            .expect("receive")
            .expect("a ring");
        assert_eq!((ring.pane, ring.generation, ring.ports.len()), (9, 4, 2));
        // Within one task a moved send right keeps its name.
        assert_eq!(ring.ports, ports);
        ring.ports.into_iter().for_each(release);
    }

    #[test]
    fn rings_from_another_sender_are_dropped() {
        let receiver = Receiver::check_in(&service()).expect("check in");
        let sender = Sender::look_up(receiver.name()).expect("look up");
        sender
            .send(1, 1, &[new_port().expect("port")])
            .expect("send");
        assert_eq!(
            receiver
                .receive(std::process::id() + 1, 1_000)
                .expect("receive"),
            None
        );
        assert_eq!(
            receiver.receive(std::process::id(), 10).expect("timeout"),
            None
        );
    }

    #[test]
    fn an_unknown_service_is_an_error() {
        assert!(Sender::look_up(&service()).is_err());
    }
}
