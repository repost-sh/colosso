//! The device's lock as it changes (`docs/devices.md` 5): its own notices watched on a task of
//! the session's, each said at once so acts wait on it, and `devicectl` asked which way it went.

use std::time::Duration;

use idevice::IdeviceService as _;
use idevice::notification_proxy::NotificationProxyClient;
use idevice::provider::UsbmuxdProvider;
use tokio::sync::mpsc::UnboundedReceiver;

use super::checks;

/// The lock notifications the device posts as it locks and unlocks.
const LOCK_NOTICES: [&str; 2] = ["com.apple.springboard.lockstate", "com.apple.springboard.hasBlankedScreen"];
/// Notices that come this close together are one change, looked at once.
const NOTICES_SETTLE: Duration = Duration::from_millis(250);

/// What the lock watcher says: a notice came (`None`), or which way it went.
pub type LockWord = Option<bool>;

/// How long the lock watcher rests before it watches again, its notices having stopped.
const WATCH_AGAIN: Duration = Duration::from_secs(5);

/// Watches the device's lock notifications on a task of its own, for as long as the session
/// runs: each notice said at once (acts wait on it), notices that come together are one change,
/// after which `devicectl` is asked which way it went (`checks::locked`), off this thread. If
/// the notices stop, they're watched again a moment after.
pub fn watch(provider: &UsbmuxdProvider, udid: &str) -> UnboundedReceiver<LockWord> {
    let (told, heard) = tokio::sync::mpsc::unbounded_channel();
    let (provider, udid) = (provider.clone(), udid.to_owned());
    tokio::spawn(async move {
        while !told.is_closed() {
            if let Some(mut proxy) = lock_notices(&provider).await {
                while proxy.receive_notification().await.is_ok() {
                    if told.send(None).is_err() {
                        return;
                    }
                    while tokio::time::timeout(NOTICES_SETTLE, proxy.receive_notification()).await.is_ok_and(|notice| notice.is_ok()) {}
                    let asked = udid.clone();
                    let locked = tokio::task::spawn_blocking(move || checks::locked(&asked)).await.ok().flatten();
                    if told.send(locked).is_err() {
                        return;
                    }
                }
            }
            tokio::time::sleep(WATCH_AGAIN).await;
        }
    });
    heard
}

/// The device's notification proxy, observing its lock's notices.
async fn lock_notices(provider: &UsbmuxdProvider) -> Option<NotificationProxyClient> {
    let mut proxy = NotificationProxyClient::connect(provider).await.ok()?;
    for name in LOCK_NOTICES {
        proxy.observe_notification(name).await.ok()?;
    }
    Some(proxy)
}
