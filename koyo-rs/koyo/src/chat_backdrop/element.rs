//! What a chat draws its backdrop with. Cropping is placing: the screen's picture is painted as
//! big as the screen, where the screen lies relative to the window, and clipped to the chat's
//! pane, so only the pane's part reaches the GPU and two panes side by side meet without a
//! seam. Moving or resizing the window only moves where it's placed: GPUI draws the whole
//! window again as the window moves (`Window::bounds_changed`).

use gpui_kit::{
    App, Bounds, ContentMask, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Position, Rgba, Size, Style, Window, point, px, relative, size,
};

use super::blur;
use super::dots::Grid;
use super::make::Recipe;
use super::screens::{self, Hold};
use super::strips::{self, Placed, Shared};
use crate::settings::ChatBackdrop;
use crate::theme;

/// The screen's wallpaper behind a chat, filling its parent, which must be `relative`, looking
/// as `settings` say. Painted, it says where it was and what the strips over it draw (`placed`).
pub struct Picture {
    pub settings: ChatBackdrop,
    pub placed: Shared,
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
        self.placed.replace(None);
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
        let settings = self.settings;
        let scale = window.scale_factor();
        let recipe = recipe(display, screen.size, scale, settings, cx);
        let sigma = (settings.blur)
            .then(|| settings.blur_strength * scale / blur::SHRINK as f32);
        let Some(showing) = screens::picture(display, recipe, sigma, cx) else {
            return;
        };
        let shown = bounds.intersect(&screen);
        window.with_content_mask(Some(ContentMask { bounds: shown }), |window| {
            window.paint_surface(screen, showing.placeholder);
        });
        let grid = Grid::new(settings.dot_size, settings.density, scale);
        let (width, height) = grid.tile();
        let tile = size(px(width as f32 / scale), px(height as f32 / scale));
        self.placed.replace(Some(Placed {
            screen,
            shown,
            blur: showing.blur,
            dots: (strips::tile(grid, cx), tile),
        }));
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
/// and the card's colour are now; none while it has no wallpaper to show.
fn recipe(
    display: u64,
    size: Size<Pixels>,
    scale: f32,
    backdrop: ChatBackdrop,
    cx: &mut App,
) -> Option<Recipe> {
    let wallpaper = screens::wallpaper(display, cx)?;
    let Rgba { r, g, b, .. } = theme::palette(cx).card.into();
    Some(Recipe {
        wallpaper,
        screen: (f64::from(size.width.as_f32()), f64::from(size.height.as_f32())),
        scale: f64::from(scale),
        card: [r, g, b],
        dim: backdrop.dim,
        blocks: backdrop.quality.blocks(),
        resolution: backdrop.quality.resolution(),
    })
}
