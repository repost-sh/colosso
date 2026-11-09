use std::os::unix::net::UnixStream;

use super::*;

/// RFC 2409's 1024-bit MODP group (Oakley group 2), generator 2.
const PRIME: &str = "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE649286651ECE65381FFFFFFFFFFFFFFFF";

fn prime() -> Vec<u8> {
    (0..PRIME.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&PRIME[at..at + 2], 16).unwrap())
        .collect()
}

fn take(stream: &mut UnixStream, n: usize) -> Vec<u8> {
    let mut buffer = vec![0u8; n];
    stream.read_exact(&mut buffer).unwrap();
    buffer
}

fn c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|byte| *byte == 0).unwrap();
    String::from_utf8(bytes[..end].to_vec()).unwrap()
}

/// The Mac's side of a type-30 login and session select, checking what the client sent:
/// `(name, password)` it decrypted, and the session-select command.
fn server(mut stream: UnixStream, refuse_session: bool) -> (String, String, u8) {
    stream.write_all(b"RFB 003.889\n").unwrap();
    assert_eq!(take(&mut stream, 12), b"RFB 003.889\n");
    stream.write_all(&[2, 2, 30]).unwrap();
    assert_eq!(take(&mut stream, 1), [30]);
    let prime = prime();
    let pair = KeyPair::new(2, &prime).unwrap();
    let mut header = 2u16.to_be_bytes().to_vec();
    header.extend_from_slice(&u16::try_from(prime.len()).unwrap().to_be_bytes());
    stream.write_all(&header).unwrap();
    stream.write_all(&prime).unwrap();
    stream.write_all(&pair.public).unwrap();
    let encrypted = take(&mut stream, 128);
    let client_public = take(&mut stream, prime.len());
    let key = crypto::md5(&pair.shared(&client_public).unwrap());
    let credentials = crypto::aes_ecb(false, &key, &encrypted).unwrap();
    stream.write_all(&0u32.to_be_bytes()).unwrap();
    assert_eq!(take(&mut stream, 1), [0xC1]);
    let mut init = vec![0u8; 24];
    init[0..2].copy_from_slice(&1680u16.to_be_bytes());
    init[2..4].copy_from_slice(&1050u16.to_be_bytes());
    let mut name = vec![0u8; 22];
    name[2..6].copy_from_slice(&0x0000_000Cu32.to_be_bytes());
    name.extend_from_slice(b"Mac");
    init[20..24].copy_from_slice(&u32::try_from(name.len()).unwrap().to_be_bytes());
    stream.write_all(&init).unwrap();
    stream.write_all(&name).unwrap();
    let info = [0u8, 1, 0, 0, 0, 0x07];
    stream.write_all(&6u16.to_be_bytes()).unwrap();
    stream.write_all(&info).unwrap();
    let select = take(&mut stream, 74 + 128);
    assert_eq!(
        &select[74..],
        encrypted.as_slice(),
        "the credentials go again with the select"
    );
    let result: [u8; 3] = [0, 0, u8::from(refuse_session)];
    stream.write_all(&3u16.to_be_bytes()).unwrap();
    stream.write_all(&result).unwrap();
    (
        c_string(&credentials[..64]),
        c_string(&credentials[64..]),
        select[8],
    )
}

#[test]
fn a_type_30_login_carries_the_credentials_the_mac_can_decrypt() {
    let (mut client, mac) = UnixStream::pair().unwrap();
    let mac = std::thread::spawn(move || server(mac, false));
    let server_init = login(&mut client, "koyo-agents", "s3cret-password").unwrap();
    let (name, password, command) = mac.join().unwrap();
    assert_eq!(
        (name.as_str(), password.as_str()),
        ("koyo-agents", "s3cret-password")
    );
    assert_eq!(command, 2, "connect to a virtual display");
    assert_eq!(
        server_init.size,
        Size {
            width: 1680,
            height: 1050
        }
    );
    assert_eq!(server_init.flags & 0x04, 0x04);
}

