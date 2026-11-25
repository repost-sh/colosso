//! The sidebar's scrolling rows fading as they reach an edge there's more past, so none is cut off:
//! whole at an end they're scrolled to, and the further from it the more of the fade there is. The
//! rows themselves fade, as they're on glass no colour could be laid over.

use gpui_kit::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, ScrollHandle, Window, px,
};

/// How far in from an edge the rows come up from nothing to whole.
pub(super) const FADE: f32 = 24.;

/// `scrolled`, the element `scroll` tracks, fading at its top and its bottom.
pub(super) fn edge_fade(scrolled: impl IntoElement, scroll: &ScrollHandle) -> EdgeFade {
    EdgeFade {
        scrolled: scrolled.into_any_element(),
        scroll: scroll.clone(),
    }
}

/// How far in the rows fade from their top and their bottom, scrolled `offset` down of `most`.
fn fades(offset: Pixels, most: Pixels) -> (Pixels, Pixels) {
    let fade = |past: Pixels| past.clamp(px(0.), px(FADE));
    (fade(offset), fade(most - offset))
}

pub(super) struct EdgeFade {
    scrolled: AnyElement,
    scroll: ScrollHandle,
}

impl Element for EdgeFade {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.scrolled.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.scrolled.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let (top, bottom) = fades(-self.scroll.offset().y, self.scroll.max_offset().y);
        window.with_edge_fade(bounds, top, bottom, |window| self.scrolled.paint(window, cx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_fade_at_an_edge_as_far_as_theres_more_past_it() {
        let at = |offset: f32, most: f32| fades(px(offset), px(most));
        assert_eq!(at(0., 0.), (px(0.), px(0.)), "they fit");
        assert_eq!(at(0., 300.), (px(0.), px(FADE)), "at the top");
        assert_eq!(at(10., 300.), (px(10.), px(FADE)), "just off it");
        assert_eq!(at(150., 300.), (px(FADE), px(FADE)), "between");
        assert_eq!(at(300., 300.), (px(FADE), px(0.)), "at the bottom");
        assert_eq!(at(-40., 300.), (px(0.), px(FADE)), "pulled past the top");
    }
}
