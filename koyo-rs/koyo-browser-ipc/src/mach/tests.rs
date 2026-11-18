use std::os::unix::net::UnixStream;

use super::refused::{
    MACH_MSG_OOL_DESCRIPTOR, MACH_MSG_OOL_PORTS_DESCRIPTOR, MACH_MSG_OOL_VOLATILE_DESCRIPTOR,
};
use super::*;

const MACH_MSG_IPC_SPACE: KernReturn = 0x2000;
const MACH_MSG_VM_SPACE: KernReturn = 0x1000;
const MACH_RCV_INVALID_NAME: KernReturn = 0x1000_4002;
const MACH_RCV_PORT_DIED: KernReturn = 0x1000_4009;

const SERVICE: &str = "codes.koyo.browser-ipc-test";

/// This process, as the kernel names it at the other end of a socket pair it holds both ends of.
fn us() -> Process {
    let (ours, _theirs) = UnixStream::pair().expect("socket pair");
    Process::at_other_end(&ours).expect("our own token")
}

/// Another process than this one, its token `us` with `word` changed.
fn not_us(word: usize) -> Process {
    let mut other = us();
    other.token[word] += 1;
    other
}

#[test]
fn a_ring_reaches_the_service_with_its_ports() {
    let receiver = Receiver::check_in(SERVICE).expect("check in");
    let sender = Sender::look_up(receiver.name()).expect("look up");
    let ports = [new_port().expect("port"), new_port().expect("port")];
    sender.send(9, 4, &ports).expect("send");
    let ring = receiver
        .receive(us(), 1_000)
        .expect("receive")
        .expect("a ring");
    assert_eq!((ring.pane, ring.generation, ring.ports.len()), (9, 4, 2));
    assert_eq!(ring.ports, ports);
    ring.ports.into_iter().for_each(release);
}

/// Where an audit token has its pid.
const PID: usize = 5;

#[test]
fn rings_from_another_process_are_dropped_even_under_the_same_pid() {
    const PID_VERSION: usize = 7;
    let receiver = Receiver::check_in(SERVICE).expect("check in");
    let sender = Sender::look_up(receiver.name()).expect("look up");
    for other in [not_us(PID), not_us(PID_VERSION)] {
        sender
            .send(1, 1, &[new_port().expect("port")])
            .expect("send");
        let taken = receiver.receive(other, 1_000).expect("receive");
        assert_eq!(taken, None, "a ring from us was taken as {other:?}'s");
        assert_eq!(receiver.receive(us(), 10).expect("timeout"), None);
    }
}

#[test]
fn the_process_at_the_other_end_of_a_socket_pair_is_the_one_that_spoke() {
    let (mut app, host) = UnixStream::pair().unwrap();
    let mut command = std::process::Command::new("/bin/sh");
    command.args(["-c", "printf x >&0; read line"]);
    let mut child = crate::inherit::spawn(command, host).unwrap();
    let mut said = [0u8; 1];
    io::Read::read_exact(&mut app, &mut said).unwrap();
    let theirs = Process::at_other_end(&app).unwrap();
    io::Write::write_all(&mut app, b"\n").unwrap();
    child.wait().unwrap();
    assert_eq!(u32::try_from(theirs.pid()).ok(), Some(child.id()), "the host's pid is its own");
    assert_ne!(theirs, us());
}

#[test]
fn an_unknown_service_is_an_error() {
    assert!(Sender::look_up(&format!("{SERVICE}.never-checked-in")).is_err());
}

unsafe extern "C" {
    fn mach_port_get_refs(task: MachPort, name: MachPort, right: u32, refs: *mut u32)
    -> KernReturn;
}

const MACH_PORT_RIGHT_SEND: u32 = 0;

fn send_refs(port: MachPort) -> u32 {
    let mut refs = 0;
    // SAFETY: an out pointer for the count of a name this process holds.
    let kr = unsafe { mach_port_get_refs(task(), port, MACH_PORT_RIGHT_SEND, &raw mut refs) };
    assert_eq!(kr, KERN_SUCCESS, "mach_port_get_refs");
    refs
}

