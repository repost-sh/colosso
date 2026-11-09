//! Apple's RFB 003.889, as much as holding an agent session takes (`docs/agent-desktop.md` 6):
//! the type-30 login, `ClientInit` `0xC1`, session select command 2 ("connect to virtual
//! display"), `SetDisplayConfiguration` (`0x1d`) and the server's messages read through so
//! the stream stays in step. Login and session select follow Rocuronium's `AgentRFBProtocol`;
//! the display configuration and layouts follow `zDesk`'s notes; the bytes are the `PoC`'s
//! (`poc/src/rfbpoc.m`). No `SetEncryption` is ever sent, so the connection stays plain RFB,
//! on loopback only (the firewall anchor). The session's picture over RFB is black and is
//! never used.

use std::io::{self, Read, Write};

use koyo_desk_wire::Size;

use crate::crypto::{self, KeyPair};

const VERSION: &[u8; 12] = b"RFB 003.889\n";
const TYPE_30: u8 = 30;

/// What the server said at its init.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub size: Size,
    pub flags: u32,
}

fn read_n(stream: &mut impl Read, n: usize) -> io::Result<Vec<u8>> {
    let mut buffer = vec![0u8; n];
    stream.read_exact(&mut buffer)?;
    Ok(buffer)
}

fn be16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn refused(what: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, what.into())
}

/// The 128 bytes type 30 encrypts: the name at 0 and the password at 64, each ended by a zero,
/// the rest random.
fn credentials(user: &str, password: &str) -> io::Result<Vec<u8>> {
    if user.len() >= 64 || password.len() >= 64 {
        return Err(refused("a name or password of 64 bytes or more"));
    }
    let mut block = crypto::random(128);
    block[..user.len()].copy_from_slice(user.as_bytes());
    block[user.len()] = 0;
    block[64..64 + password.len()].copy_from_slice(password.as_bytes());
    block[64 + password.len()] = 0;
    Ok(block)
}

/// Logs `user` in and asks for a session of its own on a virtual display. Returns the server's
/// init once the session is accepted.
pub fn login<S: Read + Write>(stream: &mut S, user: &str, password: &str) -> io::Result<Server> {
    let banner = read_n(stream, 12)?;
    if banner != VERSION {
        return Err(refused(format!(
            "not Apple's RFB 003.889: {:?}",
            String::from_utf8_lossy(&banner)
        )));
    }
    stream.write_all(VERSION)?;
    let count = read_n(stream, 1)?[0];
    let types = read_n(stream, usize::from(count))?;
    if !types.contains(&TYPE_30) {
        return Err(refused(format!("no type-30 login offered ({types:?})")));
    }
    stream.write_all(&[TYPE_30])?;
    let header = read_n(stream, 4)?;
    let generator = u32::from(be16(&header));
    let key_len = usize::from(be16(&header[2..]));
    if !(64..=1024).contains(&key_len) {
        return Err(refused(format!("a {key_len}-byte key")));
    }
    let prime = read_n(stream, key_len)?;
    let peer = read_n(stream, key_len)?;
    let pair = KeyPair::new(generator, &prime)?;
    let mut secret = pair.shared(&peer)?;
    let key = crypto::md5(&secret);
    secret.fill(0);
    let mut block = credentials(user, password)?;
    let encrypted = crypto::aes_ecb(true, &key, &block)?;
    block.fill(0);
    let mut answer = encrypted.clone();
    answer.extend_from_slice(&pair.public);
    stream.write_all(&answer)?;
    let result = read_n(stream, 4)?;
    if be32(&result) != 0 {
        return Err(refused(format!(
            "the login was refused ({}): the password, or the account not in Screen Sharing's list",
            be32(&result)
        )));
    }
    // Shared, Apple's extended ServerInit, session selection.
    stream.write_all(&[0xC1])?;
    let init = read_n(stream, 24)?;
    let size = Size {
        width: u32::from(be16(&init)),
        height: u32::from(be16(&init[2..])),
    };
    let name_len = be32(&init[20..]) as usize;
    let name = read_n(stream, name_len)?;
    let flags = if name.len() >= 6 { be32(&name[2..]) } else { 0 };
    if flags & 0x04 == 0 {
        return Err(refused("the server offers no session selection"));
    }
    let info_len = usize::from(be16(&read_n(stream, 2)?));
    let info = read_n(stream, info_len)?;
    if info.len() < 6 || be32(&info[2..]) & 0x04 == 0 {
        return Err(refused("a session of its own isn't permitted"));
    }
    stream.write_all(&session_select(&encrypted))?;
    let result_len = usize::from(be16(&read_n(stream, 2)?));
    let result = read_n(stream, result_len)?;
    if result.len() < 3 || result[2] != 0 {
        return Err(refused(format!(
            "the session was refused ({})",
            result.get(2).copied().unwrap_or(255)
        )));
    }
    Ok(Server { size, flags })
}

