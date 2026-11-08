//! What a new passkey's attestation object holds: its authenticator data, and in that the new
//! public key, which a page reads as DER `SubjectPublicKeyInfo` (`getPublicKey()`) next to the
//! key's COSE algorithm (`getPublicKeyAlgorithm()`). The object is CBOR (`WebAuthn`'s
//! "Attestation Object"); anything malformed reads as nothing.

use cbor::{Item, Reader};

mod cbor;
#[cfg(test)]
pub(super) mod tests;

/// Where the authenticator data's flags are: after the relying party's id hash.
const FLAGS: usize = 32;
/// The flag for attested credential data: a new passkey's id and key.
const ATTESTED: u8 = 0x40;
/// Where the attested credential data starts: after the flags and the signature counter.
const ATTESTED_AT: usize = FLAGS + 1 + 4;
/// The authenticator's model id, at the start of the attested credential data.
const AAGUID: usize = 16;

/// COSE key types (RFC 9053).
const OKP: i64 = 1;
const EC2: i64 = 2;
const RSA: i64 = 3;
/// COSE curves: NIST P-256, and Ed25519 for `EdDSA`.
const P256: i64 = 1;
const ED25519: i64 = 6;

/// `SubjectPublicKeyInfo` up to a P-256 key's uncompressed point: `id-ecPublicKey` on
/// `prime256v1`, in a bit string of 66 bytes.
const P256_SPKI: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a,
    0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];
/// `SubjectPublicKeyInfo` up to an Ed25519 key: `id-Ed25519`, in a bit string of 33 bytes.
const ED25519_SPKI: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];
/// The `AlgorithmIdentifier` of an RSA key: `rsaEncryption`, with no parameters.
const RSA_ALGORITHM: [u8; 15] = [
    0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00,
];

/// What the page is told of a new passkey from its attestation object.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Attested {
    /// The authenticator data, as the attestation object holds it.
    pub authenticator_data: Vec<u8>,
    /// The new key as DER `SubjectPublicKeyInfo`; none for a key type Koyo doesn't know.
    pub public_key: Option<Vec<u8>>,
    /// The new key's COSE algorithm.
    pub algorithm: i64,
}

/// What `attestation_object` says of the new passkey; none when it's malformed or holds no
/// new key.
pub(super) fn read(attestation_object: &[u8]) -> Option<Attested> {
    let authenticator_data = authenticator_data(attestation_object)?;
    let key = CoseKey::read(attested_key(authenticator_data)?)?;
    Some(Attested {
        authenticator_data: authenticator_data.to_vec(),
        public_key: key.subject_public_key_info(),
        algorithm: key.algorithm,
    })
}

/// The attestation object's `authData`.
fn authenticator_data(attestation_object: &[u8]) -> Option<&[u8]> {
    let mut reader = Reader::new(attestation_object);
    let Item::Map(entries) = reader.item()? else {
        return None;
    };
    for _ in 0..entries {
        let key = reader.whole()?;
        let value = reader.whole()?;
        if key == Item::Text(b"authData") {
            return match value {
                Item::Bytes(data) => Some(data),
                _ => None,
            };
        }
    }
    None
}

/// The COSE key in `authenticator_data`'s attested credential data, and what follows it.
fn attested_key(authenticator_data: &[u8]) -> Option<&[u8]> {
    let flags = *authenticator_data.get(FLAGS)?;
    if flags & ATTESTED == 0 {
        return None;
    }
    let attested = authenticator_data.get(ATTESTED_AT..)?;
    let (_aaguid, rest) = attested.split_at_checked(AAGUID)?;
    let (length, rest) = rest.split_at_checked(2)?;
    let length = u16::from_be_bytes(length.try_into().ok()?);
    let (_id, key) = rest.split_at_checked(usize::from(length))?;
    Some(key)
}

