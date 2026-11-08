//! Attestation objects built by hand, as authenticators write them, and broken ones.

use super::{Attested, read};

/// A CBOR head of major type `major` and argument `n`, in its shortest form.
fn head(major: u8, n: u64) -> Vec<u8> {
    let major = major << 5;
    match n {
        0..=23 => vec![major | n as u8],
        24..=0xff => vec![major | 0x18, n as u8],
        0x100..=0xffff => [&[major | 0x19][..], &(n as u16).to_be_bytes()].concat(),
        0x1_0000..=0xffff_ffff => [&[major | 0x1a][..], &(n as u32).to_be_bytes()].concat(),
        _ => [&[major | 0x1b][..], &n.to_be_bytes()].concat(),
    }
}

fn int(value: i64) -> Vec<u8> {
    if value >= 0 {
        head(0, value as u64)
    } else {
        head(1, (-1 - value) as u64)
    }
}

fn bytes(value: &[u8]) -> Vec<u8> {
    [head(2, value.len() as u64), value.to_vec()].concat()
}

fn text(value: &str) -> Vec<u8> {
    [head(3, value.len() as u64), value.as_bytes().to_vec()].concat()
}

fn map(entries: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut out = head(5, entries.len() as u64);
    for (key, value) in entries {
        out.extend_from_slice(key);
        out.extend_from_slice(value);
    }
    out
}

/// Authenticator data with attested credential data holding `key`: user present and
/// verified, a 16-byte credential id.
fn authenticator_data(key: &[u8]) -> Vec<u8> {
    let mut data = vec![0x11; 32];
    data.push(0x45);
    data.extend_from_slice(&[0, 0, 0, 7]);
    data.extend_from_slice(&[0x22; 16]);
    data.extend_from_slice(&16u16.to_be_bytes());
    data.extend_from_slice(&[0xab; 16]);
    data.extend_from_slice(key);
    data
}

/// A `none` attestation object holding `authenticator_data`.
fn attestation_object(authenticator_data: &[u8]) -> Vec<u8> {
    map(&[
        (text("fmt"), text("none")),
        (text("attStmt"), map(&[])),
        (text("authData"), bytes(authenticator_data)),
    ])
}

fn p256_key(x: &[u8], y: &[u8]) -> Vec<u8> {
    map(&[
        (int(1), int(2)),
        (int(3), int(-7)),
        (int(-1), int(1)),
        (int(-2), bytes(x)),
        (int(-3), bytes(y)),
    ])
}