#[test]
fn a_refused_session_is_an_error() {
    let (mut client, mac) = UnixStream::pair().unwrap();
    let mac = std::thread::spawn(move || server(mac, true));
    let error = login(&mut client, "koyo-agents", "pw").unwrap_err();
    mac.join().unwrap();
    assert!(error.to_string().contains("session was refused"), "{error}");
}

#[test]
fn another_protocol_is_refused_at_its_banner() {
    let (mut client, mut mac) = UnixStream::pair().unwrap();
    mac.write_all(b"RFB 003.008\n").unwrap();
    let error = login(&mut client, "a", "b").unwrap_err();
    assert!(error.to_string().contains("003.889"), "{error}");
}

fn field(message: &[u8], at: usize) -> u32 {
    be32(&message[at..])
}

#[test]
fn a_display_configuration_asks_for_one_dynamic_display_at_1x() {
    let message = display_configuration(Size {
        width: 1680,
        height: 1050,
    });
    // Five modes: the one asked for and the four presets, none of the same pixel size.
    assert_eq!(message.len(), 4 + 8 + 0x9c + 5 * 0x1c);
    assert_eq!(message[0], 0x1d);
    assert_eq!(usize::from(be16(&message[2..])), message.len() - 4);
    let descriptor = &message[12..];
    assert_eq!(field(descriptor, 0x7a), 1, "dynamic");
    assert_eq!(
        (field(descriptor, 0x7a + 16), field(descriptor, 0x7a + 20)),
        (3840, 2160)
    );
    assert_eq!(be16(&descriptor[0x7a + 32..]), 5);
    let first = &descriptor[0x9c..];
    assert_eq!(
        [
            field(first, 0),
            field(first, 4),
            field(first, 8),
            field(first, 12)
        ],
        [1680, 1050, 1680, 1050]
    );
    assert_eq!(
        u64::from_be_bytes(first[16..24].try_into().unwrap()),
        60f64.to_bits()
    );
}

#[test]
fn a_preset_of_the_same_pixel_size_is_left_out() {
    let message = display_configuration(Size {
        width: 3840,
        height: 2160,
    });
    assert_eq!(be16(&message[12 + 0x7a + 32..]), 4);
}

#[test]
fn an_update_carrying_a_layout_says_the_screen_size() {
    let mut update = vec![0u8, 0, 0, 2];
    // A layout pseudo-rectangle: 0x451, its body's length, the body (framebuffer 3360 × 1050).
    let mut rect = vec![0u8; 12];
    rect[8..12].copy_from_slice(&0x451u32.to_be_bytes());
    update.extend_from_slice(&rect);
    let mut body = vec![0u8; 0x14];
    body[6..8].copy_from_slice(&3360u16.to_be_bytes());
    body[8..10].copy_from_slice(&1050u16.to_be_bytes());
    update.extend_from_slice(&u16::try_from(body.len()).unwrap().to_be_bytes());
    update.extend_from_slice(&body);
    // And a zlib rectangle, read through unread.
    let mut zlib = vec![0u8; 12];
    zlib[4..6].copy_from_slice(&10u16.to_be_bytes());
    zlib[6..8].copy_from_slice(&10u16.to_be_bytes());
    zlib[8..12].copy_from_slice(&6u32.to_be_bytes());
    update.extend_from_slice(&zlib);
    update.extend_from_slice(&5u32.to_be_bytes());
    update.extend_from_slice(&[1, 2, 3, 4, 5]);
    update.push(2);
    let mut reader = update.as_slice();
    assert_eq!(
        read_message(&mut reader, AREA_SCREEN).unwrap(),
        Some(Size {
            width: 3360,
            height: 1050
        })
    );
    assert_eq!(
        read_message(&mut reader, AREA_SCREEN).unwrap(),
        None,
        "the bell after it"
    );
    assert!(reader.is_empty());
}

const AREA_SCREEN: Size = Size {
    width: 1680,
    height: 1050,
};
