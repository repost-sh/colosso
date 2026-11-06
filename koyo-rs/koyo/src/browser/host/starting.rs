//! Starting the host: launched off the main thread, its panes told only once it's taken a
//! while, started once more if it failed to, and again when it stops on its own, unless it
//! keeps stopping; then the person is told why.

use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::Context;
use koyo_browser_ipc::client::{Connection, Incoming};
use koyo_browser_ipc::{self as ipc, AppRequest};

#[cfg(test)]
use super::TestLaunch;
use super::inbox::Inbox;
use super::{Browsers, Host, STARTING_QUIETLY};

/// How many times a host that failed to start is started again before the person is told.
const START_RETRIES: u32 = 1;
/// A host that stops is started again for its pages, unless it stopped this often lately:
/// then it would only stop again, and the person is told.
const RESTARTS: usize = 3;
const RESTARTS_WITHIN: Duration = Duration::from_secs(60);

impl Browsers {
    pub(super) fn start(&mut self, cx: &mut Context<Self>) {
        // A unit test starts only the engine it names: the app's would keep its profiles in
        // the person's data folder.
        #[cfg(test)]
        let launch = match cx.try_global::<TestLaunch>() {
            Some(TestLaunch(launch)) => Ok(launch.clone()),
            None => Err("this test starts no browser engine".to_owned()),
        };
        #[cfg(not(test))]
        let launch = crate::browser::engine::launch();
        let launch = match launch {
            Ok(launch) => launch,
            Err(why) => {
                self.fail(why, cx);
                return;
            }
        };
        self.host = Host::Starting;
        self.starting_since = Some(Instant::now());
        self.notify_views(cx);
        // Each message goes in the inbox; the first since a drain rings the loop below's bell.
        let inbox = Inbox::new();
        self.inbox = Some(inbox.clone());
        let (bell, mut rung) = mpsc::unbounded::<()>();
        let started = cx.background_executor().spawn(async move {
            Connection::start(&launch, move |message: Incoming| {
                if bell.is_closed() {
                    return Some(message);
                }
                if inbox.put(message) {
                    let _ = bell.unbounded_send(());
                }
                None
            })
        });
        let quietly = cx.background_executor().timer(STARTING_QUIETLY);
        self._events = Some(cx.spawn(async move |this, cx| {
            // Panes say the host is starting once it has taken a while.
            let started = futures::future::select(started, quietly).await;
            let started = match started {
                futures::future::Either::Left((started, _)) => started,
                futures::future::Either::Right(((), started)) => {
                    let _ = this.update(cx, |this, cx| this.notify_views(cx));
                    started.await
                }
            };
            let running = this
                .update(cx, |this, cx| this.started(started, cx))
                .ok()
                .flatten();
            if running.is_none() {
                return;
            }
            while rung.next().await.is_some() {
                if this.update(cx, Self::drain).is_err() {
                    return;
                }
            }
        }));
    }

    /// The host said hello, or failed to; `Some` once it runs.
    fn started(
        &mut self,
        started: std::io::Result<Connection>,
        cx: &mut Context<Self>,
    ) -> Option<()> {
        let writer = started.and_then(|connection| {
            let writer = connection.sender()?;
            Ok((connection, writer))
        });
        let (connection, writer) = match writer {
            Ok(started) => started,
            Err(error) => {
                let message = format!("The browser engine did not start: {error}");
                // Another app's host holding the profiles won't let go for a retry.
                let busy = error.kind() == std::io::ErrorKind::ResourceBusy;
                if !busy && self.failed_starts < START_RETRIES {
                    self.failed_starts += 1;
                    log::warn!("{message}; starting it again");
                    self.start(cx);
                } else {
                    self.fail(message, cx);
                }
                return None;
            }
        };
        // Socket writes stay off the main thread.
        let (requests, outgoing) = std::sync::mpsc::channel::<AppRequest>();
        std::thread::Builder::new()
            .name("browser-requests".into())
            .spawn(move || {
                let mut writer = writer;
                while let Ok(request) = outgoing.recv() {
                    if ipc::write_frame(&mut writer, &request).is_err() {
                        return;
                    }
                }
            })
            .ok()?;
        for request in std::mem::take(&mut self.queued) {
            let _ = requests.send(request);
        }
        log::info!(
            "browser engine {} (Chromium {}) serving",
            connection.engine,
            connection.chromium
        );
        self.host = Host::Running {
            connection,
            requests,
        };
        self.starting_since = None;
        self.failed_starts = 0;
        self.epoch += 1;
        self.notify_views(cx);
        Some(())
    }

    /// The running host went away on its own: it is started again for the pages, which
    /// reopen, unless it keeps stopping.
    pub(super) fn stopped(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.host, Host::Running { .. }) {
            return;
        }
        let now = Instant::now();
        self.stops
            .retain(|stop| now.duration_since(*stop) < RESTARTS_WITHIN);
        self.stops.push(now);
        if self.stops.len() > RESTARTS {
            self.fail("The browser engine stopped.".into(), cx);
            return;
        }
        log::warn!("the browser engine stopped; starting it again");
        self.drop_calls("The browser engine stopped.");
        self.rings.clear();
        self.pending.clear();
        self.host = Host::Idle;
        self.start(cx);
    }

    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        log::warn!("{message}");
        self.starting_since = None;
        self.drop_calls(&message);
        self.host = Host::Failed(message);
        self.queued.clear();
        self.pending.clear();
        self.rings.clear();
        self.notify_views(cx);
    }
}
