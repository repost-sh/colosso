//! What the stand-in tests and the off-screen pictures do to a link: what its host would have
//! said, a host standing in for one, its process, a failure and a host gone.

#[cfg(test)]
use koyo_device_wire::{Screen, State as Said, ToHost};
use gpui_kit::Context;
use koyo_device_wire::FromHost;

#[cfg(test)]
use super::host;
use super::{Incoming, Link};

impl Link {
    /// For the stand-in tests and the off-screen pictures: what the host would have said.
    #[cfg(any(test, feature = "visual-check"))]
    pub fn hear(&mut self, said: FromHost, cx: &mut Context<Self>) {
        self.receive(Incoming::Said(said), cx);
    }

    /// The host's process, while one runs: the live pictures kill it to see the restart.
    #[cfg(feature = "visual-check")]
    pub fn pid(&self) -> Option<u32> {
        self.running.as_ref().map(|running| running.pid)
    }

    /// For tests elsewhere in the app: a stand-in host, ready with `screen`; what the app tells it
    /// comes out of the receiver.
    #[cfg(test)]
    pub fn stand_in(&mut self, screen: Screen, cx: &mut Context<Self>) -> std::sync::mpsc::Receiver<ToHost> {
        let (tell, told) = std::sync::mpsc::channel();
        self.use_host(host::Running::stand_in(tell), cx);
        self.hear(FromHost::State { state: Said::Ready { screen } }, cx);
        told
    }

    /// For the off-screen pictures: the state a host that kept stopping leaves.
    #[cfg(feature = "visual-check")]
    pub fn failed(&mut self, why: &str, cx: &mut Context<Self>) {
        self.fail(why.to_owned(), cx);
    }

    /// For the stand-in tests: the host went.
    #[cfg(test)]
    pub fn host_closed(&mut self, cx: &mut Context<Self>) {
        self.receive(Incoming::Closed, cx);
    }
}
