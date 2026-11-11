//! Setting up Koyo's simulators (`docs/devices.md` 10): a dialog that takes the person through
//! what they need, one step at a time, as the agent desktop's does: Xcode 27 or later, the one
//! in use; its components; an iOS simulator system; a device of Koyo's own; its first start.
//! Each row flips the moment its state does. While the dialog is open and Koyo in front, it
//! looks again every two seconds and the moment Koyo comes forward; never behind it. Nothing
//! is run before the person presses Continue.

use std::time::{Duration, Instant};

use gpui_kit::{
    Action, App, AsyncApp, Context, Div, Entity, FocusHandle, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, Styled as _, Subscription, Task, WeakEntity, Window, div, px,
};

use super::needs::{Need, Xcode};
use super::{Devices, Link, Standing, devices};
use crate::modal::{self, Button, Mark, Tone};
use crate::theme;

pub mod device;
mod words;

pub use words::models_menu;

const WIDTH: f32 = 500.;
const INTRO: f32 = 2.;
const HINT: f32 = 3.;
const GAP: f32 = 12.;
#[expect(clippy::cast_precision_loss, reason = "five rows")]
const HEIGHT: f32 = 2. * modal::PADDING
    + modal::HEADING_LINE
    + INTRO * modal::LINE
    + words::ROWS.len() as f32 * modal::STEP
    + HINT * modal::SMALL_LINE
    + modal::FIELD
    + modal::FOOTER
    + 6. * GAP;
/// How often the dialog looks again while Koyo is in front.
const LOOK_EVERY: Duration = Duration::from_secs(2);
/// Xcode in the App Store.
const APP_STORE: &str = "macappstore://apps.apple.com/app/id497799835";

/// The model the person chose from the device step's menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = devices, no_json)]
struct ChooseModel(String);

/// Where the person is; the dialog's rows read from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// What the Mac has isn't known yet.
    Looking,
    Needs(Need),
    /// Xcode's components being installed, by the first `simctl` call.
    Installing,
    Making,
    Starting,
    Ready,
    /// The device made didn't start: its own words.
    Failed,
    /// Nothing was missing.
    Done,
}

impl Step {
    /// The row a step is at: 0 for Xcode to 4 for the first start; past them all, 5.
    fn row(self) -> usize {
        match self {
            Step::Looking | Step::Needs(Need::Xcode) => 0,
            Step::Needs(Need::Components) | Step::Installing => 1,
            Step::Needs(Need::System) => 2,
            Step::Needs(Need::Device) | Step::Making => 3,
            Step::Starting | Step::Failed => 4,
            Step::Ready | Step::Done => 5,
        }
    }

    /// Whether the person's to do is outside Koyo, for the dialog to look again while it waits.
    fn outside(self) -> bool {
        matches!(self, Step::Needs(Need::Xcode | Need::Components | Need::System))
    }
}

/// The device the dialog makes.
enum Device {
    Choosing,
    Making,
    /// Not made: why.
    Failed(String),
    Made(Entity<Link>),
}

/// The first `simctl` call, which installs Xcode's components.
enum Install {
    NotAsked,
    Running,
    /// What it said when it went wrong; nothing when it went well.
    Ended(String),
}

pub struct Setup {
    devices: Entity<Devices>,
    focus: FocusHandle,
    /// The model chosen from the menu; else the newest iPhone.
    model: Option<String>,
    device: Device,
    install: Install,
    /// The step last seen: Ready brings Koyo forward once.
    seen: Step,
    _looking: Task<()>,
    _watching: Vec<Subscription>,
}

/// Opens the setup over `parent`, and looks at the Mac afresh.
pub fn open(parent: &mut Window, cx: &mut App) {
    modal::open_kept(WIDTH, HEIGHT, parent, cx, |window, cx| {
        let devices = devices(cx);
        devices.update(cx, Devices::look_again);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Setup {
            focus,
            model: None,
            device: Device::Choosing,
            install: Install::NotAsked,
            seen: Step::Looking,
            // The Mac looked at again while the person's to do is outside Koyo; the time so far
            // of a first start moving on.
            _looking: cx.spawn(async move |this, cx| {
                let waiting = |this: &Setup, cx: &App| this.step(cx).outside();
                look_along(this, cx, waiting, |this, due, cx| {
                    if due {
                        this.devices.update(cx, Devices::look_along);
                    }
                    if this.step(cx) == Step::Starting {
                        cx.notify();
                    }
                })
                .await;
            }),
            _watching: vec![cx.observe(&devices, |this: &mut Setup, _, cx| this.moved(cx))],
            devices,
        }
    });
}

/// Opens the setup if it's due: something is missing, it wasn't offered since Koyo started,
/// and the person is at Koyo's window. A tab or the picker asks; never an agent's tool.
pub fn offer(window: &mut Window, cx: &mut App) {
    if window.is_window_active() && devices(cx).update(cx, |devices, _| devices.offers_setup()) {
        open(window, cx);
    }
}

