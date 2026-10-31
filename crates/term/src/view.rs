//! A terminal as a view (`docs/terminals.md` 7.1): attached once its first layout gives the
//! size to attach with, fed frames by its VT thread, sized to its bounds as they change, and
//! detached, never closed, when it goes away: the terminal lives on in the daemon.

use std::cell::{Cell as Shared, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use colosso_term_control::{Exit, Handle, OpenRequest, Size, TerminalRecord};
use colosso_term_engine::{Cursor, Frame, OptionAsAlt};
use gpui_kit::{
    App, Bounds, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyBinding, ParentElement as _, Pixels, Render, SharedString, Styled as _, Task,
    Window, actions, div, point, size,
};

use crate::client::TermClient;
use crate::element::{Grid, TerminalElement};
use crate::glyphs::{Glyphs, Look};
use crate::vt::{Command, Vt};

mod link;

actions!(
    terminal,
    [
        Copy,
        Paste,
        ScrollPageUp,
        ScrollPageDown,
        ScrollToTop,
        ScrollToBottom
    ]
);

/// The key context terminal views take, which their bindings are scoped to.
pub const CONTEXT: &str = "Terminal";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
        KeyBinding::new("cmd-v", Paste, Some(CONTEXT)),
        KeyBinding::new("shift-pageup", ScrollPageUp, Some(CONTEXT)),
        KeyBinding::new("shift-pagedown", ScrollPageDown, Some(CONTEXT)),
        KeyBinding::new("cmd-home", ScrollToTop, Some(CONTEXT)),
        KeyBinding::new("cmd-end", ScrollToBottom, Some(CONTEXT)),
    ]);
}

/// Half a blink.
const BLINK: Duration = Duration::from_millis(530);

/// The terminal a view shows: one the daemon keeps already, or one to open.
#[derive(Clone, Debug)]
pub enum Source {
    Existing { handle: Handle, name: String },
    Open(Box<OpenRequest>),
}

/// What the view tells whoever shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum TerminalEvent {
    /// The terminal it opened, or found under its name.
    Opened(Box<TerminalRecord>),
    Title(String),
    Bell,
    Exited(Exit),
    /// The terminal was closed and forgotten.
    Closed,
}

pub(crate) enum Link {
    /// Asking the daemon for the terminal.
    Opening,
    /// Waiting for the first layout, which gives the size to attach with.
    Waiting,
    Attaching,
    Attached {
        vt: Vt,
        index: u32,
    },
    Failed(SharedString),
}

pub struct TerminalView {
    pub(crate) client: Arc<TermClient>,
    /// `None` while the terminal is being opened.
    pub(crate) handle: Option<Handle>,
    name: String,
    pub(crate) link: Link,
    pub(crate) frame: Option<Arc<Frame>>,
    look: Rc<Look>,
    glyphs: Rc<RefCell<Glyphs>>,
    pub(crate) grid: Rc<Shared<Option<Grid>>>,
    pub(crate) focus: FocusHandle,
    /// The size last sent to the daemon.
    requested: Option<Size>,
    resize: Option<Task<()>>,
    updates: Option<Task<()>>,
    blink: Option<Task<()>>,
    cursor_on: bool,
    pub(crate) marked: Option<String>,
    pub(crate) drag: Option<crate::input::Drag>,
    /// Wheel travel short of a whole row, kept for the next event.
    pub(crate) wheel_rest: f32,
    pub(crate) option_as_alt: OptionAsAlt,
    exited: Option<Exit>,
    _subscriptions: Vec<gpui_kit::Subscription>,
}

