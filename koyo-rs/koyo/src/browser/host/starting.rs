//! Starting the host: launched off the main thread, once what was under way on the profiles'
//! folder is done with it (`folder.rs`), its panes told only once it's taken a while. It
//! starts once while Koyo runs, in Koyo's own process (`browser/native`): one that fails to
//! start, or stops, leaves its panes saying why.

use std::time::Instant;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::Context;
use koyo_browser_ipc::client::{Connection, Incoming};
use koyo_browser_ipc::{self as ipc, AppRequest};

#[cfg(test)]
use super::TestLaunch;
use super::{Browsers, Host, STARTING_QUIETLY};

impl Browsers {
    pub(super) fn start(&mut self, cx: &mut Context<Self>) {
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
        self.asked_at = Some((Instant::now(), false));
        self.notify_views(cx);
        let (tell, mut told) = mpsc::unbounded::<Incoming>();
        let readied = super::folder::readied(cx);
        let started = cx.background_executor().spawn(async move {
            readied.await;
            let sink = move |message: Incoming| {
                tell.unbounded_send(message)
                    .err()
                    .map(mpsc::TrySendError::into_inner)
            };
            crate::browser::native::start(&launch, sink)
        });
        let quietly = cx.background_executor().timer(STARTING_QUIETLY);
        self._events = Some(cx.spawn(async move |this, cx| {
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
            while let Some(first) = told.next().await {
                let handled = this.update(cx, |this, cx| {
                    let mut next = Some(first);
                    while let Some(message) = next.take() {
                        let closed = matches!(message, Incoming::Closed);
                        this.receive(message, cx);
                        if !closed {
                            next = told.try_recv().ok();
                        }
                    }
                });
                if handled.is_err() {
                    return;
                }
            }
        }));
    }

    /// The host said hello, or failed to; `Some` once it runs.
    pub(super) fn started(
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
                self.fail(format!("The browser engine did not start: {error}"), cx);
                return None;
            }
        };
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
        let asked = self.asked_at.map_or(0, |(since, _)| since.elapsed().as_millis());
        let described = super::super::engine::described(&connection);
        if connection.framework.is_koyos() || cfg!(debug_assertions) {
            log::info!("browser engine serving, {asked} ms after it was asked for: {described}");
        } else {
            log::warn!("browser engine serving, {asked} ms after it was asked for: {described}");
        }
        self.host = Host::Running {
            connection,
            requests,
        };
        self.epoch += 1;
        self.notify_views(cx);
        Some(())
    }

    /// The running host went away on its own. It can't be started again in this process:
    /// the panes say so.
    pub(super) fn stopped(&mut self, cx: &mut Context<Self>) {
        if matches!(self.host, Host::Running { .. }) {
            self.fail(
                "The browser engine stopped. Restart Koyo to bring its pages back.".into(),
                cx,
            );
        }
    }

    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        log::warn!("{message}");
        self.asked_at = None;
        self.drop_calls(&message);
        self.host = Host::Failed(message);
        self.queued.clear();
        self.notify_views(cx);
    }
}
