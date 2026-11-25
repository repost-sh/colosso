//! The sidebar's foot (`docs/layout.md`, `docs/sidebar.md` 7): the Claude accounts' button, the
//! spaces in the middle and a + that starts a worktree, adds a project or makes a space.
//!
//! Each space is a mark in the text's colour. The space shown stretches into a capsule at full
//! strength; the others are dots, dimmed, brightening while the pointer's on them, which also
//! brings up the space's name on glass over its dot. Switching moves both at once: the capsule
//! shrinks back to a dot as the next one grows, its width going a little past and settling, the
//! strength easing, so the eye follows the switch rather than seeing it jump.
//!
//! A space that isn't shown says what its projects would: an indigo dot by its mark while something
//! in it waits for the person, a spinner while something works.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::base::motion::{Spring, Transition, spring, transition};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{
    AnyElement, App, Axis, Bounds, Context, Div, ElementId, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, ParentElement as _, Pixels, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, div, px, relative,
};

use super::hover::Hovered;
use crate::chat::{Activity, spinner_of};
use crate::pane::dot;
use crate::projects::{AddProject, NewWorktree};
use crate::settings;
use crate::shell::spaces::{NewSpace, space_menu};
use crate::shell::{List, Shell, icon_button};
use crate::theme;

/// The + button's width, which the accounts' button at the left of the foot keeps too, so the
/// spaces sit in the middle.
const PLUS: f32 = 24.;
/// A space's mark: as high as a dot is wide; the space shown is a capsule this wide.
const DOT: f32 = 7.;
const CAPSULE: f32 = 20.;
/// The room round a mark that takes the pointer.
const HIT: f32 = 22.;
/// How strong a mark is: the space shown, one the pointer's on, and the rest.
const SHOWN: f32 = 1.;
const POINTED: f32 = 0.75;
const RESTING: f32 = 0.4;
/// How a mark changes width: on a spring a little under critical damping, so the capsule goes a
/// hair past and settles back, as the Arc browser's does, and keeps its speed if the space changes
/// again mid-way.
const STRETCH: Spring = Spring::new(Duration::from_millis(300)).with_damping(0.65);
/// How long a mark takes to change strength.
const FADE: Duration = Duration::from_millis(220);
/// The dot by a mark whose space has something waiting for the person: smaller than a row's,
/// as the mark is.
const PILL: f32 = 5.;
/// Where that dot is: from the mark's middle across, and from the top of its room down.
const PILL_AT: (f32, f32) = (2.5, 3.5);
/// The spinner by a mark whose space has something at work: where the dot would be, their
/// middles one, and half as wide again, so its ring reads at that size.
const RING: f32 = PILL * 1.5;