/// A setup dialog's clock: every second `tick`, told whether to look at the Mac again: with
/// Koyo in front and the person `waiting` on something done outside Koyo, every two seconds,
/// and at once when Koyo comes forward; never behind it. Ends with the dialog.
pub(super) async fn look_along<V: 'static>(
    this: WeakEntity<V>,
    cx: &mut AsyncApp,
    waiting: impl Fn(&V, &App) -> bool,
    tick: impl Fn(&mut V, bool, &mut Context<V>),
) {
    let mut last = Instant::now();
    let mut in_front = true;
    loop {
        cx.background_executor().timer(Duration::from_secs(1)).await;
        let Ok(()) = this.update(cx, |this, cx| {
            let front = cx.active_window().is_some();
            let due = front && waiting(this, cx) && (!in_front || last.elapsed() >= LOOK_EVERY);
            if due {
                last = Instant::now();
            }
            in_front = front;
            tick(this, due, cx);
        }) else {
            return;
        };
    }
}

/// The last line of what `simctl` said went wrong, short.
fn last_line(said: &str) -> String {
    let last = said.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or_default();
    last.trim().chars().take(160).collect()
}

impl Setup {
    pub(super) fn step(&self, cx: &App) -> Step {
        match &self.device {
            Device::Made(link) => {
                return match link.read(cx).standing() {
                    Standing::Ready(_) | Standing::Locked(_) => Step::Ready,
                    Standing::Ended { .. } | Standing::Failed(_) => Step::Failed,
                    Standing::Off | Standing::Starting(_) => Step::Starting,
                };
            }
            Device::Making => return Step::Making,
            Device::Choosing | Device::Failed(_) => {}
        }
        let devices = self.devices.read(cx);
        match (devices.need(), &devices.found) {
            (Some(Need::Components), _) if matches!(self.install, Install::Running) => Step::Installing,
            (Some(need), _) => Step::Needs(need),
            (None, None) => Step::Looking,
            (None, Some(_)) => Step::Done,
        }
    }

    /// The device made, once it is.
    pub(super) fn made(&self) -> Option<&Entity<Link>> {
        match &self.device {
            Device::Made(link) => Some(link),
            _ => None,
        }
    }

    /// The model the device step makes: the person's choice, else the newest iPhone.
    fn model(&self, cx: &App) -> Option<String> {
        let default = || Some(self.devices.read(cx).found.as_ref()?.default_model()?.name.clone());
        self.model.clone().or_else(default)
    }

    /// What the dialog watches changed: with everything ready, Koyo comes forward.
    fn moved(&mut self, cx: &mut Context<Self>) {
        let step = self.step(cx);
        if step != self.seen && step == Step::Ready {
            cx.activate(true);
        }
        self.seen = step;
        cx.notify();
    }

    /// Has the first `simctl` call install Xcode's components, and looks again once it ends.
    fn install(&mut self, cx: &mut Context<Self>) {
        let Some(set) = super::set_dir() else {
            return;
        };
        self.install = Install::Running;
        let ran = cx.background_executor().spawn(async move {
            super::simulators::list(&set).err().map(|why| last_line(&why)).unwrap_or_default()
        });
        cx.spawn(async move |this, cx| {
            let said = ran.await;
            let _ = this.update(cx, |this, cx| {
                this.install = Install::Ended(said);
                this.devices.update(cx, Devices::look_again);
                this.moved(cx);
            });
        })
        .detach();
        self.moved(cx);
    }

    /// Makes the device chosen, and opens and starts it in a tab of its own, shown.
    pub(super) fn make(&mut self, cx: &mut Context<Self>) {
        self.device = Device::Making;
        let model = self.model(cx);
        let this = cx.entity().downgrade();
        self.devices.update(cx, |registry, cx| {
            registry.make(model, cx, move |made, cx| {
                let opened = made.and_then(|made| {
                    let chosen = super::Chosen::from(&made);
                    let (_, shell) = crate::control::main_shell(cx).ok_or("Koyo's window is closed.")?;
                    shell.update(cx, |shell, cx| shell.device_pane(Some(chosen), true, cx));
                    let link = devices(cx).read(cx).existing(&made.udid).ok_or_else(|| "Its tab didn't open.".to_owned())?;
                    link.update(cx, Link::start);
                    Ok(link)
                });
                let _ = this.update(cx, |this, cx| match opened {
                    Ok(link) => this.starts(link, cx),
                    Err(why) => {
                        this.device = Device::Failed(why);
                        this.moved(cx);
                    }
                });
            });
        });
        self.moved(cx);
    }

