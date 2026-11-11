//! `koyo-device-host --watch` (`docs/devices.md` 3): usbmuxd's word when a device is plugged in
//! or pulled out by cable (R5; over the network isn't Koyo's), a line each on standard output
//! for the app, which then asks `devicectl` for the details: the app never polls, and idevice's
//! runtime stays out of it. The watcher ends when the app closes its standard input, or goes.
//!
//! ```text
//! attached <udid>
//! detached
//! ```

use std::collections::HashSet;
use std::io::{Read as _, Write as _};

use futures::StreamExt as _;
use idevice::usbmuxd::{Connection, UsbmuxdConnection, UsbmuxdListenEvent};

/// Watches until the app lets go: its standard input closed, or its standard output gone.
///
/// # Errors
/// When usbmuxd can't be reached or listened to.
pub fn watch() -> Result<(), String> {
    // The app closing its end of the pipe ends the watcher.
    let (closed, mut gone) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let mut sink = [0u8; 64];
        while std::io::stdin().read(&mut sink).is_ok_and(|read| read > 0) {}
        let _ = closed.send(());
    });
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|error| format!("no runtime: {error}"))?;
    runtime.block_on(async {
        let mut usbmuxd = UsbmuxdConnection::default().await.map_err(|error| format!("usbmuxd isn't there: {error}"))?;
        let mut events = usbmuxd.listen().await.map_err(|error| format!("usbmuxd won't say: {error}"))?;
        let mut out = std::io::stdout();
        // What's plugged in by cable, by usbmuxd's number: its going is said by that alone.
        let mut wired = HashSet::new();
        loop {
            let event = tokio::select! {
                event = events.next() => event,
                _ = &mut gone => return Ok(()),
            };
            let line = match event {
                Some(Ok(UsbmuxdListenEvent::Connected(device))) if device.connection_type == Connection::Usb => {
                    wired.insert(device.device_id);
                    format!("attached {}", device.udid)
                }
                Some(Ok(UsbmuxdListenEvent::Disconnected(number))) if wired.remove(&number) => "detached".to_owned(),
                Some(Ok(_)) => continue,
                Some(Err(error)) => return Err(format!("usbmuxd stopped saying: {error}")),
                None => return Ok(()),
            };
            if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
                return Ok(());
            }
        }
    })
}
