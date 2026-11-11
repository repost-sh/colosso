//! Setting the agent desktop up (`docs/agent-desktop.md` 11): a dialog that takes the person
//! through the two grants its host needs, one at a time. Continue has the host ask macOS for
//! the next one and opens its pane of System Settings; the dialog is out of the way while
//! they're there, sees each grant the moment it's given, leads on to the next by itself, and
//! brings Koyo back once the desktop is made.

use gpui_kit::{
    App, Context, Div, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    div, px,
};

use super::granting::{Grant, Step};
use super::{Desk, State, desk};
use crate::modal::{self, Button, Mark, Tone};
use crate::theme::{self, Palette};

const WIDTH: f32 = 480.;
/// The lines of what the dialog says first, and of what it says about the step at hand.
const INTRO: f32 = 3.;
const HINT: f32 = 3.;
const GAP: f32 = 12.;
const HEIGHT: f32 = 2. * modal::PADDING
    + modal::HEADING_LINE
    + INTRO * modal::LINE
    + 2. * modal::STEP
    + HINT * modal::SMALL_LINE
    + modal::SMALL_LINE
    + modal::FOOTER
    + 5. * GAP;

pub struct Setup {
    desk: Entity<Desk>,
    /// The step the dialog last saw, so it moves on by itself as one is given.
    seen: Step,
    /// Whether the person set out with Continue: what's given from then on leads to the
    /// next thing without their asking.
    going: bool,
    _watching: [Subscription; 2],
}

/// Opens the setup over `parent`.
pub fn open(parent: &mut Window, cx: &mut App) {
    modal::open_kept(WIDTH, HEIGHT, parent, cx, |_, cx| {
        let desk = desk(cx);
        // The desk looks for the grants for as long as the dialog is up (`setup_shows`).
        desk.update(cx, |desk, cx| desk.setup_shows(true, cx));
        Setup {
            seen: desk.read(cx).step(),
            going: false,
            _watching: [
                cx.observe(&desk, |this: &mut Setup, _, cx| this.moved(cx)),
                cx.on_release(|this: &mut Setup, cx| {
                    (this.desk).update(cx, |desk, cx| desk.setup_shows(false, cx));
                }),
            ],
            desk,
        }
    });
}

/// Opens the setup if it's due: a grant is missing, and it wasn't offered yet since Koyo
/// started.
pub fn offer(window: &mut Window, cx: &mut App) {
    if desk(cx).update(cx, |desk, _| desk.offers_setup()) {
        open(window, cx);
    }
}

impl Setup {
    /// The desk changed: with the person on their way, a grant given leads on to the next,
    /// and the desktop made brings Koyo back in front.
    fn moved(&mut self, cx: &mut Context<Self>) {
        let step = self.desk.read(cx).step();
        if step != self.seen && self.going {
            match step {
                Step::Needs(Grant::ScreenRecording) if self.seen != Step::Starting => {
                    self.ask(Grant::ScreenRecording, cx);
                }
                Step::Ready => cx.activate(true),
                _ => {}
            }
        }
        self.seen = step;
        cx.notify();
    }

    fn ask(&mut self, grant: Grant, cx: &mut Context<Self>) {
        self.going = true;
        self.desk.update(cx, |desk, cx| desk.ask_grant(grant, cx));
    }

    /// A grant's row: its number, a wheel while it's waited for, a tick once it's given.
    fn row(&self, number: usize, grant: Grant, cx: &App, p: Palette) -> Div {
        let (name, why) = match grant {
            Grant::Accessibility => (
                "Accessibility",
                "To make that desktop, and to click and type in the apps on it.",
            ),
            Grant::ScreenRecording => (
                "Screen Recording",
                "To show you the desktop, and to take screenshots for agents.",
            ),
        };
        let desk = self.desk.read(cx);
        let given = desk.grants.is_some_and(|grants| match grant {
            Grant::Accessibility => grants.accessibility,
            Grant::ScreenRecording => grants.screen_recording,
        });
        let waited = self.going && self.seen == Step::Needs(grant);
        let (mark, stands) = if given {
            (Mark::Done, "Allowed")
        } else if waited {
            (Mark::Busy, "Waiting for you…")
        } else {
            (Mark::Next { dim: false }, "")
        };
        modal::step(number, mark, name, why, stands, p)
    }

