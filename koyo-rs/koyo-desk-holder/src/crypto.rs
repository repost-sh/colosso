//! What Apple's security type 30 needs (`docs/agent-desktop.md` 6, after Rocuronium and
//! `poc/src/rfbpoc.m`): Diffie-Hellman through Security's `SecDH…` functions, which it exports
//! without declaring them (looked up at run time, so a macOS without them is an error, not a
//! crash), and `CommonCrypto`'s MD5 and AES-128 in ECB mode, which `libSystem` carries.

use std::ffi::{CStr, c_void};
use std::io;
use std::sync::OnceLock;

type Context = *mut c_void;
type Create =
    unsafe extern "C" fn(u32, *const u8, usize, u32, *const u8, usize, *mut Context) -> i32;
type Generate = unsafe extern "C" fn(Context, *mut u8, *mut usize) -> i32;
type Compute = unsafe extern "C" fn(Context, *const u8, usize, *mut u8, *mut usize) -> i32;
type Destroy = unsafe extern "C" fn(Context);

unsafe extern "C" {
    fn CC_MD5(data: *const c_void, len: u32, md: *mut u8) -> *mut u8;
    fn CCCrypt(
        op: u32,
        alg: u32,
        options: u32,
        key: *const c_void,
        key_length: usize,
        iv: *const c_void,
        data_in: *const c_void,
        data_in_length: usize,
        data_out: *mut c_void,
        data_out_available: usize,
        data_out_moved: *mut usize,
    ) -> i32;
}

const ENCRYPT: u32 = 0;
const DECRYPT: u32 = 1;
const AES: u32 = 0;
const ECB: u32 = 2;

struct Functions {
    create: Create,
    generate: Generate,
    compute: Compute,
    destroy: Destroy,
}

fn functions() -> io::Result<&'static Functions> {
    static FUNCTIONS: OnceLock<Option<Functions>> = OnceLock::new();
    FUNCTIONS
        .get_or_init(|| {
            let path = c"/System/Library/Frameworks/Security.framework/Security";
            // SAFETY: a system framework, opened for its exported symbols.
            let library = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW) };
            if library.is_null() {
                return None;
            }
            let find = |name: &CStr| {
                // SAFETY: an open library and a symbol name.
                let symbol = unsafe { libc::dlsym(library, name.as_ptr()) };
                (!symbol.is_null()).then_some(symbol)
            };
            // SAFETY: each symbol is Security's function of the signature given above, as
            // Rocuronium and the PoC call them.
            unsafe {
                Some(Functions {
                    create: std::mem::transmute::<*mut c_void, Create>(find(c"SecDHCreate")?),
                    generate: std::mem::transmute::<*mut c_void, Generate>(find(
                        c"SecDHGenerateKeypair",
                    )?),
                    compute: std::mem::transmute::<*mut c_void, Compute>(find(c"SecDHComputeKey")?),
                    destroy: std::mem::transmute::<*mut c_void, Destroy>(find(c"SecDHDestroy")?),
                })
            }
        })
        .as_ref()
        .ok_or_else(|| {
            io::Error::other("this macOS has no SecDH functions for Screen Sharing's login")
        })
}

/// One side of a Diffie-Hellman exchange over the group `generator`, `prime`.
pub struct KeyPair {
    context: Context,
    /// Our public key, left-padded with zeros to the prime's length.
    pub public: Vec<u8>,
    len: usize,
}

impl KeyPair {
    pub fn new(generator: u32, prime: &[u8]) -> io::Result<KeyPair> {
        let functions = functions()?;
        let mut context: Context = std::ptr::null_mut();
        // SAFETY: the prime's bytes and length, no recipient key, a valid out-pointer.
        let made = unsafe {
            (functions.create)(
                generator,
                prime.as_ptr(),
                prime.len(),
                0,
                std::ptr::null(),
                0,
                &raw mut context,
            )
        };
        if made != 0 || context.is_null() {
            return Err(io::Error::other(format!("SecDHCreate said {made}")));
        }
        let mut public = vec![0u8; prime.len()];
        let mut written = public.len();
        // SAFETY: a context made above, a buffer of `written` bytes.
        let generated =
            unsafe { (functions.generate)(context, public.as_mut_ptr(), &raw mut written) };
        if generated != 0 {
            // SAFETY: made above and not used after.
            unsafe { (functions.destroy)(context) };
            return Err(io::Error::other(format!(
                "SecDHGenerateKeypair said {generated}"
            )));
        }
        Ok(KeyPair {
            context,
            public: padded(&public[..written], prime.len()),
            len: prime.len(),
        })
    }

    /// The shared secret with `peer`'s public key, left-padded to the prime's length.
    pub fn shared(&self, peer: &[u8]) -> io::Result<Vec<u8>> {
        let functions = functions()?;
        let mut secret = vec![0u8; self.len];
        let mut written = secret.len();
        // SAFETY: the context is ours; both buffers are of the lengths given.
        let computed = unsafe {
            (functions.compute)(
                self.context,
                peer.as_ptr(),
                peer.len(),
                secret.as_mut_ptr(),
                &raw mut written,
            )
        };
        if computed != 0 {
            return Err(io::Error::other(format!("SecDHComputeKey said {computed}")));
        }
        let shared = padded(&secret[..written], self.len);
        secret.fill(0);
        Ok(shared)
    }
}

impl Drop for KeyPair {
    fn drop(&mut self) {
        if let Ok(functions) = functions() {
            // SAFETY: made by `new`, destroyed once.
            unsafe { (functions.destroy)(self.context) };
        }
    }
}

fn padded(bytes: &[u8], len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len.saturating_sub(bytes.len())];
    out.extend_from_slice(bytes);
    out
}

pub fn md5(data: &[u8]) -> [u8; 16] {
    let mut digest = [0u8; 16];
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    // SAFETY: `data` of `len` bytes in, 16 bytes out.
    unsafe { CC_MD5(data.as_ptr().cast(), len, digest.as_mut_ptr()) };
    digest
}

/// AES-128 in ECB mode, no padding: `data` a whole number of 16-byte blocks.
pub fn aes_ecb(encrypt: bool, key: &[u8; 16], data: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = vec![0u8; data.len()];
    let mut moved = 0;
    // SAFETY: key, input and output of the lengths given; ECB takes no IV.
    let status = unsafe {
        CCCrypt(
            if encrypt { ENCRYPT } else { DECRYPT },
            AES,
            ECB,
            key.as_ptr().cast(),
            key.len(),
            std::ptr::null(),
            data.as_ptr().cast(),
            data.len(),
            out.as_mut_ptr().cast(),
            out.len(),
            &raw mut moved,
        )
    };
    if status != 0 || moved != data.len() {
        return Err(io::Error::other(format!("CCCrypt said {status}")));
    }
    Ok(out)
}

/// `n` random bytes from the kernel's generator.
pub fn random(n: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; n];
    // SAFETY: a buffer of its stated length.
    unsafe { libc::arc4random_buf(bytes.as_mut_ptr().cast(), bytes.len()) };
    bytes
}
