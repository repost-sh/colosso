//! Koyo's browser host.
//!
//! One binary plays every CEF role. Started by the app, it is the browser
//! process: it connects back to the app's socket, renders each browser pane
//! off-screen and hands the frames over as shared surfaces. Started by CEF
//! with `--type=…` from inside a helper bundle, it is a renderer, GPU or
//! utility process and only runs CEF's subprocess entry point.
#![expect(
    clippy::transmute_ptr_to_ptr,
    reason = "cef's wrap_* macros transmute references in what they expand to"
)]
mod app;
mod devtools;
mod dialogs;
mod host;
#[cfg(target_os = "macos")]
mod mac_app;
mod menus;
mod pane;
mod pending;
#[cfg(target_os = "macos")]
mod surfaces;

use std::process::ExitCode;

/// Whether this engine was packaged without a release signature: its
/// Info.plist says so, and a signed bundle's Info.plist is sealed.
#[cfg(target_os = "macos")]
pub(crate) fn mock_keychain() -> bool {
    use objc2_foundation::{NSBundle, NSNumber, NSString};
    NSBundle::mainBundle()
        .objectForInfoDictionaryKey(&NSString::from_str("KoyoMockKeychain"))
        .and_then(|value| value.downcast::<NSNumber>().ok())
        .is_some_and(|value| value.boolValue())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn mock_keychain() -> bool {
    false
}

/// The host's own app bundle, `Koyo Browser Host.app`.
#[cfg(target_os = "macos")]
pub(crate) fn own_bundle() -> Option<String> {
    let bundle = objc2_foundation::NSBundle::mainBundle().bundlePath().to_string();
    let is_app = std::path::Path::new(&bundle).extension().is_some_and(|ext| ext == "app");
    is_app.then_some(bundle)
}

/// The rate pages paint at: the fastest display's, so a 120 Hz display
/// scrolls at 120 frames a second. Chromium only paints what changes, so an
/// idle page costs nothing more.
#[cfg(target_os = "macos")]
pub(crate) fn frame_rate() -> i32 {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    MainThreadMarker::new()
        .and_then(|mtm| {
            NSScreen::screens(mtm)
                .iter()
                .map(|screen| screen.maximumFramesPerSecond())
                .max()
        })
        .and_then(|fps| i32::try_from(fps).ok())
        .map_or(60, |fps| fps.clamp(30, 240))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn frame_rate() -> i32 {
    60
}

/// The largest display's size in points, which no pane outgrows: what a page's view grows to
/// while its pane's edge is dragged, so a drag never has it grow twice.
#[cfg(target_os = "macos")]
pub(crate) fn largest_screen() -> Option<(u32, u32)> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    let mtm = MainThreadMarker::new()?;
    let points = |side: f64| side.max(0.).round() as u32;
    NSScreen::screens(mtm)
        .iter()
        .map(|screen| {
            let size = screen.frame().size;
            (points(size.width), points(size.height))
        })
        .max_by_key(|(width, height)| u64::from(*width) * u64::from(*height))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn largest_screen() -> Option<(u32, u32)> {
    None
}

#[expect(
    clippy::print_stderr,
    reason = "the host has no log of its own before CEF is up: its stderr is the app's"
)]
fn main() -> ExitCode {
    let helper = std::env::args().any(|arg| arg.starts_with("--type="));
    #[cfg(target_os = "macos")]
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
        #[cfg(target_os = "macos")]
        let mut sandbox = cef::sandbox::Sandbox::new();
        #[cfg(target_os = "macos")]
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