pub fn bar(shell: &Shell, window: &mut Window, cx: &mut Context<Shell>) -> impl IntoElement {
    let settings = settings::get(cx);
    let shown = settings.space;
    let no_project = settings
        .shown_space()
        .is_none_or(|space| space.projects.is_empty());
    let spaces: Vec<u64> = settings.spaces.iter().map(|space| space.id).collect();
    let marks: Vec<(u64, AnyElement)> = (spaces.into_iter().enumerate())
        .map(|(index, id)| {
            let state = (index == shown, shell.sidebar_hover.on == Some(Hovered::Space(id)));
            let activity = if index == shown {
                Activity::Quiet
            } else {
                shell.space_activity(index, cx)
            };
            let note = shell.sidebar_hover.noting(Hovered::Space(id));
            let mark = mark(shell, (index, id), state, activity, note, window, cx);
            (id, mark.into_any_element())
        })
        .collect();
    let plus = div()
        .w(px(PLUS))
        .flex()
        .justify_center()
        .on_mouse_down(
            MouseButton::Left,
            move |event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                NativeMenu::new()
                    .menu_with_disabled("New Worktree…", no_project, Box::new(NewWorktree))
                    .menu("New Project…", Box::new(AddProject))
                    .menu("New Space…", Box::new(NewSpace))
                    .show(event.position, window, cx);
            },
        )
        .child(icon_button("sidebar-foot-plus", IconName::Plus, "New", cx).small());
    div()
        .debug_selector(|| "sidebar-foot".to_owned())
        .h(px(34.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .child(div().w(px(PLUS)).children(crate::accounts::button(cx)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .child(super::mark_drag::row(shell, marks, cx)),
        )
        .child(plus)
}

/// What the mark of the space at `index`, known by `id`, says of its `activity`
/// (`docs/sidebar.md` 4): the dot or the spinner, in one place, up and right of the mark's
/// middle, where the mark's dot is and the capsule's middle would be. It's laid over the mark,
/// taking no room, so nothing moves as it comes and goes. It shows only by a mark that's `a_dot`:
/// on a space just left, once the capsule has shrunk back.
fn pill(
    (index, id): (usize, u64),
    activity: Activity,
    a_dot: bool,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let waits = activity == Activity::Attention && a_dot;
    let fade = transition(
        ElementId::from(("space-mark-pill", id)),
        if waits { 1. } else { 0. },
        Transition::new(FADE),
        window,
        cx,
    );
    if !a_dot {
        return None;
    }
    let (sign, wide) = match activity {
        Activity::Quiet => return None,
        Activity::Working => (spinner_of(format!("space-{id}"), RING, cx), RING),
        Activity::Attention => {
            let waiting = theme::palette(cx).waiting;
            let dot = dot(waiting).size(px(PILL)).opacity(fade.clamp(0., 1.));
            (dot, PILL)
        }
    };
    let out = (wide - PILL) / 2.;
    let pill = div()
        .debug_selector(move || format!("space-mark-pill-{index}"))
        .absolute()
        .top(px(PILL_AT.1 - out))
        .left(relative(0.5))
        .ml(px(PILL_AT.0 - out))
        .child(sign);
    Some(pill.into_any_element())
}

/// The mark of the space at `index`, known by `id` whatever its place, `(shown, pointed)` at,
/// moving to what that makes it, with what it says of its space's `activity`; `note` hears where
/// it's drawn, for its name's card. Clicked, it shows its space; right-clicked, it has its
/// space's menu; dragged sideways, it's taken up by the marks' row (`mark_drag.rs`).
fn mark<F: FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static>(
    shell: &Shell,
    (index, id): (usize, u64),
    (shown, pointed): (bool, bool),
    activity: Activity,
    note: F,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let width = spring(
        ElementId::from(("space-mark-width", id)),
        if shown { CAPSULE } else { DOT },
        STRETCH,
        window,
        cx,
    );
    let pill = pill((index, id), activity, width < DOT + 1., window, cx);
    let strength = transition(
        ElementId::from(("space-mark-strength", id)),
        match (shown, pointed) {
            (true, _) => SHOWN,
            (false, true) => POINTED,
            (false, false) => RESTING,
        },
        Transition::new(FADE),
        window,
        cx,
    );
    let colour = theme::palette(cx).text;
    let mark = div()
        .id(("space-mark", id))
        .debug_selector(move || format!("space-mark-{index}"))
        .relative()
        .h(px(HIT))
        .px(px((HIT - DOT) / 2.))
        .flex()
        .items_center()
        .cursor_pointer()
        .child(
            div()
                .h(px(DOT))
                .w(px(width.max(DOT)))
                .rounded_full()
                .bg(colour)
                .opacity(strength.clamp(0., 1.)),
        )
        .children(pill)
        .on_prepaint(note)
        .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
            this.hover(Hovered::Space(id), *hovered, window, cx);
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.press_hovered(cx);
                if this.workspace.shown_in_every_space() {
                    cx.stop_propagation();
                }
            }),
        )
        .on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            space_menu(id, cx).show(event.position, window, cx);
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(index) = settings::get(cx).space_index(id) {
                this.slide_to_space(index, cx);
            }
        }));
    shell.mark_hand.lift(mark, (List::Spaces, id, index), Axis::Horizontal)
}
