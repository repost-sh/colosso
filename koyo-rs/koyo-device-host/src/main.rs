//! `koyo-device-host`: the host of one device (`docs/devices.md` 3), started by Koyo as its
//! child with its end of the link inherited.
//!
//! ```text
//! koyo-device-host --simulator <udid> --set <device set> --state <folder> [--stats]
//! koyo-device-host --phone <udid> --name <name> --state <folder> [--stats]
//! koyo-device-host --watch
//! ```

fn main() -> std::process::ExitCode {
    #[cfg(target_os = "macos")]
    {
        koyo_device_host::main(&std::env::args().skip(1).collect::<Vec<_>>())
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::process::ExitCode::FAILURE
    }
}