/// A `none` attestation object for a P-256 key, x all ones and y all twos, as other tests
/// of passkeys take one.
pub(in crate::passkeys) fn p256_attestation_object() -> Vec<u8> {
    attestation_object(&authenticator_data(&p256_key(&[1; 32], &[2; 32])))
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

#[test]
fn a_p256_key_reads_as_its_uncompressed_point_under_id_ec_public_key() {
    let (x, y) = ([0x01; 32], [0x02; 32]);
    let data = authenticator_data(&p256_key(&x, &y));
    let attested = read(&attestation_object(&data)).unwrap();
    let spki = [
        hex("3059301306072a8648ce3d020106082a8648ce3d03010703420004"),
        x.to_vec(),
        y.to_vec(),
    ]
    .concat();
    assert_eq!(
        attested,
        Attested {
            authenticator_data: data,
            public_key: Some(spki),
            algorithm: -7,
        }
    );
}

#[test]
fn an_ed25519_key_reads_under_id_ed25519() {
    let x = [0x5a; 32];
    let key = map(&[
        (int(1), int(1)),
        (int(3), int(-8)),
        (int(-1), int(6)),
        (int(-2), bytes(&x)),
    ]);
    let attested = read(&attestation_object(&authenticator_data(&key))).unwrap();
    assert_eq!(
        attested.public_key,
        Some([hex("302a300506032b6570032100"), x.to_vec()].concat())
    );
    assert_eq!(attested.algorithm, -8);
}

#[test]
fn an_rsa_key_reads_as_its_modulus_and_exponent_under_rsa_encryption() {
    // A 2048-bit modulus with its first bit set takes a zero before it, as DER integers do.
    let modulus = [&[0xc3][..], &[0x99; 255]].concat();
    let key = map(&[
        (int(1), int(3)),
        (int(3), int(-257)),
        (int(-1), bytes(&modulus)),
        (int(-2), bytes(&[0x01, 0x00, 0x01])),
    ]);
    let attested = read(&attestation_object(&authenticator_data(&key))).unwrap();
    let spki = [
        hex("30820122300d06092a864886f70d01010105000382010f003082010a0282010100"),
        modulus,
        hex("0203010001"),
    ]
    .concat();
    assert_eq!(attested.public_key, Some(spki));
    assert_eq!(attested.algorithm, -257);
}

#[test]
fn a_key_type_koyo_doesnt_know_still_tells_its_algorithm() {
    let key = map(&[
        (int(1), int(4)),
        (int(3), int(5)),
        (int(-1), bytes(&[9; 16])),
    ]);
    let attested = read(&attestation_object(&authenticator_data(&key))).unwrap();
    assert_eq!(attested.public_key, None);
    assert_eq!(attested.algorithm, 5);
    // A P-256 key of the wrong size, likewise.
    let key = p256_key(&[1; 31], &[2; 32]);
    let attested = read(&attestation_object(&authenticator_data(&key))).unwrap();
    assert_eq!((attested.public_key, attested.algorithm), (None, -7));
}

#[test]
fn entries_in_any_order_and_a_full_attestation_statement_are_read_past() {
    let data = authenticator_data(&p256_key(&[3; 32], &[4; 32]));
    let statement = map(&[
        (text("alg"), int(-7)),
        (text("sig"), bytes(&[0x30; 70])),
        // x5c: an array of certificates, under a tag for good measure.
        (
            text("x5c"),
            [head(4, 2), head(6, 24), bytes(&[1; 300]), bytes(&[2; 3])].concat(),
        ),
    ]);
    let object = map(&[
        (text("authData"), bytes(&data)),
        (text("attStmt"), statement.clone()),
        (text("fmt"), text("packed")),
    ]);
    assert_eq!(read(&object).unwrap().authenticator_data, data);
    let object = map(&[
        (text("fmt"), text("packed")),
        (text("attStmt"), statement),
        (int(-4), [head(7, 22)].concat()),
        (text("authData"), bytes(&data)),
    ]);
    assert_eq!(read(&object).unwrap().authenticator_data, data);
}

#[test]
fn authenticator_data_without_a_new_key_reads_as_nothing() {
    let mut data = authenticator_data(&p256_key(&[1; 32], &[2; 32]));
    data[32] &= !0x40;
    assert_eq!(read(&attestation_object(&data)), None);
    // A credential id longer than the data.
    let mut data = authenticator_data(&p256_key(&[1; 32], &[2; 32]));
    data[53..55].copy_from_slice(&0xffffu16.to_be_bytes());
    assert_eq!(read(&attestation_object(&data)), None);
    // A key with no algorithm.
    let key = map(&[(int(1), int(2)), (int(-1), int(1))]);
    assert_eq!(read(&attestation_object(&authenticator_data(&key))), None);
    // `authData` that isn't bytes, and an object with none.
    assert_eq!(read(&map(&[(text("authData"), text("no"))])), None);
    assert_eq!(read(&map(&[(text("fmt"), text("none"))])), None);
}

#[test]
fn every_truncation_of_an_attestation_object_reads_as_nothing() {
    let object = attestation_object(&authenticator_data(&p256_key(&[1; 32], &[2; 32])));
    assert!(read(&object).is_some());
    for end in 0..object.len() {
        assert_eq!(read(&object[..end]), None, "cut at {end}");
    }
}

#[test]
fn malformed_cbor_reads_as_nothing_without_panicking() {
    let key = p256_key(&[1; 32], &[2; 32]);
    let data = authenticator_data(&key);
    // Not a map; an indefinite-length map; a reserved length.
    assert_eq!(read(&bytes(&data)), None);
    assert_eq!(
        read(&[&[0xbf][..], &text("authData"), &bytes(&data), &[0xff]].concat()),
        None
    );
    assert_eq!(read(&[0xbc]), None);
    // Lengths and counts far past the end, which mustn't be taken at their word.
    assert_eq!(
        read(&[&[0xa1][..], &text("authData"), &head(2, u64::MAX)].concat()),
        None
    );
    assert_eq!(read(&head(5, u64::MAX)), None);
    assert_eq!(
        read(&[&[0xa1][..], &text("x"), &head(4, 1 << 40)].concat()),
        None
    );
    // Arrays nested deeper than any attestation object goes.
    let deep = [vec![0x81; 10_000], vec![0x00]].concat();
    let object = map(&[(text("attStmt"), deep), (text("authData"), bytes(&data))]);
    assert_eq!(read(&object), None);
    // A key that isn't a map, and one cut short.
    let mut short = authenticator_data(&key[..key.len() - 1]);
    assert_eq!(read(&attestation_object(&short)), None);
    short.truncate(55 + 16);
    short.push(0x80);
    assert_eq!(read(&attestation_object(&short)), None);
}

#[test]
fn arbitrary_bytes_never_panic() {
    // A fixed linear congruential sequence: the same inputs on every run.
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as u8
    };
    let valid = attestation_object(&authenticator_data(&p256_key(&[1; 32], &[2; 32])));
    for round in 0..2_000 {
        let mut input = valid.clone();
        for _ in 0..=round % 8 {
            let at = usize::from(next()) * input.len() / 256;
            input[at] = next();
        }
        let _ = read(&input);
        let noise: Vec<u8> = (0..round % 97).map(|_| next()).collect();
        let _ = read(&noise);
    }
}
