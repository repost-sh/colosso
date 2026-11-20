//! The engine's framework, as it says of itself: whether it's the CEF this host was built
//! against, and what `script/cef` built it with, in its `Resources/koyo-build.txt`
//! (`script/cef-framework`): the codec switches of its build and Koyo's patches. Both are
//! asked once the framework is loaded and before CEF starts, off the main thread where the
//! host runs in the app's process; the host then asks for nothing a patch it lacks would add,
//! and tells the app what it has.

use std::ffi::CStr;
use std::path::Path;
use std::sync::OnceLock;

use koyo_browser_ipc::Framework;

use crate::blocking::Blocking;

/// What the framework says it was built with, beside its resources.
const MANIFEST: &str = "Resources/koyo-build.txt";

/// What the framework the engine runs on was built with: read once, as CEF starts once in a
/// process.
static BUILT: OnceLock<Framework> = OnceLock::new();

/// Configures the CEF API version this host was built for, and checks that the framework
/// loaded is the CEF it was built against; says why not. The framework must be loaded.
pub(crate) fn check() -> Result<(), String> {
    agrees(api_hash(0).as_deref(), api_hash(2).as_deref(), &headers())
}

/// Reads what the framework at `framework` was built with, for the engine's life.
pub(crate) fn read(_: Blocking<'_>, framework: &Path) {
    let path = framework.join(MANIFEST);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            log::warn!("{}: {error}", path.display());
            String::new()
        }
    };
    let _ = BUILT.set(parse(&text));
}

/// What the engine's framework was built with: nothing until it's [`read`].
pub(crate) fn built() -> Framework {
    BUILT.get().cloned().unwrap_or_default()
}

/// Whether the engine's framework was built with Koyo's patch `patch` (`script/cef-patches`).
pub(crate) fn patched(patch: &str) -> bool {
    BUILT
        .get()
        .is_some_and(|built| built.patches.iter().any(|name| name == patch))
}

/// What a manifest says: H.264 and AAC where both codec switches are on, and every patch it
/// lists. One line each, as `script/cef-framework manifest` writes them.
fn parse(text: &str) -> Framework {
    let (mut proprietary, mut branded) = (false, false);
    let mut patches = Vec::new();
    for line in text.lines() {
        match line.split_whitespace().collect::<Vec<_>>()[..] {
            ["proprietary_codecs=true"] => proprietary = true,
            ["ffmpeg_branding=\"Chrome\""] => branded = true,
            ["patch", name, _hash] => patches.push(name.to_owned()),
            _ => {}
        }
    }
    Framework {
        codecs: proprietary && branded,
        patches,
    }
}

/// The hash the loaded framework gives for `entry` of the API version this host was built for
/// (`cef_api_hash.h`): 0 that API's, 2 the framework's commit. None where it doesn't know the
/// version.
fn api_hash(entry: i32) -> Option<String> {
    let hash = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, entry);
    if hash.is_null() {
        return None;
    }
    // SAFETY: not null, a zero-terminated string CEF keeps for the process's life.
    let hash = unsafe { CStr::from_ptr(hash) };
    Some(hash.to_string_lossy().into_owned())
}

/// The CEF this host was built against, as its headers name it:
/// `156.0.14+g4fa4ab5+chromium-156.0.8078.48`.
fn headers() -> String {
    String::from_utf8_lossy(cef::sys::CEF_VERSION)
        .trim_end_matches('\0')
        .to_owned()
}

/// Whether a framework that gives `api` for the host's API version, and `commit` for its own,
/// is the CEF of `headers`: one that knows that API version, built at the commit `headers`
/// names after its `+g`.
fn agrees(api: Option<&str>, commit: Option<&str>, headers: &str) -> Result<(), String> {
    let again = "Package the engine again: script/browser-engine.";
    if api.is_none() {
        return Err(format!(
            "The browser engine's framework doesn't know the CEF API this Koyo was built for, \
             CEF {headers}'s. {again}"
        ));
    }
    let expected = (headers.split('+')).find_map(|part| part.strip_prefix('g'));
    match (expected, commit) {
        (Some(expected), Some(commit)) if !expected.is_empty() && commit.starts_with(expected) => {
            Ok(())
        }
        (_, commit) => Err(format!(
            "The browser engine's framework is CEF at commit {}, and this Koyo was built for CEF \
             {headers}. {again}",
            commit.unwrap_or("unknown")
        )),
    }
}

#[cfg(test)]
mod tests;
