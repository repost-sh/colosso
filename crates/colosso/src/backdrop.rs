//! Paints the gradient theme behind the window, and the bitmaps the theme picker shows.
//! GPUI's gradients take two stops and have no radial form, so the theme's stacked linear
//! and radial fades are rendered into a small image the GPU stretches over the window.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Bounds, Corners, Global, IntoElement, ParentElement as _, Pixels, RenderImage, Rgba,
    Styled as _, Window, canvas, div, point, px, size,
};
use image::{Frame, RgbaImage};

use crate::gradient::{self, GradientTheme, Rgb};

/// The window gradient is rendered at this fraction of the window's size; it is smooth
/// enough that the stretch doesn't show.
const DOWNSCALE: f32 = 4.;
/// The rendered size snaps up to a multiple of this, so a live resize reuses one image for
/// a while instead of rendering one per frame.
const SIZE_STEP: u32 = 8;
/// Side of the grain tile, in pixels; each one lands on a single screen pixel. Large, so few
/// tiles cover the window.
const GRAIN_TILE: u32 = 512;

/// A color fading to transparent over a gradient line, as CSS paints
/// `gradient(color from, transparent to)`, with `from` and `to` along the line from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fade {
    shape: Shape,
    color: Rgba,
    from: f32,
    to: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    Solid,
    /// A line through the center at this angle, in degrees: 0 points up, clockwise.
    Linear(f32),
    /// A circle at this point, as fractions of the box, reaching the farthest corner.
    Radial(f32, f32),
}

impl Fade {
    const fn new(shape: Shape, color: Rgba, from: f32, to: f32) -> Self {
        Fade {
            shape,
            color,
            from,
            to,
        }
    }

    /// The fade laid over a `width` by `height` box, with everything that doesn't vary
    /// per pixel worked out once.
    fn over(&self, width: f32, height: f32) -> Ramp {
        let (field, center) = match self.shape {
            Shape::Solid => (Field::Solid, (0., 0.)),
            Shape::Linear(degrees) => {
                let (sin, cos) = degrees.to_radians().sin_cos();
                let length = (width * sin).abs() + (height * cos).abs();
                let field = Field::Linear(sin / length, cos / length);
                (field, (width / 2., height / 2.))
            }
            Shape::Radial(cx, cy) => {
                let (cx, cy) = (cx * width, cy * height);
                let reach = [(0., 0.), (width, 0.), (0., height), (width, height)]
                    .into_iter()
                    .map(|(x, y): (f32, f32)| (x - cx).hypot(y - cy))
                    .fold(0., f32::max);
                (Field::Radial(1. / reach), (cx, cy))
            }
        };
        Ramp {
            field,
            center,
            from: self.from,
            per_span: 1. / (self.to - self.from),
            color: self.color,
        }
    }
}

/// A `Fade` placed in a box, ready to sample per pixel.
struct Ramp {
    field: Field,
    center: (f32, f32),
    from: f32,
    per_span: f32,
    color: Rgba,
}

enum Field {
    Solid,
    /// The gradient line's direction, divided by its length.
    Linear(f32, f32),
    /// One over the distance to the farthest corner.
    Radial(f32),
}

impl Ramp {
    fn alpha_at(&self, x: f32, y: f32) -> f32 {
        let (dx, dy) = (x - self.center.0, y - self.center.1);
        let t = match self.field {
            Field::Solid => return self.color.a,
            Field::Linear(sin, cos) => dx * sin - dy * cos + 0.5,
            Field::Radial(per_reach) => dx.hypot(dy) * per_reach,
        };
        let kept = 1. - ((t - self.from) * self.per_span).clamp(0., 1.);
        self.color.a * kept
    }
}

/// The window's fades, bottom first.
fn window_fades(theme: &GradientTheme) -> Vec<Fade> {
    let colors: Vec<Rgba> = theme
        .colors()
        .into_iter()
        .map(|color| theme.glass(color))
        .collect();
    let fade = Fade::new;
    match colors[..] {
        [color] => vec![fade(Shape::Solid, color, 0., 1.)],
        [first, second] => vec![
            fade(Shape::Linear(-30.), second, 0.3, 1.2),
            fade(Shape::Linear(150.), first, 0.3, 1.2),
        ],
        [first, second, third] => vec![
            fade(Shape::Radial(0., 0.), first, 0.1, 0.7),
            fade(Shape::Radial(0.95, 0.), second, 0., 0.75),
            fade(Shape::Linear(-5.), third, 0.1, 0.8),
        ],
        _ => Vec::new(),
    }
}

/// A three-color swatch's fades, bottom first.
fn swatch_fades(colors: [Rgb; 3]) -> Vec<Fade> {
    vec![
        Fade::new(Shape::Linear(0.), gradient::rgba(colors[2], 1.), 0., 0.6),
        Fade::new(Shape::Radial(1., 0.), gradient::rgba(colors[1], 1.), 0., 1.),
        Fade::new(Shape::Radial(0., 0.), gradient::rgba(colors[0], 1.), 0., 1.),
    ]
}