/// A COSE key (RFC 9052), as far as its public key goes.
#[derive(Debug, Default)]
struct CoseKey<'a> {
    kind: Option<i64>,
    algorithm: i64,
    /// Label -1: an EC2 or OKP key's curve.
    curve: Option<i64>,
    /// Label -1: an RSA key's modulus.
    modulus: Option<&'a [u8]>,
    /// Label -2: an EC2 or OKP key's x, an RSA key's exponent.
    x: Option<&'a [u8]>,
    /// Label -3: an EC2 key's y.
    y: Option<&'a [u8]>,
}

impl<'a> CoseKey<'a> {
    /// The key at the start of `data`; none without an algorithm.
    fn read(data: &'a [u8]) -> Option<Self> {
        let mut reader = Reader::new(data);
        let Item::Map(entries) = reader.item()? else {
            return None;
        };
        let mut key = CoseKey::default();
        let mut algorithm = None;
        for _ in 0..entries {
            let label = reader.whole()?;
            let value = reader.whole()?;
            match (label, value) {
                (Item::Int(1), Item::Int(kind)) => key.kind = Some(kind),
                (Item::Int(3), Item::Int(alg)) => algorithm = Some(alg),
                (Item::Int(-1), Item::Int(curve)) => key.curve = Some(curve),
                (Item::Int(-1), Item::Bytes(modulus)) => key.modulus = Some(modulus),
                (Item::Int(-2), Item::Bytes(x)) => key.x = Some(x),
                (Item::Int(-3), Item::Bytes(y)) => key.y = Some(y),
                _ => {}
            }
        }
        key.algorithm = algorithm?;
        Some(key)
    }

    /// The key as DER `SubjectPublicKeyInfo`, for the key types a page's `getPublicKey()`
    /// knows: P-256, Ed25519 and RSA.
    fn subject_public_key_info(&self) -> Option<Vec<u8>> {
        match (self.kind?, self.curve) {
            (EC2, Some(P256)) => {
                let (x, y) = (self.x?, self.y?);
                if x.len() != 32 || y.len() != 32 {
                    return None;
                }
                Some([&P256_SPKI[..], &[0x04], x, y].concat())
            }
            (OKP, Some(ED25519)) => {
                let x = self.x?;
                (x.len() == 32).then(|| [&ED25519_SPKI[..], x].concat())
            }
            (RSA, _) => rsa_spki(self.modulus?, self.x?),
            _ => None,
        }
    }
}

/// An RSA key's `SubjectPublicKeyInfo`: its `RSAPublicKey` (modulus and exponent) in a bit
/// string under `rsaEncryption`.
fn rsa_spki(modulus: &[u8], exponent: &[u8]) -> Option<Vec<u8>> {
    if modulus.is_empty() || exponent.is_empty() {
        return None;
    }
    let key = der(0x30, &[integer(modulus), integer(exponent)].concat());
    let bits = der(0x03, &[&[0x00][..], &key].concat());
    Some(der(0x30, &[&RSA_ALGORITHM[..], &bits].concat()))
}

/// A DER `INTEGER` of the unsigned big-endian `value`: its leading zeros dropped, and one put
/// back when the first bit is set, so it doesn't read as negative.
fn integer(value: &[u8]) -> Vec<u8> {
    let start = value
        .iter()
        .position(|&byte| byte != 0)
        .unwrap_or(value.len());
    let value = &value[start..];
    match value.first() {
        None => der(0x02, &[0x00]),
        Some(first) if first & 0x80 != 0 => der(0x02, &[&[0x00][..], value].concat()),
        Some(_) => der(0x02, value),
    }
}

/// A DER item of `tag` holding `content`, its length short below 128 bytes and long above.
fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut item = vec![tag];
    let length = content.len();
    if length < 0x80 {
        item.push(length as u8);
    } else {
        let bytes = length.to_be_bytes();
        let start = bytes
            .iter()
            .position(|&byte| byte != 0)
            .unwrap_or(bytes.len());
        item.push(0x80 | (bytes.len() - start) as u8);
        item.extend_from_slice(&bytes[start..]);
    }
    item.extend_from_slice(content);
    item
}