    /// The way on when macOS hasn't put the host in a list: its app shown in Finder, to be
    /// dragged in. Offered while a grant is waited for.
    fn way_out(&self, cx: &mut Context<Self>, p: Palette) -> Div {
        let line = div()
            .h(px(modal::SMALL_LINE))
            .flex()
            .items_center()
            .text_xs();
        if !(self.going && matches!(self.seen, Step::Needs(_))) {
            return line;
        }
        line.child(
            div()
                .id("agent-desktop-setup-reveal")
                .cursor_pointer()
                .text_color(p.muted)
                .hover(gpui_kit::Styled::underline)
                .child("Not in the list? Show Koyo Agent Desktop in Finder, and drag it in.")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.desk.update(cx, |desk, cx| desk.reveal_host(cx));
                })),
        )
    }

    /// What to do now, in a line or two.
    fn hint(&self, cx: &App) -> String {
        let settings = "switch on Koyo Agent Desktop";
        match (self.seen, self.going) {
            (Step::Needs(grant), false) => {
                let lists = match grant {
                    Grant::Accessibility => "in two lists",
                    Grant::ScreenRecording => "in one more list",
                };
                format!(
                    "Continue takes you to System Settings, where you {settings} {lists}. \
                     This dialog waits for you here."
                )
            }
            (Step::Needs(Grant::Accessibility), true) => format!(
                "In System Settings → Privacy & Security → Accessibility, {settings}. macOS \
                 may ask for your password. The next list opens by itself."
            ),
            (Step::Needs(Grant::ScreenRecording), true) => format!(
                "macOS asks whether it may record the screen: choose Open System Settings. \
                 Then, in Screen & System Audio Recording, {settings}. If macOS offers to \
                 quit and reopen it, either answer is fine."
            ),
            (Step::Starting, _) => "Starting the agent desktop…".to_owned(),
            (Step::Ready, _) => "The agent desktop is ready. Agents can use it from now on, \
                                 and you can watch them, and take over, in its tab."
                .to_owned(),
            (Step::Unavailable, _) => match &self.desk.read(cx).state {
                State::Failed(why) => why.clone(),
                _ => "The agent desktop is off.".to_owned(),
            },
        }
    }
}

impl Render for Setup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let step = self.seen;
        let leave = if step == Step::Ready {
            "Done"
        } else {
            "Not Now"
        };
        let leave = Button::new("agent-desktop-setup-leave", leave, Tone::Plain)
            .on_click(|_, window, cx| modal::dismiss(window, cx));
        let next = match step {
            Step::Needs(grant) => {
                let label = if self.going {
                    "Open System Settings"
                } else {
                    "Continue"
                };
                Some(
                    Button::new("agent-desktop-setup-next", label, Tone::Primary)
                        .on_click(cx.listener(move |this, _, _, cx| this.ask(grant, cx))),
                )
            }
            Step::Starting => {
                Some(Button::new("agent-desktop-setup-next", "Continue", Tone::Primary).busy(true))
            }
            Step::Ready => Some(
                Button::new(
                    "agent-desktop-setup-next",
                    "Show the Desktop",
                    Tone::Primary,
                )
                .on_click(|_, window, cx| {
                    if let Some((_, shell)) = crate::control::main_shell(cx) {
                        shell.update(cx, |shell, cx| shell.desktop_pane(true, cx));
                    }
                    modal::dismiss(window, cx);
                }),
            ),
            Step::Unavailable => None,
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(modal::heading("Set up the agent desktop"))
            .child(div().h(px(INTRO * modal::LINE)).text_color(p.muted).child(
                "Agents open and drive apps on a desktop of their own, shown in a Koyo \
                         tab, while you keep working on yours. macOS asks you to allow two \
                         things for it, once.",
            ))
            .child(self.row(1, Grant::Accessibility, cx, p))
            .child(self.row(2, Grant::ScreenRecording, cx, p))
            .child(
                div()
                    .h(px(HINT * modal::SMALL_LINE))
                    .text_xs()
                    .text_color(p.text)
                    .child(self.hint(cx)),
            )
            .child(self.way_out(cx, p))
            .child(modal::footer(None, p).child(leave).children(next))
    }
}