/// Session select: length 200, version 2, command 2 (connect to virtual display), the
/// client's name, then the encrypted credentials again.
pub fn session_select(encrypted: &[u8]) -> Vec<u8> {
    let mut message = vec![0u8; 74];
    message[1] = 200;
    message[3] = 2;
    message[8] = 2;
    message[10..14].copy_from_slice(b"Koyo");
    message.extend_from_slice(encrypted);
    message
}

/// `SetPixelFormat`: 32 bits per pixel, depth 24, little-endian true colour.
pub fn pixel_format() -> [u8; 20] {
    let mut message = [0u8; 20];
    message[4] = 32;
    message[5] = 24;
    message[7] = 1;
    for at in [8, 10, 12] {
        message[at..at + 2].copy_from_slice(&255u16.to_be_bytes());
    }
    message[14] = 16;
    message[15] = 8;
    message
}

/// `SetEncodings` (zlib, raw, copy-rect, Apple's display info and layout), then `SetMode`
/// (control).
pub fn encodings_and_mode() -> Vec<u8> {
    let encodings: [i32; 5] = [6, 0, 1, 0x44d, 0x451];
    let mut message = vec![2, 0];
    message.extend_from_slice(&5u16.to_be_bytes());
    for encoding in encodings {
        message.extend_from_slice(&encoding.to_be_bytes());
    }
    message.extend_from_slice(&[0x0a, 0, 0, 1]);
    message
}

/// The presets that follow the asked-for mode, left out when one has its pixel size: the Mac
/// doesn't answer a list with the same pixel size twice.
const PRESETS: [[u32; 4]; 4] = [
    [2880, 1800, 1440, 900],
    [3840, 2160, 1920, 1080],
    [2880, 1620, 1440, 810],
    [2624, 1696, 1312, 848],
];

/// `SetDisplayConfiguration` for one dynamic display of `size` points at 1x.
pub fn display_configuration(size: Size) -> Vec<u8> {
    let (width, height) = (size.width, size.height);
    let mut modes = vec![[width, height, width, height]];
    modes.extend(
        PRESETS
            .iter()
            .filter(|mode| !(mode[0] == width && mode[1] == height)),
    );
    let descriptor_len = 0x9c + modes.len() * 0x1c;
    let body_len = 8 + descriptor_len;
    let mut message = vec![0u8; 4 + body_len];
    message[0] = 0x1d;
    message[2..4].copy_from_slice(&u16::try_from(body_len).unwrap_or(u16::MAX).to_be_bytes());
    let body = &mut message[4..];
    body[0..2].copy_from_slice(&1u16.to_be_bytes());
    body[2..4].copy_from_slice(&1u16.to_be_bytes());
    let descriptor = &mut body[8..];
    descriptor[0..2].copy_from_slice(
        &u16::try_from(descriptor_len)
            .unwrap_or(u16::MAX)
            .to_be_bytes(),
    );
    let name = b"Koyo Agent Display 1";
    descriptor[2..2 + name.len()].copy_from_slice(name);
    // The declared maximum is a ceiling, never below what's asked for.
    let (max_width, max_height) = (width.max(3840), height.max(2160));
    let q = &mut descriptor[0x7a..];
    q[0..4].copy_from_slice(&1u32.to_be_bytes());
    // Millimetres at 264 dpi, worked out in double and sent as float, as Apple's viewer does.
    let millimetres = |pixels: u32| ((f64::from(pixels) * 25.4 / 264.) as f32).to_bits();
    q[8..12].copy_from_slice(&millimetres(max_width).to_be_bytes());
    q[12..16].copy_from_slice(&millimetres(max_height).to_be_bytes());
    q[16..20].copy_from_slice(&max_width.to_be_bytes());
    q[20..24].copy_from_slice(&max_height.to_be_bytes());
    q[28..32].copy_from_slice(&7u32.to_be_bytes());
    q[32..34].copy_from_slice(&u16::try_from(modes.len()).unwrap_or(0).to_be_bytes());
    for (index, mode) in modes.iter().enumerate() {
        let at = 0x9c + index * 0x1c;
        let entry = &mut descriptor[at..at + 0x1c];
        for (field, value) in mode.iter().enumerate() {
            entry[field * 4..field * 4 + 4].copy_from_slice(&value.to_be_bytes());
        }
        entry[16..24].copy_from_slice(&60f64.to_bits().to_be_bytes());
    }
    message
}