impl EventEmitter<TerminalEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TerminalView {
    pub fn new(
        client: Arc<TermClient>,
        source: Source,
        look: Look,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus, window, |this, _, cx| this.focused(true, cx)),
            cx.on_blur(&focus, window, |this, _, cx| this.focused(false, cx)),
        ];
        let (handle, name, link) = match source {
            Source::Existing { handle, name } => (Some(handle), name, Link::Waiting),
            Source::Open(request) => {
                let name = request.name.clone();
                Self::open(Arc::clone(&client), *request, cx);
                (None, name, Link::Opening)
            }
        };
        TerminalView {
            client,
            handle,
            name,
            link,
            frame: None,
            look: Rc::new(look),
            glyphs: Rc::default(),
            grid: Rc::default(),
            focus,
            requested: None,
            resize: None,
            updates: None,
            blink: None,
            cursor_on: true,
            marked: None,
            drag: None,
            wheel_rest: 0.,
            option_as_alt: OptionAsAlt::No,
            exited: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn handle(&self) -> Option<&Handle> {
        self.handle.as_ref()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn title(&self) -> Option<&str> {
        self.frame
            .as_ref()
            .map(|frame| frame.title.as_str())
            .filter(|title| !title.is_empty())
    }

    /// The screen's text as last drawn, a line per row, trailing blanks trimmed.
    pub fn text(&self) -> String {
        self.frame
            .as_ref()
            .map(|frame| frame.text())
            .unwrap_or_default()
    }

    /// Whether a full-screen program has the alternate screen up.
    pub fn alt_screen(&self) -> bool {
        self.frame.as_ref().is_some_and(|frame| frame.alt_screen)
    }

    /// The grid as last laid out, in columns and rows.
    pub fn grid_size(&self) -> Option<(u16, u16)> {
        self.grid.get().map(|grid| (grid.cols, grid.rows))
    }

    /// Where the cell at `col`, `row` is drawn in the window, as last laid out.
    pub fn cell_bounds(&self, col: u16, row: u16) -> Option<Bounds<Pixels>> {
        let grid = self.grid.get()?;
        let (width, height) = (grid.metrics.cell_width, grid.metrics.cell_height);
        let origin = grid.origin + point(width * f32::from(col), height * f32::from(row));
        Some(Bounds::new(origin, size(width, height)))
    }

    /// Asks the VT thread for the selected text, which arrives on the receiver: `None` when
    /// nothing is selected. Nothing arrives when the terminal isn't attached.
    pub fn selected_text(&self) -> crossbeam_channel::Receiver<Option<String>> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.send(Command::Copy(tx));
        rx
    }

    pub fn exited(&self) -> Option<Exit> {
        self.exited
    }

    pub fn set_look(&mut self, look: Look, cx: &mut Context<Self>) {
        if *self.look == look {
            return;
        }
        self.look = Rc::new(look);
        self.send(Command::Colors {
            foreground: self.look.foreground,
            background: self.look.background,
        });
        cx.notify();
    }

    pub fn set_option_as_alt(&mut self, option: OptionAsAlt) {
        self.option_as_alt = option;
        self.send(Command::OptionAsAlt(option));
    }

    pub(crate) fn send(&self, command: Command) {
        if let Link::Attached { vt, .. } = &self.link {
            vt.send(command);
        }
    }

    fn focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        self.send(Command::Focus(focused));
        self.cursor_on = true;
        self.blink = focused.then(|| self.blinker(cx));
        cx.notify();
    }

    /// Typing shows the cursor and restarts its blink.
    pub(crate) fn typed(&mut self, cx: &mut Context<Self>) {
        self.cursor_on = true;
        if self.blink.is_some() {
            self.blink = Some(self.blinker(cx));
        }
    }

    fn blinker(&self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let blinks = this
                    .update(cx, |this, cx| {
                        let blinking = this
                            .frame
                            .as_ref()
                            .and_then(|frame| frame.cursor)
                            .is_some_and(Cursor::blinking);
                        if blinking {
                            this.cursor_on = !this.cursor_on;
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !blinks {
                    break;
                }
            }
        })
    }
}

impl Drop for TerminalView {
    fn drop(&mut self) {
        if let (Link::Attached { index, .. }, Some(handle)) = (&self.link, self.handle.clone()) {
            let (client, index) = (Arc::clone(&self.client), *index);
            // The view is going; the terminal stays in the daemon.
            let detached = std::thread::Builder::new()
                .name("term-detach".into())
                .spawn(move || client.detach(&handle, index));
            if let Err(error) = detached {
                log::warn!("couldn't detach {}: {error}", self.name);
            }
        }
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let element = TerminalElement {
            view: cx.entity(),
            frame: self.frame.clone(),
            look: Rc::clone(&self.look),
            glyphs: Rc::clone(&self.glyphs),
            grid: Rc::clone(&self.grid),
            focus: self.focus.clone(),
            focused: self.focus.is_focused(window),
            cursor_on: self.cursor_on,
            marked: self.marked.clone().map(SharedString::from),
        };
        let notice = match &self.link {
            Link::Failed(message) => Some(message.clone()),
            _ => None,
        };
        div()
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .size_full()
            .relative()
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::scroll_page_up))
            .on_action(cx.listener(Self::scroll_page_down))
            .on_action(cx.listener(Self::scroll_to_top))
            .on_action(cx.listener(Self::scroll_to_bottom))
            .child(element)
            .children(notice.map(|message| div().absolute().top_2().left_2().child(message)))
    }
}
