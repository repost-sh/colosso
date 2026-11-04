//! A terminal as a view (`docs/terminals.md` 7.1): attached once its first layout gives the
//! size to attach with, fed frames by its VT thread, sized to its bounds as they change, and
//! detached, never closed, when it goes away: the terminal lives on in the daemon.

use std::cell::{Cell as Shared, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use koyo_term_control::{Exit, Handle, OpenRequest, Pinned, Size, TerminalRecord};
use koyo_term_engine::{Cursor, Frame, OptionAsAlt};
use gpui_kit::{
    Action, App, Bounds, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyBinding, NoAction, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, actions, div, point, px, size,
};

use crate::client::TermClient;
use crate::element::{Grid, PinnedLook, TerminalElement};
use crate::fit::{Fitting, Scale};
use crate::glyphs::{Glyphs, Look};
use crate::vt::{Command, Vt};

mod link;
mod responsive;


actions!(
    terminal,
    [
        Copy,
        Paste,
        ScrollPageUp,
        ScrollPageDown,
        ScrollToTop,
        ScrollToBottom,
        ToggleResponsive
    ]
);

/// The key context terminal views take, which their bindings are scoped to.
pub const CONTEXT: &str = "Terminal";

/// What macOS terminals make of a few Command and Option chords, as Ghostty and iTerm2 do:
/// the line editing a shell reads (Emacs keys), since a program is never sent Command.
pub const MAC_EDITING: [(&str, &str); 5] = [
    // Deletes back to the line's start.
    ("cmd-backspace", "\x15"),
    // To the line's start and end.
    ("cmd-left", "\x01"),
    ("cmd-right", "\x05"),
    // A word back and forward.
    ("alt-left", "\x1bb"),
    ("alt-right", "\x1bf"),
];

/// Sends bytes to the program, for a chord that means them ([`MAC_EDITING`]).
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = terminal, no_json)]
pub struct SendText(pub &'static str);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
        KeyBinding::new("cmd-v", Paste, Some(CONTEXT)),
        KeyBinding::new("shift-pageup", ScrollPageUp, Some(CONTEXT)),
        KeyBinding::new("shift-pagedown", ScrollPageDown, Some(CONTEXT)),
        KeyBinding::new("cmd-home", ScrollToTop, Some(CONTEXT)),
        KeyBinding::new("cmd-end", ScrollToBottom, Some(CONTEXT)),
        KeyBinding::new("cmd-alt-r", ToggleResponsive, Some(CONTEXT)),
        // The program's, through `key_down`, not the window's, which moves focus on them.
        KeyBinding::new("tab", NoAction, Some(CONTEXT)),
        KeyBinding::new("shift-tab", NoAction, Some(CONTEXT)),
    ]);
    cx.bind_keys(
        (MAC_EDITING.iter())
            .map(|(keys, text)| KeyBinding::new(keys, SendText(text), Some(CONTEXT))),
    );
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
    /// Its pin, or the font it's fitted at, changed: what `pinned` and `font_percent` say is
    /// new.
    Responsive,
    /// The pin at these columns and rows didn't take: the daemon refused it, or couldn't be
    /// reached.
    PinRefused(u16, u16),
    /// The daemon of another build was replaced: views it turned away may try again.
    DaemonReplaced,
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
    /// The daemon is another build's: the notice offers to replace it (`docs/terminals.md`
    /// 5.1).
    OtherBuild,
    /// Stopping the other build's daemon.
    Replacing,
    Failed(SharedString),
}