/// `FramebufferUpdateRequest` for the whole screen.
pub fn update_request(incremental: bool, size: Size) -> [u8; 10] {
    let mut message = [3, u8::from(incremental), 0, 0, 0, 0, 0, 0, 0, 0];
    message[6..8].copy_from_slice(&u16::try_from(size.width).unwrap_or(u16::MAX).to_be_bytes());
    message[8..10].copy_from_slice(&u16::try_from(size.height).unwrap_or(u16::MAX).to_be_bytes());
    message
}

/// `AutoFrameBufferUpdate` (`0x09`): the server pushes updates for the whole screen; a layout
/// change drops the arming.
pub fn auto_updates(size: Size) -> [u8; 16] {
    let mut message = [0u8; 16];
    message[0] = 0x09;
    message[2..4].copy_from_slice(&1u16.to_be_bytes());
    message[12..14].copy_from_slice(&u16::try_from(size.width).unwrap_or(u16::MAX).to_be_bytes());
    message[14..16].copy_from_slice(&u16::try_from(size.height).unwrap_or(u16::MAX).to_be_bytes());
    message
}

/// Reads one server message through, pixels and all: the screen's size when it was an update
/// that carried Apple's layout.
pub fn read_message(stream: &mut impl Read, screen: Size) -> io::Result<Option<Size>> {
    let kind = read_n(stream, 1)?[0];
    match kind {
        0 => {
            let header = read_n(stream, 3)?;
            let rects = be16(&header[1..]);
            let mut layout = None;
            for _ in 0..rects {
                let rect = read_n(stream, 12)?;
                let (width, height) =
                    (usize::from(be16(&rect[4..])), usize::from(be16(&rect[6..])));
                match be32(&rect[8..]) as i32 {
                    0x451 => {
                        let len = usize::from(be16(&read_n(stream, 2)?));
                        let body = read_n(stream, len)?;
                        if body.len() >= 0x14 {
                            layout = Some(Size {
                                width: u32::from(be16(&body[6..])),
                                height: u32::from(be16(&body[8..])),
                            });
                        }
                    }
                    0x44d => {
                        let head = read_n(stream, 10)?;
                        read_n(stream, usize::from(be16(&head[8..])) * 0x1c)?;
                    }
                    -223 => {
                        layout = Some(Size {
                            width: width as u32,
                            height: height as u32,
                        });
                    }
                    0 => {
                        io::copy(
                            &mut stream.by_ref().take((width * height * 4) as u64),
                            &mut io::sink(),
                        )?;
                    }
                    6 => {
                        let len = be32(&read_n(stream, 4)?);
                        io::copy(&mut stream.by_ref().take(u64::from(len)), &mut io::sink())?;
                    }
                    1 => {
                        read_n(stream, 4)?;
                    }
                    other => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "an encoding the holder never asked for: {other} ({width}×{height}, screen {}×{})",
                                screen.width, screen.height
                            ),
                        ));
                    }
                }
            }
            Ok(layout)
        }
        1 => {
            let header = read_n(stream, 5)?;
            read_n(stream, usize::from(be16(&header[3..])) * 6)?;
            Ok(None)
        }
        2 => Ok(None),
        3 => {
            let header = read_n(stream, 7)?;
            io::copy(
                &mut stream.by_ref().take(u64::from(be32(&header[3..]))),
                &mut io::sink(),
            )?;
            Ok(None)
        }
        0x14 => {
            read_n(stream, 7)?;
            Ok(None)
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("an unknown server message {other:#04x}"),
        )),
    }
}

#[cfg(test)]
mod tests;
