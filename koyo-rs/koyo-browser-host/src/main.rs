//! Koyo's browser host, as a binary: the browser process the app starts for off-screen pages,
//! and every helper CEF starts (`koyo_browser_host`).

fn main() -> std::process::ExitCode {
    koyo_browser_host::main()
}