pub struct TerminalView {
    pub(crate) client: Arc<TermClient>,
    /// `None` while the terminal is being opened.
    pub(crate) handle: Option<Handle>,
    /// What opens the terminal, kept until it's open so it can be asked again.
    request: Option<Box<OpenRequest>>,
    name: String,
    pub(crate) link: Link,
    pub(crate) frame: Option<Arc<Frame>>,
    look: Rc<Look>,
    glyphs: Rc<RefCell<Glyphs>>,
    pub(crate) grid: Rc<Shared<Option<Grid>>>,
    /// The grid the pane holds at the person's font, whose size the daemon is sent.
    pub(crate) fill: Rc<Shared<Option<Grid>>>,
    /// The element's bounds as last laid out, which a pin is fitted to.
    pub(crate) pane: Rc<Shared<Option<Bounds<Pixels>>>>,
    /// The terminal's size in the daemon, and the pin it comes from (`docs/responsive.md`).
    pub(crate) size: Option<Size>,
    pub(crate) pinned: Option<Pinned>,
    pub(crate) fitting: Rc<Shared<Fitting>>,
    /// The look the pinned grid is drawn in at a smaller font, kept across frames.
    pinned_look: PinnedLook,
    /// How large the pinned grid's font is: the pane's say.
    pub(crate) scale: Scale,
    /// The pin to send next, one at a time (`None` unpins), and the one sent last.
    pin: link::Latest<Option<(u16, u16)>>,
    pub(crate) focus: FocusHandle,
    /// The size to send the daemon next, one at a time, and the one sent last: a pane
    /// dragged resizes the program at every frame the daemon keeps up with, and at the size
    /// it's dragged to last.
    resize: link::Latest<Size>,
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
        let (handle, name, link, request) = match source {
            Source::Existing { handle, name } => (Some(handle), name, Link::Waiting, None),
            Source::Open(request) => {
                let name = request.name.clone();
                Self::open(Arc::clone(&client), (*request).clone(), cx);
                (None, name, Link::Opening, Some(request))
            }
        };
        TerminalView {
            client,
            handle,
            request,
            name,
            link,
            frame: None,
            look: Rc::new(look),
            glyphs: Rc::default(),
            grid: Rc::default(),
            fill: Rc::default(),
            pane: Rc::default(),
            size: None,
            pinned: None,
            fitting: Rc::default(),
            pinned_look: Rc::default(),
            scale: Scale::Fit,
            pin: link::Latest::default(),
            focus,
            resize: link::Latest::default(),
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
            fill: Rc::clone(&self.fill),
            pane: Rc::clone(&self.pane),
            pinned: self
                .pinned
                .as_ref()
                .map(|pinned| (pinned.cols, pinned.rows)),
            scale: self.scale,
            fitting: Rc::clone(&self.fitting),
            pinned_look: Rc::clone(&self.pinned_look),
            focus: self.focus.clone(),
            focused: self.focus.is_focused(window),
            cursor_on: self.cursor_on,
            marked: self.marked.clone().map(SharedString::from),
        };
        let notice = match &self.link {
            Link::Failed(message) => Some(message.clone()),
            Link::Replacing => Some("Restarting the terminals…".into()),
            _ => None,
        };
        let other_build = matches!(self.link, Link::OtherBuild).then(|| self.other_build(cx));
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
            .on_action(
                cx.listener(|this, SendText(text): &SendText, _, cx| this.type_text(text, cx)),
            )
            // ⌥⌘R goes on up, to the pane that shows the terminal responsive.
            .child(element)
            .children(notice.map(|message| div().absolute().top_2().left_2().child(message)))
            .children(other_build)
    }
}

impl TerminalView {
    /// Whether the daemon turned the view away as another build's.
    pub fn is_other_build(&self) -> bool {
        matches!(self.link, Link::OtherBuild)
    }

    /// 5.1's notice: the terminals are another build's, and a button replaces its daemon.
    fn other_build(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let foreground = crate::paint::hsla(self.look.foreground);
        let background = crate::paint::hsla(self.look.background);
        let restart = div()
            .id("restart-terminals")
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(foreground.opacity(0.5))
            .cursor_pointer()
            .hover(move |style| style.bg(foreground.opacity(0.1)))
            .child("Restart terminals")
            .on_click(cx.listener(|this, _, _, cx| this.replace_daemon(cx)));
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            // As wide as the tab allows, up to a comfortable line.
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_3()
                    .p_4()
                    .w_full()
                    .max_w(px(440.))
                    .rounded_md()
                    .border_1()
                    .border_color(foreground.opacity(0.3))
                    .bg(background)
                    .text_color(foreground)
                    .child(
                        "Terminals are served by another build of Koyo; restart them to \
                         use this one. Their programs end, and the shells you opened start \
                         again.",
                    )
                    .child(restart),
            )
    }
}
