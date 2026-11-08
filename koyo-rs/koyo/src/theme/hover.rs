//! What's hovered easing in and out: a `transition` towards hovered, its hovered state kept
//! with the element as the transition's value is, so a view needn't keep a field, and a
//! listener of its own, for each thing of its that fades under the pointer.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::{App, ElementId, Window};

/// How far what `id` names has eased towards hovered, 0 to 1 over `fade`, and the hover
/// listener that moves it, for the element's `on_hover`. Both live with the element, so the
/// view showing it redraws as the state changes, as it does for a transition.
pub fn eased(
    id: impl Into<ElementId>,
    fade: Duration,
    window: &mut Window,
    cx: &mut App,
) -> (f32, impl Fn(&bool, &mut Window, &mut App) + 'static) {
    let id = id.into();
    let state = ElementId::NamedChild(Arc::new(id.clone()), "hovered".into());
    let hovered = window.use_keyed_state(state, cx, |_, _| false);
    let target = if *hovered.read(cx) { 1. } else { 0. };
    let strength = transition(id, target, Transition::new(fade), window, cx);
    let listener = move |now: &bool, _: &mut Window, cx: &mut App| {
        hovered.update(cx, |hovered, cx| {
            if *hovered != *now {
                *hovered = *now;
                cx.notify();
            }
        });
    };
    (strength, listener)
}