/// Sends `message`, whatever it says, to the service `sender` looked up.
fn send_raw<T>(sender: &Sender, message: &mut T, bits: u32, reply: MachPort) {
    let size = u32::try_from(size_of::<T>()).unwrap();
    // SAFETY: every message here starts with a header, and is as large as it says.
    unsafe {
        let header = std::ptr::from_mut(message).cast::<Header>();
        *header = Header {
            bits,
            size,
            remote_port: sender.port,
            local_port: reply,
            id: SURFACES_ID,
            ..Header::default()
        };
        let kr = mach_msg(header, MACH_SEND_MSG | MACH_SEND_TIMEOUT, size, 0, 0, 1_000, 0);
        assert_eq!(kr, KERN_SUCCESS, "mach_msg send");
    }
}

#[test]
fn a_message_too_large_is_dropped_and_the_rings_after_it_still_come() {
    #[repr(C)]
    struct Large {
        header: Header,
        body: [u8; 256],
    }
    let receiver = Receiver::check_in(SERVICE).expect("check in");
    let sender = Sender::look_up(receiver.name()).expect("look up");
    let mut large = Large {
        header: Header::default(),
        body: [7; 256],
    };
    send_raw(&sender, &mut large, MACH_MSG_TYPE_COPY_SEND, 0);
    let dropped = receiver.receive(us(), 1_000);
    assert_eq!(dropped.map_err(|error| error.to_string()), Ok(None));
    let port = new_port().expect("port");
    sender.send(5, 1, &[port]).expect("send");
    let ring = receiver
        .receive(us(), 1_000)
        .expect("receive")
        .expect("a ring");
    assert_eq!((ring.pane, &ring.ports), (5, &vec![port]));
    ring.ports.into_iter().for_each(release);
}

#[test]
fn a_message_that_isnt_a_ring_frees_none_of_the_receivers_own_rights() {
    let receiver = Receiver::check_in(SERVICE).expect("check in");
    let sender = Sender::look_up(receiver.name()).expect("look up");
    let own = new_port().expect("port");
    let mut forged = SurfacesMessage {
        descriptor_count: u32::from(crate::RING),
        count: u32::from(crate::RING),
        ..SurfacesMessage::default()
    };
    for slot in &mut forged.ports {
        slot.name = own;
    }
    send_raw(&sender, &mut forged, MACH_MSG_TYPE_COPY_SEND, 0);
    let refused = receiver.receive(us(), 1_000).expect("receive");
    assert_eq!(refused, None);
    assert_eq!(send_refs(own), 1, "the message's bytes named a right it then lost");
}

#[test]
fn a_refused_message_gives_back_the_rights_it_brought() {
    let receiver = Receiver::check_in(SERVICE).expect("check in");
    let sender = Sender::look_up(receiver.name()).expect("look up");
    let own = new_port().expect("port");
    let mut message = SurfacesMessage::default();
    let bits = MACH_MSG_TYPE_COPY_SEND | (MACH_MSG_TYPE_COPY_SEND << 8);
    send_raw(&sender, &mut message, bits, own);
    let refused = receiver.receive(us(), 1_000).expect("receive");
    assert_eq!(refused, None);
    assert_eq!(send_refs(own), 1, "the reply right the message brought was kept");
}

#[test]
fn receivers_checked_in_under_one_prefix_have_names_of_their_own() {
    let first = Receiver::check_in(SERVICE).expect("check in");
    let second = Receiver::check_in(SERVICE).expect("a second under the same prefix");
    assert_ne!(first.name(), second.name());
    for receiver in [&first, &second] {
        let suffix = receiver.name().strip_prefix(&format!("{SERVICE}.")).unwrap_or_default();
        assert!(suffix.len() >= 32, "{} is guessable", receiver.name());
    }
}

/// What a receive that returned `kr` gets from a buffer holding `message`, with `from`'s token
/// in its trailer, when it takes rings from this process.
fn verdict_on(kr: KernReturn, message: SurfacesMessage, from: Process) -> Verdict {
    let buffer = Received {
        message,
        trailer: AuditTrailer {
            audit: from.token,
            ..AuditTrailer::default()
        },
    };
    verdict(kr, &buffer, us())
}

/// A ring of two ports, as the host sends it.
fn sent() -> SurfacesMessage {
    message(0, 9, 4, &[1, 2]).expect("a ring")
}