    /// The device made, `link` its, starting: the dialog follows it to ready.
    pub fn starts(&mut self, link: Entity<Link>, cx: &mut Context<Self>) {
        self._watching.push(cx.observe(&link, |this, _, cx| this.moved(cx)));
        self.device = Device::Made(link);
        self.moved(cx);
    }

    /// A step's row, by where the dialog is.
    fn row(&self, index: usize, step: Step, cx: &App) -> Div {
        let at = step.row();
        let (name, why) = words::ROWS[index];
        let busy = matches!(step, Step::Looking | Step::Installing | Step::Making | Step::Starting);
        let (mark, stands) = if index < at {
            (Mark::Done, self.there(index, cx))
        } else if index == at && busy {
            (Mark::Busy, String::new())
        } else {
            (Mark::Next { dim: index > at }, String::new())
        };
        modal::step(index + 1, mark, name, why, &stands, theme::palette(cx))
    }

    /// What a step done found: Xcode's version, the system, the device.
    fn there(&self, index: usize, cx: &App) -> String {
        let devices = self.devices.read(cx);
        let found = devices.found.as_ref();
        match index {
            0 => match found.map(|found| &found.xcode) {
                Some(Xcode::Ready(version)) => format!("Xcode {version}"),
                _ => String::new(),
            },
            1 => "Installed".into(),
            2 => (found.and_then(|found| found.systems.first()))
                .map(|system| format!("{}, {}", system.name, crate::viewer::size_on_disk(system.bytes)))
                .unwrap_or_default(),
            3 => match (self.made(), &devices.simulators) {
                (Some(link), _) => link.read(cx).name.clone(),
                (None, Some(Ok(all))) if all.len() == 1 => all[0].name.clone(),
                (None, Some(Ok(all))) => format!("{} devices", all.len()),
                _ => String::new(),
            },
            _ => "Ready".into(),
        }
    }

    /// The buttons at the bottom: the way out, and the way on from `step`.
    fn buttons(&self, step: Step, cx: &mut Context<Self>) -> Vec<Button> {
        let again = |tone| {
            Button::new("device-setup-again", "Check Again", tone)
                .on_click(|_, _, cx| devices(cx).update(cx, Devices::look_again))
        };
        let next = |label: &'static str| Button::new("device-setup-next", label, Tone::Primary);
        let (leave, tone) = match step {
            Step::Ready | Step::Done => ("Done", Tone::Primary),
            _ => ("Not Now", Tone::Plain),
        };
        let mut buttons = vec![Button::new("device-setup-leave", leave, tone).on_click(|_, window, cx| modal::dismiss(window, cx))];
        let found = self.devices.read(cx).found.as_ref();
        let app_store = found.is_some_and(|found| matches!(found.xcode, Xcode::Missing | Xcode::Older(_)));
        match step {
            Step::Needs(Need::Xcode) if app_store => {
                buttons.push(again(Tone::Plain));
                buttons.push(next("Open the App Store").on_click(|_, _, cx| cx.open_url(APP_STORE)));
            }
            Step::Needs(Need::Components) if matches!(self.install, Install::NotAsked) => {
                buttons.push(next("Continue").on_click(cx.listener(|this, _, _, cx| this.install(cx))));
            }
            Step::Needs(Need::Device) => {
                buttons.push(next("Continue").on_click(cx.listener(|this, _, _, cx| this.make(cx))));
            }
            Step::Needs(_) => buttons.push(again(Tone::Primary)),
            Step::Looking | Step::Installing | Step::Making | Step::Starting => buttons.push(next("Continue").busy(true)),
            Step::Failed => {
                let start = cx.listener(|this, _, _, cx| {
                    if let Some(link) = this.made() {
                        link.update(cx, Link::start);
                    }
                });
                buttons.push(next("Start Again").on_click(start));
            }
            Step::Ready | Step::Done => {}
        }
        buttons
    }
}

impl Render for Setup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let step = self.step(cx);
        let rows: Vec<Div> = (0..words::ROWS.len()).map(|index| self.row(index, step, cx)).collect();
        let chooser = (step == Step::Needs(Need::Device)).then(|| words::chooser(self, cx));
        let error = match &self.device {
            Device::Failed(why) => Some(why.clone().into()),
            _ => None,
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, ChooseModel(model): &ChooseModel, _, cx| {
                this.model = Some(model.clone());
                cx.notify();
            }))
            .child(modal::heading("Set up Koyo's simulators"))
            .child(div().h(px(INTRO * modal::LINE)).text_color(p.muted).child(
                "iPhones and iPads that you and agents use in a Koyo tab, kept apart from \
                 Xcode's own simulators.",
            ))
            .children(rows)
            .child(div().h(px(HINT * modal::SMALL_LINE)).text_xs().text_color(p.text).child(words::hint(self, step, cx)))
            .child(div().h(px(modal::FIELD)).flex().items_center().children(chooser))
            .child(modal::footer(error, p).children(self.buttons(step, cx)))
    }
}
