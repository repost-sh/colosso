//! A host for a test that answers the app's hello and keeps what the app asks of it: the app
//! runs against it as against the engine's, and a test reads the requests its pages send and
//! hands the app what a host says.

use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use gpui_kit::TestAppContext;
use koyo_browser_ipc::client::{Connection, Embedded, Incoming, Launch};
use koyo_browser_ipc::{self as ipc, AppRequest, Hello, HostEvent, PaneId};

use super::Browsers;

/// The pane of the request a test sends behind the app's, which says the app's have all come.
const FENCE: PaneId = PaneId::MAX;

/// The app's host, a test's: what the app asked of it, in order.
pub(crate) struct FakeHost {
    asked: Receiver<AppRequest>,
}

impl FakeHost {
    /// Starts it as the app's host, with its profiles in the scratch folder `name`: the pages
    /// the app has then open in it, as in any host that starts.
    pub(crate) fn serve(name: &str, cx: &mut TestAppContext) -> Self {
        let (tell, asked) = channel();
        let embedded = Embedded {
            start: Box::new(move |stream, _| answer(stream, tell)),
            finish: Box::new(|_| {}),
        };
        let scratch = crate::test_support::scratch(name);
        let launch = Launch {
            executable: scratch.join("fake-host"),
            profiles: scratch.join("profiles"),
            passkeys: false,
        };
        let connection = Connection::start_embedded(&launch, embedded, |_| None);
        let browsers = cx.update(Browsers::global);
        browsers.update(cx, |browsers, cx| browsers.started(connection, cx));
        cx.run_until_parked();
        Self { asked }
    }

    /// What the app asked of it since this was last read.
    pub(crate) fn asked(&self, cx: &mut TestAppContext) -> Vec<AppRequest> {
        cx.run_until_parked();
        let fence = AppRequest::Close { pane: FENCE };
        let browsers = cx.update(Browsers::global);
        browsers.update(cx, |browsers, _| browsers.send(fence.clone()));
        let mut asked = Vec::new();
        loop {
            let request = (self.asked.recv_timeout(Duration::from_secs(10)))
                .expect("the app's requests reach the host");
            if request == fence {
                return asked;
            }
            asked.push(request);
        }
    }

    /// Answers each protocol command the app sent since this was last read with what `reply`
    /// gives for its method, parameters and session: what the app asked, in order.
    pub(crate) fn answers(
        &self,
        cx: &mut TestAppContext,
        mut reply: impl FnMut(&str, &serde_json::Value, Option<&str>) -> Result<serde_json::Value, String>,
    ) -> Vec<AppRequest> {
        let asked = self.asked(cx);
        for request in &asked {
            if let AppRequest::Cdp { pane, call, session, method, params, .. } = request {
                let (result, error) = match reply(method, params, session.as_deref()) {
                    Ok(result) => (Some(result), None),
                    Err(error) => (None, Some(error)),
                };
                self.says(HostEvent::CdpReply { pane: *pane, call: *call, result, error }, cx);
            }
        }
        asked
    }

    /// The host says `event`, as the app hears it from the engine's.
    pub(crate) fn says(&self, event: HostEvent, cx: &mut TestAppContext) {
        let browsers = cx.update(Browsers::global);
        browsers.update(cx, |browsers, cx| {
            browsers.receive(Incoming::Event(event), cx);
        });
        cx.run_until_parked();
    }
}

/// Plays the host on its end of the socket, on a thread of its own: answers the app's hello,
/// already on the socket, with this protocol's, and passes on each request the app sends.
fn answer(mut stream: UnixStream, tell: Sender<AppRequest>) {
    std::thread::spawn(move || {
        let ready = HostEvent::Ready {
            protocol: ipc::PROTOCOL,
            engine: "fake".into(),
            chromium: "0".into(),
            framework: ipc::Framework::default(),
        };
        let greeted = ipc::read_frame::<Hello>(&mut stream).is_ok_and(|hello| hello.is_some());
        if !greeted || ipc::write_frame(&mut stream, &ready).is_err() {
            return;
        }
        while let Ok(Some(request)) = ipc::read_frame::<AppRequest>(&mut stream) {
            if tell.send(request).is_err() {
                return;
            }
        }
    });
}
