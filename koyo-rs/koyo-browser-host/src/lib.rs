//! Koyo's browser host.
//!
//! A library and the binary made of it. As a library it runs inside Koyo's own process
//! ([`embedded`]): the browser process is Koyo, and a page is a Chrome-style browser whose own
//! views Koyo shows in its window ([`native`], `docs/native-pages.md`). The app talks to it
//! over a socket (`koyo-browser-ipc`).
//!
//! The binary is what CEF starts with `--type=…` from inside a helper bundle: a renderer, GPU
//! or utility process, which only runs CEF's subprocess entry point. Started without, it is a
//! browser process of its own, speaking the same protocol, its pages in holders nothing
//! shows: the engine's tests run it so.
#![expect(
    clippy::transmute_ptr_to_ptr,
    reason = "cef's wrap_* macros transmute references in what they expand to"
)]
mod app;
mod devtools;
mod dialogs;
pub mod embedded;
mod host;
mod location;
mod mac_app;
mod menus;
pub mod native;
mod pane;
mod passkeys;
mod pending;

use std::process::ExitCode;

/// Whether this engine was packaged without a release signature: its
/// Info.plist says so, and a signed bundle's Info.plist is sealed. Inside Koyo's process the
/// engine's bundle isn't the main one.
pub(crate) fn mock_keychain() -> bool {
    use objc2_foundation::{NSBundle, NSNumber, NSString};
    let bundle = match embedded::engine() {
        Some(engine) => NSBundle::bundleWithPath(&NSString::from_str(&engine.to_string_lossy())),
        None => Some(NSBundle::mainBundle()),
    };
    bundle
        .and_then(|bundle| bundle.objectForInfoDictionaryKey(&NSString::from_str("KoyoMockKeychain")))
        .and_then(|value| value.downcast::<NSNumber>().ok())
        .is_some_and(|value| value.boolValue())
}

/// The host's own app bundle, `Koyo Browser Host.app`: the main bundle, or inside Koyo's
/// process the engine's.
pub(crate) fn own_bundle() -> Option<String> {
    if let Some(engine) = embedded::engine() {
        return Some(engine.to_string_lossy().into_owned());
    }
    let bundle = objc2_foundation::NSBundle::mainBundle()
        .bundlePath()
        .to_string();
    let is_app = std::path::Path::new(&bundle)
        .extension()
        .is_some_and(|ext| ext == "app");
    is_app.then_some(bundle)
}

/// The binary's whole life, as the browser process or one of CEF's helpers.
#[expect(
    clippy::print_stderr,
    reason = "the host has no log of its own before CEF is up: its stderr is the app's"
)]
#[must_use]
pub fn main() -> ExitCode {
    let helper = std::env::args().any(|arg| arg.starts_with("--type="));
    let _framework = {
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => {
                eprintln!("koyo-browser-host: no executable path: {error}");
                return ExitCode::FAILURE;
            }
        };
        let loader = cef::library_loader::LibraryLoader::new(&exe, helper);
        if !loader.load() {
            eprintln!("koyo-browser-host: cannot load the Chromium Embedded Framework");
            return ExitCode::FAILURE;
        }
        loader
    };
    // The API version this build was compiled against, before any other call.
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new();

    if helper {
        let mut sandbox = cef::sandbox::Sandbox::new();
        sandbox.initialize(args.as_main_args());
        let mut app = app::HostApp::new();
        let code = cef::execute_process(
            Some(args.as_main_args()),
            Some(&mut app),
            std::ptr::null_mut(),
        );
        return u8::try_from(code.max(0)).map_or(ExitCode::FAILURE, ExitCode::from);
    }

    match host::run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("koyo-browser-host: {error}");
            ExitCode::FAILURE
        }
    }
}
