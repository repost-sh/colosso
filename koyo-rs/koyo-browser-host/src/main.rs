//! Koyo's browser host, as a binary: every helper CEF starts, and the browser process of
//! the engine's own tests (`koyo_browser_host`). Inside Koyo the host is the library.

fn main() -> std::process::ExitCode {
    koyo_browser_host::main()
}