/// Stacks `fades` into a `width` by `height` image, compositing as CSS does.
fn render(fades: &[Fade], width: u32, height: u32) -> Arc<RenderImage> {
    let ramps: Vec<Ramp> = fades
        .iter()
        .map(|fade| fade.over(width as f32, height as f32))
        .collect();
    let image = RgbaImage::from_fn(width, height, |x, y| {
        let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
        // Premultiplied, so fading to transparent keeps the color's hue.
        let mut out = [0f32; 4];
        for ramp in &ramps {
            let alpha = ramp.alpha_at(x, y);
            let color = [ramp.color.r, ramp.color.g, ramp.color.b];
            for channel in 0..3 {
                out[channel] = color[channel] * alpha + out[channel] * (1. - alpha);
            }
            out[3] = alpha + out[3] * (1. - alpha);
        }
        let straight = |channel: f32| {
            if out[3] > 0. {
                (channel / out[3] * 255.).round() as u8
            } else {
                0
            }
        };
        // GPUI reads image data as BGRA.
        image::Rgba([
            straight(out[2]),
            straight(out[1]),
            straight(out[0]),
            (out[3] * 255.).round() as u8,
        ])
    });
    Arc::new(RenderImage::new(vec![Frame::new(image)]))
}

/// Sparse speckles of light and dark at low alpha, from a fixed seed so the grain holds still.
fn render_grain() -> Arc<RenderImage> {
    let mut state = 0x9e37_79b9_u32;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let image = RgbaImage::from_fn(GRAIN_TILE, GRAIN_TILE, |_, _| {
        let random = next();
        let shade = random as u8;
        let alpha = ((random >> 8) as u8) / 4;
        image::Rgba([shade, shade, shade, alpha])
    });
    Arc::new(RenderImage::new(vec![Frame::new(image)]))
}

/// The window gradient last rendered, the fades and pixel size it was rendered for.
struct WindowImage {
    fades: Vec<Fade>,
    size: (u32, u32),
    image: Arc<RenderImage>,
}

#[derive(Default)]
struct Images {
    window: Option<WindowImage>,
    grain: Option<Arc<RenderImage>>,
    swatches: HashMap<[Rgb; 3], Arc<RenderImage>>,
}

impl Global for Images {}

/// The gradient and grain under the window's contents. Fills its parent, which must be
/// `relative`, and paints nothing without a theme.
pub fn backdrop(cx: &App) -> impl IntoElement {
    let theme = gradient::current(cx);
    let fades = window_fades(theme);
    let texture = theme.texture;
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .when(!fades.is_empty(), |backdrop| {
            backdrop.child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| paint_window(bounds, fades, window, cx),
                )
                .size_full(),
            )
        })
        .when(texture > 0., |backdrop| {
            backdrop.child(grain(texture, Corners::default()))
        })
}

fn paint_window(bounds: Bounds<Pixels>, fades: Vec<Fade>, window: &mut Window, cx: &mut App) {
    let scaled = |side: Pixels| {
        ((side.as_f32() / DOWNSCALE).ceil() as u32 / SIZE_STEP + 1) * SIZE_STEP
    };
    let scaled = (scaled(bounds.size.width), scaled(bounds.size.height));
    let images = cx.default_global::<Images>();
    let image = match &images.window {
        Some(painted) if painted.fades == fades && painted.size == scaled => painted.image.clone(),
        _ => {
            let image = render(&fades, scaled.0, scaled.1);
            let painted = WindowImage {
                fades,
                size: scaled,
                image: image.clone(),
            };
            if let Some(old) = images.window.replace(painted) {
                window.drop_image(old.image).ok();
            }
            image
        }
    };
    window
        .paint_image(bounds, bounds, Corners::default(), image, 0, false)
        .ok();
}

/// The grain tiled over its parent at `opacity`, clipped to `corners`. The parent must be
/// `relative`.
pub fn grain(opacity: f32, corners: Corners<Pixels>) -> impl IntoElement {
    // Pinned, or it would sit below whatever comes before it in the parent.
    div().absolute().top_0().left_0().size_full().opacity(opacity).child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let tile = cx
                    .default_global::<Images>()
                    .grain
                    .get_or_insert_with(render_grain)
                    .clone();
                // One noise pixel per screen pixel: any larger and the stretch blurs the
                // specks into blotches.
                let side = px(GRAIN_TILE as f32 / window.scale_factor());
                let mut y = bounds.top();
                while y < bounds.bottom() {
                    let mut x = bounds.left();
                    while x < bounds.right() {
                        let tile_bounds = Bounds::new(point(x, y), size(side, side));
                        window
                            .paint_image(bounds, tile_bounds, corners, tile.clone(), 0, false)
                            .ok();
                        x += side;
                    }
                    y += side;
                }
            },
        )
        .size_full(),
    )
}

/// A three-color swatch, filling its parent and clipped to `corners`.
pub fn swatch(colors: [Rgb; 3], corners: Corners<Pixels>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let image = cx
                .default_global::<Images>()
                .swatches
                .entry(colors)
                .or_insert_with(|| render(&swatch_fades(colors), 52, 52))
                .clone();
            window
                .paint_image(bounds, bounds, corners, image, 0, false)
                .ok();
        },
    )
    .size_full()
}
