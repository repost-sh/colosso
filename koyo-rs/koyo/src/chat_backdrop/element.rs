//! What a chat draws its backdrop with. Cropping is placing: the screen's picture is painted as
//! big as the screen, where the screen lies relative to the window, and clipped to the chat's
//! pane, so only the pane's part reaches the GPU, and two panes side by side meet without a
//! seam and show one dither. Moving or resizing the window only moves where it's placed: GPUI
//! draws the whole window again as the window moves (`Window::bounds_changed`). The renderer
//! dithers it, sets its opacity and fades it down the pane as it draws it (`Texture::mark`).

use gpui_kit::{
    App, Bounds, ContentMask, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Position, Size, Style, Window, point, px, relative,
};

use super::make::Recipe;
use super::screens::{self, Hold};
use crate::settings::ChatBackdrop;

/// The screen's wallpaper behind a chat, filling its parent, which must be `relative`, looking
/// as `settings` say.
pub struct Picture {
    pub settings: ChatBackdrop,
}

impl IntoElement for Picture {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Picture {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some("chat-backdrop".into())
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
        let mut style = Style {
            position: Position::Absolute,
            ..Style::default()
        };
        style.inset.top = px(0.).into();
        style.inset.left = px(0.).into();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some((display, screen)) = screen(window, cx) else {
            return;
        };
        // Held for as long as the chat draws it: dropped with the element's state the first
        // frame that doesn't.
        if let Some(id) = id {
            window.with_element_state::<Hold, _>(id, |held, _| match held {
                Some(held) if held.display == display => ((), held),
                _ => ((), screens::hold(display, cx)),
            });
        }
        let recipe = recipe(
            display,
            screen.size,
            window.scale_factor(),
            self.settings,
            cx,
        );
        let Some(placeholder) = screens::picture(display, recipe, self.settings, cx) else {
            return;
        };
        // Clipped to the pane, which the renderer fades down; it draws only what's also on the
        // screen.
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_surface(screen, placeholder);
        });
    }
}

/// The screen holding most of `window`, as GPUI's display ID and where it lies relative to the
/// window.
fn screen(window: &Window, cx: &App) -> Option<(u64, Bounds<Pixels>)> {
    #[cfg(feature = "visual-check")]
    if let Some(stand_in) = super::visual::screen() {
        return Some(stand_in);
    }
    let display = window.display(cx)?;
    // The window's bounds are its screen's, from the screen's top left.
    let origin = window.bounds().origin;
    let screen = Bounds::new(point(-origin.x, -origin.y), display.bounds().size);
    Some((u64::from(display.id()), screen))
}

/// What screen `display`'s picture is made from, `size` in points at `scale`, as `backdrop`
/// says; none while it has no wallpaper to show.
fn recipe(
    display: u64,
    size: Size<Pixels>,
    scale: f32,
    backdrop: ChatBackdrop,
    cx: &mut App,
) -> Option<Recipe> {
    let wallpaper = screens::wallpaper(display, cx)?;
    Some(Recipe {
        wallpaper,
        screen: (
            f64::from(size.width.as_f32()),
            f64::from(size.height.as_f32()),
        ),
        scale: f64::from(scale),
        quality: backdrop.quality,
    })
}
