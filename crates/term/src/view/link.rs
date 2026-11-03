//! The view's link to its terminal: opened in the daemon if need be, attached with the size
//! its first layout gives, its VT thread started and listened to, and the daemon told of
//! each size the grid is laid out at, as it goes: a pane dragged resizes the program at every
//! frame, as a terminal window does. A lost daemon is attached again, which starts it.

use std::sync::Arc;

use koyo_term_control::{
    AttachMode, EventKind, Handle, OpenRequest, RequestBody, ResponseBody, Size,
};
use futures::StreamExt as _;
use gpui_kit::{AppContext as _, Context};

use super::{Link, TerminalEvent, TerminalView};
use crate::client::{TermClient, is_other_build};
use crate::element::Grid;
use crate::vt::{Command, Update, Vt};

/// What's to be sent the daemon next, whether a send is under way, and what was sent last:
/// at most one send is under way, so what's asked for as a pane or a grip is dragged goes at
/// every frame the daemon keeps up with, and what's asked for last goes last; what was sent
/// last isn't sent again, unless it failed.
pub(super) struct Latest<T> {
    wanted: Option<T>,
    sending: bool,
    pub(super) sent: Option<T>,
}

impl<T> Default for Latest<T> {
    fn default() -> Self {
        Latest {
            wanted: None,
            sending: false,
            sent: None,
        }
    }
}

