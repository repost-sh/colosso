//! The view's link to its terminal: opened in the daemon if need be, attached with the size
//! its first layout gives, its VT thread started and listened to, and the daemon told of
//! each size the grid settles at. A lost daemon is attached again, which starts it.

use std::sync::Arc;
use std::time::Duration;

use colosso_term_control::{AttachMode, EventKind, OpenRequest, RequestBody, ResponseBody, Size};
use futures::StreamExt as _;
use gpui_kit::{AppContext as _, Context};

use super::{Link, TerminalEvent, TerminalView};
use crate::client::{TermClient, is_other_build};
use crate::element::Grid;
use crate::vt::{Command, Update, Vt};

/// How long the size settles before the daemon hears it, so a window being dragged
/// doesn't reflow the program at every step.
const RESIZE_SETTLE: Duration = Duration::from_millis(50);

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
        opened: Result<ResponseBody, colosso_term_control::ClientError>,
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
        if let Some(grid) = self.grid.get() {
            self.attach(size_of(grid), cx);
        }
    }

    /// The element laid the grid out anew: attach with it, or tell the daemon.
    pub(crate) fn grid_changed(&mut self, grid: Grid, cx: &mut Context<Self>) {
        let size = size_of(grid);
        log::debug!("{}: laid out {}x{}", self.name, size.cols, size.rows);
        match self.link {
            Link::Waiting => self.attach(size, cx),
            Link::Attached { .. } if self.requested != Some(size) => {
                let Some(handle) = self.handle.clone() else {
                    return;
                };
                let client = Arc::clone(&self.client);
                self.resize = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(RESIZE_SETTLE).await;
                    let sent = cx
                        .background_spawn(async move {
                            client.request(RequestBody::Resize { handle, size })
                        })
                        .await;
                    if let Err(error) = sent {
                        log::warn!("couldn't resize the terminal: {error}");
                    }
                    this.update(cx, |this, _| this.requested = Some(size)).ok();
                }));
            }
            Link::Opening
            | Link::Attaching
            | Link::Attached { .. }
            | Link::OtherBuild
            | Link::Replacing
            | Link::Failed(_) => {}
        }
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
        self.requested = Some(size);
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
        attached: Result<crate::client::Attachment, colosso_term_control::ClientError>,
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
                if let Some(grid) = self.grid.get() {
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
                if let Some(grid) = self.grid.get() {
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