#[test]
fn a_ring_is_a_message_as_the_sender_makes_it_and_nothing_else() {
    assert_eq!(verdict_on(KERN_SUCCESS, sent(), us()), Verdict::Ring);
    let changed = |change: fn(&mut SurfacesMessage)| {
        let mut message = sent();
        change(&mut message);
        message
    };
    let otherwise = [
        ("shorter", changed(|m| m.header.size -= 4)),
        ("longer", changed(|m| m.header.size += 4)),
        ("another id", changed(|m| m.header.id = SURFACES_ID + 1)),
        ("not complex", changed(|m| m.header.bits &= !MACH_MSGH_BITS_COMPLEX)),
        ("a descriptor short", changed(|m| m.descriptor_count -= 1)),
        ("a descriptor over", changed(|m| m.descriptor_count += 1)),
        ("more ports than a ring", changed(|m| m.count = u32::from(crate::RING) + 1)),
    ];
    for (how, message) in otherwise {
        assert_eq!(verdict_on(KERN_SUCCESS, message, us()), Verdict::Refused, "{how}");
    }
}

#[test]
fn a_message_copied_out_only_in_part_is_given_back_with_what_came() {
    for space in [MACH_MSG_IPC_SPACE, MACH_MSG_VM_SPACE] {
        let kr = MACH_RCV_BODY_ERROR | space;
        assert_eq!(verdict_on(kr, sent(), us()), Verdict::Refused, "{kr:#x}");
    }
}

#[test]
fn a_receive_that_took_no_message_is_nothing_and_any_other_failure_the_receivers_end() {
    let nothing = [
        MACH_RCV_TIMED_OUT,
        MACH_RCV_TOO_LARGE,
        MACH_RCV_INTERRUPTED,
        MACH_RCV_HEADER_ERROR | MACH_MSG_IPC_SPACE,
    ];
    for kr in nothing {
        assert_eq!(verdict_on(kr, sent(), us()), Verdict::Nothing, "{kr:#x}");
    }
    for kr in [MACH_RCV_INVALID_NAME, MACH_RCV_PORT_DIED] {
        assert_eq!(verdict_on(kr, sent(), us()), Verdict::Dead, "{kr:#x}");
    }
}

/// Set in the child process [`ran_alone_in_a_child`] starts.
const CHILD: &str = "KOYO_BROWSER_IPC_TEST_CHILD";