impl TerminalView {
    pub(super) fn open(client: Arc<TermClient>, request: OpenRequest, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let opened = cx
                .background_spawn(async move { client.request(RequestBody::Open(request)) })
                .await;
            this.update(cx, |this, cx| this.opened(opened, cx)).ok();
        })
        .detach();
    }

    fn opened(
        &mut self,
        opened: Result<ResponseBody, koyo_term_control::ClientError>,
        cx: &mut Context<Self>,
    ) {
        let record = match opened {
            Ok(ResponseBody::Opened { terminal, .. }) => terminal,
            Ok(_) => {
                self.link =
                    Link::Failed("The daemon answered the open with something else.".into());
                return cx.notify();
            }
            Err(error) if is_other_build(&error) => {
                log::warn!("couldn't open the terminal {}: {error}", self.name);
                self.link = Link::OtherBuild;
                return cx.notify();
            }
            Err(error) => {
                log::warn!("couldn't open the terminal {}: {error}", self.name);
                self.link = Link::Failed(format!("Couldn't open the terminal: {error}").into());
                return cx.notify();
            }
        };
        self.handle = Some(record.handle.clone());
        self.request = None;
        self.name.clone_from(&record.name);
        self.exited = record.exit;
        self.link = Link::Waiting;
        cx.emit(TerminalEvent::Opened(record));
        // Laid out already while the daemon opened it.
        if let Some(grid) = self.fill.get() {
            self.attach(size_of(grid), cx);
        }
    }

    /// The element laid the grid out anew at the person's font: attach with it, or tell the
    /// daemon, which keeps it for when the terminal isn't pinned.
    pub(crate) fn grid_changed(&mut self, grid: Grid, cx: &mut Context<Self>) {
        let size = size_of(grid);
        log::debug!("{}: laid out {}x{}", self.name, size.cols, size.rows);
        match self.link {
            Link::Waiting => self.attach(size, cx),
            Link::Attached { .. } => self.send_size(size, cx),
            Link::Opening
            | Link::Attaching
            | Link::OtherBuild
            | Link::Replacing
            | Link::Failed(_) => {}
        }
    }

    /// Tells the daemon of `size` now, or, while it's being told of another, once that's
    /// answered: whichever size is the latest then, unless it's the one sent last.
    fn send_size(&mut self, size: Size, cx: &mut Context<Self>) {
        let request = |handle, size| RequestBody::Resize { handle, size };
        let failed = |_: &mut Self, _, _: &mut Context<Self>| {};
        self.send_latest(size, |this| &mut this.resize, request, failed, "resize", cx);
    }

    /// Sends the daemon `value` now, or, while the last is being sent, once that's answered:
    /// whichever is the latest then, unless it's the one sent last. `latest` is where it
    /// waits, `request` makes what's sent of it, `failed` hears of one that didn't go
    /// through, which is sent again when it's asked for again, and `what` says what a
    /// failure couldn't do.
    pub(super) fn send_latest<T: Copy + PartialEq + 'static>(
        &mut self,
        value: T,
        latest: fn(&mut Self) -> &mut Latest<T>,
        request: fn(Handle, T) -> RequestBody,
        failed: fn(&mut Self, T, &mut Context<Self>),
        what: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let pending = latest(self);
        pending.wanted = Some(value);
        if pending.sending {
            return;
        }
        pending.sending = true;
        let client = Arc::clone(&self.client);
        cx.spawn(async move |this, cx| {
            loop {
                let next = this.update(cx, |this, _| {
                    let pending = latest(this);
                    let wanted =
                        (pending.wanted.take()).filter(|value| pending.sent != Some(*value));
                    pending.sending = wanted.is_some();
                    pending.sent = wanted.or(pending.sent);
                    wanted
                });
                let Ok(Some(value)) = next else {
                    break;
                };
                let (client, body) = (Arc::clone(&client), request(handle.clone(), value));
                let sent = cx
                    .background_spawn(async move { client.request(body) })
                    .await;
                if let Err(error) = sent {
                    log::warn!("couldn't {what} the terminal: {error}");
                    let heard = this.update(cx, |this, cx| {
                        let pending = latest(this);
                        if pending.sent == Some(value) {
                            pending.sent = None;
                        }
                        failed(this, value, cx);
                    });
                    if heard.is_err() {
                        break;
                    }
                }
            }
        })
        .detach();
    }

    fn attach(&mut self, size: Size, cx: &mut Context<Self>) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        log::debug!(
            "{}: attaching {handle} at {}x{}",
            self.name,
            size.cols,
            size.rows
        );
        self.link = Link::Attaching;
        self.resize.sent = Some(size);
        let client = Arc::clone(&self.client);
        cx.spawn(async move |this, cx| {
            let attached = cx
                .background_spawn(async move { client.attach(&handle, size, AttachMode::Control) })
                .await;
            this.update(cx, |this, cx| this.attached(attached, cx)).ok();
        })
        .detach();
    }

    fn attached(
        &mut self,
        attached: Result<crate::client::Attachment, koyo_term_control::ClientError>,
        cx: &mut Context<Self>,
    ) {
        let attachment = match attached {
            Ok(attachment) => attachment,
            Err(error) if is_other_build(&error) => {
                log::warn!("couldn't attach {}: {error}", self.name);
                self.link = Link::OtherBuild;
                return cx.notify();
            }
            Err(error) => {
                log::warn!("couldn't attach {}: {error}", self.name);
                self.link = Link::Failed(format!("Couldn't attach the terminal: {error}").into());
                return cx.notify();
            }
        };
        self.exited = attachment.record.exit;
        self.sized(attachment.record.size, attachment.record.pinned.clone(), cx);
        let (updates, mut received) = futures::channel::mpsc::unbounded();
        let index = attachment.index;
        let vt = match Vt::start(
            &self.name,
            index,
            attachment.connection,
            attachment.stream,
            updates,
        ) {
            Ok(vt) => vt,
            Err(error) => {
                self.link =
                    Link::Failed(format!("Couldn't start the terminal's thread: {error}").into());
                return cx.notify();
            }
        };
        vt.send(Command::Colors {
            foreground: self.look.foreground,
            background: self.look.background,
        });
        vt.send(Command::OptionAsAlt(self.option_as_alt));
        self.link = Link::Attached { vt, index };
        self.updates = Some(cx.spawn(async move |this, cx| {
            while let Some(update) = received.next().await {
                if this
                    .update(cx, |this, cx| this.on_update(update, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    /// What the notice's button does: stops the daemon of another build, then opens or
    /// attaches again, which starts this build's.
    pub fn replace_daemon(&mut self, cx: &mut Context<Self>) {
        if !self.is_other_build() {
            return;
        }
        self.link = Link::Replacing;
        cx.notify();
        let client = Arc::clone(&self.client);
        cx.spawn(async move |this, cx| {
            let replaced = cx
                .background_spawn(async move { client.replace_daemon() })
                .await;
            this.update(cx, |this, cx| match replaced {
                Ok(()) => {
                    cx.emit(TerminalEvent::DaemonReplaced);
                    this.link_again(cx);
                }
                Err(error) => {
                    log::warn!("couldn't stop the other build's terminal daemon: {error}");
                    this.link = Link::Failed(
                        format!("Couldn't stop the other build's terminals: {error}").into(),
                    );
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Opens or attaches again, once the daemon of another build that turned the view away
    /// was replaced.
    pub fn try_again(&mut self, cx: &mut Context<Self>) {
        if self.is_other_build() {
            self.link_again(cx);
        }
    }

    fn link_again(&mut self, cx: &mut Context<Self>) {
        match (&self.handle, self.request.clone()) {
            (Some(_), _) => {
                self.link = Link::Waiting;
                if let Some(grid) = self.fill.get() {
                    self.attach(size_of(grid), cx);
                }
            }
            (None, Some(request)) => {
                self.link = Link::Opening;
                Self::open(Arc::clone(&self.client), *request, cx);
            }
            (None, None) => self.link = Link::Failed("There's no terminal to open again.".into()),
        }
        cx.notify();
    }

    fn on_update(&mut self, update: Update, cx: &mut Context<Self>) {
        log::trace!("{}: {update:?}", self.name);
        match update {
            Update::Frame => {
                if let Link::Attached { vt, .. } = &self.link
                    && let Some(frame) = vt.take_frame()
                {
                    if self
                        .frame
                        .as_ref()
                        .is_none_or(|before| before.title != frame.title)
                    {
                        cx.emit(TerminalEvent::Title(frame.title.clone()));
                    }
                    self.frame = Some(frame);
                    cx.notify();
                }
            }
            Update::Event(kind) => match *kind {
                EventKind::Exited { exit } => {
                    self.exited = Some(exit);
                    cx.emit(TerminalEvent::Exited(exit));
                    cx.notify();
                }
                EventKind::Spawned { .. } => {
                    self.exited = None;
                    cx.notify();
                }
                EventKind::Closed => cx.emit(TerminalEvent::Closed),
                EventKind::Bell => cx.emit(TerminalEvent::Bell),
                EventKind::Size { size, pinned } => self.sized(size, pinned, cx),
                EventKind::Refused { error } => {
                    log::info!("{}: input refused: {}", self.name, error.message);
                }
                _ => {}
            },
            Update::Lagging => {}
            Update::Disconnected => {
                // The daemon went away; attach again, which starts it if need be.
                self.link = Link::Waiting;
                self.updates = None;
                if let Some(grid) = self.fill.get() {
                    self.attach(size_of(grid), cx);
                }
            }
        }
    }
}

/// The size the daemon is told for a grid: its cells, and a cell in device pixels.
fn size_of(grid: Grid) -> Size {
    Size {
        cols: grid.cols,
        rows: grid.rows,
        cell_width_px: (f32::from(grid.metrics.cell_width) * grid.scale).round() as u32,
        cell_height_px: (f32::from(grid.metrics.cell_height) * grid.scale).round() as u32,
    }
}
