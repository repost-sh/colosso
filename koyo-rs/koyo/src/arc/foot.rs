//! The Arc sidebar's foot (`docs/arc-sidebar.md` 7): the spaces in the middle and a + that starts
//! a worktree, adds a project or makes a space.
//!
//! Each space is a mark in its colour. The space shown stretches into a capsule at full strength;
//! the others are dots, dimmed, brightening while the pointer's on them, which also brings up the
//! space's name on glass over its dot. Switching moves both at once: the capsule shrinks back to a
//! dot as the next one grows, its width going a little past and settling, the strength easing, so
//! the eye follows the switch rather than seeing it jump.
//!
//! A space that isn't shown says what its tiles would: an indigo dot by its mark while something in
//! it waits for the person, the tiles' spinner while something works.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::base::motion::{Spring, Transition, spring, transition};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{
    AnyElement, App, Bounds, Context, ElementId, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, ParentElement as _, Pixels, StatefulInteractiveElement as _, Styled as _,
    Window, div, px, relative,
};

use super::actions::NewSpace;
use super::hover::Hovered;
use super::space_colour;
use crate::chat::{Activity, spinner_of};
use crate::pane::dot;
use crate::projects::{AddProject, NewWorktree};
use crate::settings;
use crate::shell::{Shell, icon_button};
use crate::theme;

/// The + button's width, which the left of the foot keeps too, so the spaces sit in the middle.
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
/// hair past and settles back, as Arc's does, and keeps its speed if the space changes again
/// mid-way.
const STRETCH: Spring = Spring::new(Duration::from_millis(300)).with_damping(0.65);
/// How long a mark takes to change strength.
const FADE: Duration = Duration::from_millis(220);
/// The dot by a mark whose space has something waiting for the person: smaller than a tile's,
/// as the mark is.
const PILL: f32 = 5.;
/// Where that dot is: from the mark's middle across, and from the top of its room down.
const PILL_AT: (f32, f32) = (2.5, 3.5);
/// The spinner by a mark whose space has something at work: where the dot would be, their
/// middles one, and half as wide again, so its ring reads at that size.
const RING: f32 = PILL * 1.5;

pub fn bar(shell: &Shell, window: &mut Window, cx: &mut Context<Shell>) -> impl IntoElement {
    let settings = settings::get(cx);
    let (shown, count) = (settings.space, settings.spaces.len());
    let no_project = settings
        .shown_space()
        .is_none_or(|space| space.projects.is_empty());
    let marks: Vec<AnyElement> = (0..count)
        .map(|index| {
            let state = (
                index == shown,
                shell.arc_hover.on == Some(Hovered::Space(index)),
            );
            // The space shown says it on its tiles.
            let activity = if index == shown {
                Activity::Quiet
            } else {
                shell.space_activity(index, cx)
            };
            let note = shell.arc_hover.noting(Hovered::Space(index));
            mark(index, state, activity, note, window, cx).into_any_element()
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
        .child(icon_button("arc-plus", IconName::Plus, "New", cx).small());
    div()
        .debug_selector(|| "arc-foot".to_owned())
        .h(px(34.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .child(div().w(px(PLUS)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .children(marks),
        )
        .child(plus)
}

/// What a mark says of its space's `activity` (`docs/arc-sidebar.md` 7): the dot or the spinner,
/// in one place, up and right of the mark's middle, where the mark's dot is and the capsule's
/// middle would be. It's laid over the mark, taking no room, so nothing moves as it comes and
/// goes. It shows only by a mark that's `a_dot`: on a space just left, once the capsule has
/// shrunk back.
fn pill(
    index: usize,
    activity: Activity,
    a_dot: bool,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let waits = activity == Activity::Attention && a_dot;
    let fade = transition(
        ElementId::from(("arc-space-pill", index)),
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
        // As strong as a tile's, and never fading: the spinner is a view, drawn again only as
        // it turns, so a fade round it would stay where its first frame had it.
        Activity::Working => (spinner_of(format!("space-{index}"), RING, cx), RING),
        Activity::Attention => {
            let waiting = theme::palette(cx).waiting;
            let dot = dot(waiting).size(px(PILL)).opacity(fade.clamp(0., 1.));
            (dot, PILL)
        }
    };
    // Whichever it is, its middle is where the dot's is.
    let out = (wide - PILL) / 2.;
    let pill = div()
        .debug_selector(move || format!("arc-space-pill-{index}"))
        .absolute()
        .top(px(PILL_AT.1 - out))
        .left(relative(0.5))
        .ml(px(PILL_AT.0 - out))
        .child(sign);
    Some(pill.into_any_element())
}

/// The mark of the space at `index`, `(shown, pointed)` at, moving to what that makes it, with
/// what it says of its space's `activity`; `note` hears where it's drawn, for its name's card.
fn mark<F: FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static>(
    index: usize,
    (shown, pointed): (bool, bool),
    activity: Activity,
    note: F,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<F> {
    let width = spring(
        ElementId::from(("arc-space-width", index)),
        if shown { CAPSULE } else { DOT },
        STRETCH,
        window,
        cx,
    );
    // A dot, give or take what the spring is still settling: the space shown says nothing here.
    let pill = pill(index, activity, width < DOT + 1., window, cx);
    let strength = transition(
        ElementId::from(("arc-space-strength", index)),
        match (shown, pointed) {
            (true, _) => SHOWN,
            (false, true) => POINTED,
            (false, false) => RESTING,
        },
        Transition::new(FADE),
        window,
        cx,
    );
    let colour = space_colour(index, cx);
    div()
        .id(("arc-space", index))
        .debug_selector(move || format!("arc-space-{index}"))
        .relative()
        .h(px(HIT))
        .px(px((HIT - DOT) / 2.))
        .flex()
        .items_center()
        .cursor_pointer()
        .child(
            div()
                .h(px(DOT))
                // Going past its end on the way back could leave it narrower than a dot.
                .w(px(width.max(DOT)))
                .rounded_full()
                .bg(colour)
                .opacity(strength.clamp(0., 1.)),
        )
        .children(pill)
        .on_prepaint(note)
        .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
            this.hover(Hovered::Space(index), *hovered, window, cx);
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.press_hovered(cx)),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.slide_to_space(index, cx)))
}
