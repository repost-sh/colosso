//! The pinned pages' region (`docs/sidebar.md` 11): the tiles at the sidebar's head, or the buttons
//! in a pill on the top rail, as **Tabs** puts them. Each says how its region, its slots and its
//! zone look; this is what they do: a page's tab or a loose page let go on the region is pinned
//! where the slots made room for it, a slot dragged among them moves there, and the zone that says
//! where one goes while none is pinned is put away by its ×.

use std::rc::Rc;

use gpui_kit::base::ElementExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, Context, Div, Hsla, InteractiveElement as _, MouseButton, Pixels,
    Stateful, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::model::{Front, Pinned};
use crate::shell::{Arrange, Dragged, DraggedPane, Landing, Shell, TileGrid};

/// What the zone that says where a pinned page goes says, in the sidebar and on the rail: to let a
/// page go there while `taking` one, and to drag one there otherwise.
pub fn pinned_zone_says(taking: bool) -> &'static str {
    if taking {
        "Drop to pin"
    } else {
        "Drag a page here to pin it"
    }
}

/// `button`, the zone's ×, in the sidebar and on the rail: it says it hides the zone, and pressed,
/// puts the zone away, the press going no further.
pub fn hides_pinned_zone(button: Stateful<Div>) -> Stateful<Div> {
    button
        .tooltip(|window, cx| Tooltip::new("Hide").build(window, cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| Shell::dismiss_pinned_zone(cx))
}

impl Shell {
    /// The pinned pages' region, known as `name`, for a layout to lay out and fill with its
    /// grid or its zone; none while no page is pinned, the zone was put away and nothing it
    /// takes is in hand. `taking` says a page's tab or a loose page's row is in hand, off its
    /// list: the region lights in `lit` under it, and takes it, pinned where the slots made room
    /// for it, or after them.
    pub(crate) fn pinned_region(
        &self,
        name: &'static str,
        (taking, lit): (bool, Hsla),
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        if !self.lists_pinned(taking, cx) {
            return None;
        }
        let (tabs, pins) = (self.tab_hand.clone(), self.pinned_hand.clone());
        let note = move |bounds: Bounds<Pixels>, _: &mut Window, _: &mut App| {
            tabs.taken_at(bounds);
            pins.takes_at(bounds);
        };
        let region = div()
            .id(name)
            .debug_selector(move || name.to_owned())
            .flex_shrink_0()
            .drag_over::<DraggedPane>(move |style, held, _, _| match held.what {
                Dragged::Tab(..) | Dragged::Loose if taking => style.bg(lit),
                _ => style,
            })
            .on_drop(cx.listener(|this, held: &DraggedPane, _, cx| this.pin_dropped(held, cx)))
            .on_prepaint(note);
        #[cfg(feature = "visual-check")]
        let region = region.on_prepaint(crate::visual::noting(name.into()));
        Some(region)
    }

    /// The pinned pages' grid, known as `name` and laid out as `arrange` says, a slot each as
    /// `slot` draws the page at its place, lit when it's the one shown; none while none is
    /// pinned. A slot dragged over another moves there among them. While `taking`, they make
    /// room for what's in hand where it would land, that slot lit as `landing` says.
    pub(crate) fn pinned_grid(
        &self,
        (name, arrange): (&'static str, (Arrange, Pixels)),
        (taking, landing): (bool, Landing),
        slot: impl Fn((usize, &Pinned), bool, &mut Context<Self>) -> AnyElement,
        cx: &mut Context<Self>,
    ) -> Option<TileGrid> {
        let pinned = self.workspace.pinned();
        if pinned.is_empty() {
            return None;
        }
        let front = self.workspace.front();
        let slots = (pinned.iter().enumerate())
            .map(|(at, pinned)| {
                let shown = matches!(front, Some(Front::Pinned(shown)) if shown == at);
                (pinned.key(), slot((at, pinned), shown, cx))
            })
            .collect();
        let on_drop = cx.listener(|this, &(from, to): &(usize, usize), _, cx| {
            this.move_pinned(from, to, cx);
        });
        let hand = self.pinned_hand.clone();
        let grid = TileGrid::new(name, slots, arrange, hand, Rc::new(on_drop));
        Some(grid.when(taking, |grid| grid.taking(landing)))
    }
}