/// Runs this module's test `name` alone in a child process of this binary, where a crash takes
/// the child down rather than every test, and no other test maps memory meanwhile: true once
/// it passed there, false in the child, which then runs the test itself.
fn ran_alone_in_a_child(name: &str) -> bool {
    if std::env::var_os(CHILD).is_some() {
        return false;
    }
    let module = module_path!().split_once("::").map_or("", |(_, path)| path);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([&format!("{module}::{name}"), "--exact", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&child.stdout);
    assert!(child.status.success(), "{name} in a child: {}\n{said}", child.status);
    assert!(said.contains("1 passed"), "the child ran no test: {said}");
    true
}

const MACH_MSG_PHYSICAL_COPY: u8 = 0;
const MACH_MSG_VIRTUAL_COPY: u8 = 1;
/// A descriptor's deallocate and copy bytes as the kernel copies a port array out to a
/// receiver: moved, to be deallocated when it's done with.
const MOVED: [u8; 2] = [1, MACH_MSG_VIRTUAL_COPY];
/// The same as a sender sends what it keeps: copied, and not deallocated.
const KEPT: [u8; 2] = [0, MACH_MSG_PHYSICAL_COPY];

/// A descriptor as 64-bit userland lays it out: 8 bytes of address, deallocate and copy
/// (`memory`), disposition (or padding), type, then 4 bytes of size or count.
fn out_of_line(kind: u8, memory: [u8; 2], disposition: u8, address: u64, size: u32) -> [u8; 16] {
    let mut descriptor = [0; 16];
    descriptor[..8].copy_from_slice(&address.to_le_bytes());
    descriptor[8..12].copy_from_slice(&[memory[0], memory[1], disposition, kind]);
    descriptor[12..].copy_from_slice(&size.to_le_bytes());
    descriptor
}

/// A message whose body is `N` descriptors as [`out_of_line`] lays them out.
#[repr(C, packed(4))]
struct Carrying<const N: usize> {
    header: Header,
    descriptor_count: u32,
    descriptors: [[u8; 16]; N],
}

fn carrying<const N: usize>(descriptors: [[u8; 16]; N]) -> Carrying<N> {
    Carrying {
        header: Header::default(),
        descriptor_count: u32::try_from(N).unwrap(),
        descriptors,
    }
}

/// Where `bytes` start in this process.
fn address_of<T>(bytes: &[T]) -> u64 {
    u64::try_from(bytes.as_ptr().addr()).unwrap()
}

const MACH_TASK_BASIC_INFO: u32 = 20;

unsafe extern "C" {
    fn task_info(task: MachPort, flavor: u32, info: *mut u32, count: *mut u32) -> KernReturn;
}

/// The bytes of address space this process has mapped, as `mach_task_basic_info` counts them.
fn virtual_size() -> u64 {
    let mut info = [0u32; 12];
    let mut count = u32::try_from(info.len()).unwrap();
    // SAFETY: an out buffer of `count` words, the size of `mach_task_basic_info`.
    let kr = unsafe { task_info(task(), MACH_TASK_BASIC_INFO, info.as_mut_ptr(), &raw mut count) };
    assert_eq!(kr, KERN_SUCCESS, "task_info");
    u64::from(info[0]) | (u64::from(info[1]) << 32)
}

#[test]
fn a_refused_message_leaves_none_of_the_memory_it_brought_mapped() {
    const SIZE: u32 = 1 << 20;
    if ran_alone_in_a_child("a_refused_message_leaves_none_of_the_memory_it_brought_mapped") {
        return;
    }
    let receiver = Receiver::check_in(SERVICE).expect("check in");
    let sender = Sender::look_up(receiver.name()).expect("look up");
    let another = not_us(PID);
    let bytes = vec![7u8; SIZE as usize];
    let mut message = carrying([
        out_of_line(MACH_MSG_OOL_DESCRIPTOR, KEPT, 0, address_of(&bytes), SIZE),
        out_of_line(MACH_MSG_OOL_VOLATILE_DESCRIPTOR, KEPT, 0, address_of(&bytes), SIZE),
    ]);
    let before = virtual_size();
    send_raw(&sender, &mut message, MACH_MSGH_BITS_COMPLEX | MACH_MSG_TYPE_COPY_SEND, 0);
    assert_eq!(receiver.receive(another, 1_000).expect("receive"), None);
    let kept_mapped = virtual_size().saturating_sub(before);
    assert!(
        kept_mapped < u64::from(SIZE),
        "{kept_mapped} bytes of the {} the message brought are still mapped",
        2 * SIZE
    );
}

#[test]
fn a_refused_port_array_gives_back_the_rights_it_brought() {
    let receiver = Receiver::check_in(SERVICE).expect("check in");
    let sender = Sender::look_up(receiver.name()).expect("look up");
    let own = new_port().expect("port");
    let names = [own; 2];
    let disposition = u8::try_from(MACH_MSG_TYPE_COPY_SEND).unwrap();
    let mut message = carrying([out_of_line(
        MACH_MSG_OOL_PORTS_DESCRIPTOR,
        KEPT,
        disposition,
        address_of(&names),
        2,
    )]);
    send_raw(&sender, &mut message, MACH_MSGH_BITS_COMPLEX | MACH_MSG_TYPE_COPY_SEND, 0);
    assert_eq!(receiver.receive(not_us(PID), 1_000).expect("receive"), None);
    assert_eq!(send_refs(own), 1, "the rights the port array brought were kept");
}

#[test]
fn a_message_with_a_port_array_never_placed_here_is_given_back_without_a_crash() {
    if ran_alone_in_a_child(
        "a_message_with_a_port_array_never_placed_here_is_given_back_without_a_crash",
    ) {
        return;
    }
    let disposition = u8::try_from(MACH_MSG_TYPE_MOVE_SEND).unwrap();
    let mut buffer = Received::default();
    buffer.message.ports[0] = PortDescriptor {
        name: 0,
        disposition,
        kind: MACH_MSG_PORT_DESCRIPTOR,
        ..PortDescriptor::default()
    };
    let after_port = size_of::<Header>() + size_of::<u32>() + size_of::<PortDescriptor>();
    let out_of_line = [
        out_of_line(MACH_MSG_OOL_DESCRIPTOR, MOVED, 0, 0, 0),
        out_of_line(MACH_MSG_OOL_PORTS_DESCRIPTOR, MOVED, disposition, 0, 4),
    ]
    .concat();
    let end = after_port + out_of_line.len();
    refused::bytes(&mut buffer)[after_port..end].copy_from_slice(&out_of_line);
    buffer.message.header = Header {
        bits: MACH_MSGH_BITS_COMPLEX,
        size: u32::try_from(end).unwrap(),
        ..Header::default()
    };
    buffer.message.descriptor_count = 3;
    // SAFETY: a message as the kernel copies out one it couldn't place here, naming no right
    // and no memory: a null port, memory at address 0 and size 0, and a port array at
    // address 0.
    unsafe { refused::give_back(&mut buffer) };
}
