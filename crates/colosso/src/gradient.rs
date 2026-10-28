//! The gradient theme behind the window: up to three colors picked as dots on a disc, how
//! opaque they lie over the window's blur, and how much grain sits on top. A dot's angle
//! around the disc sets its hue and its distance from the center its lightness, or its
//! saturation for presets that fix the lightness.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Hsla, Point, Rgba, point};
use serde::{Deserialize, Serialize};

use crate::settings;

/// Side of the square pad that dot positions are measured in.
pub const PAD: f32 = 358.;
pub const MAX_DOTS: usize = 3;
/// The opacity slider's range: lower washes the colors out, higher hides the blur.
pub const MIN_OPACITY: f32 = 0.3;
pub const MAX_OPACITY: f32 = 0.8;
/// The texture dial's detents, a full turn apart.
pub const TEXTURE_STEPS: f32 = 16.;

/// Colors are read around a point just off the pad's center, over a radius a little wider
/// than the disc dots move in, so no dot reaches full lightness.
const COLOR_CENTER: f32 = 180.;
const COLOR_RADIUS: f32 = 194.;

pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Harmony {
    Complementary,
    SingleAnalogous,
    SplitComplementary,
    Analogous,
    Triadic,
    Floating,
}

impl Harmony {
    /// In the order the harmony button cycles through them. Each dot count starts on the
    /// first one that fits it.
    const ALL: [Harmony; 6] = [
        Harmony::Complementary,
        Harmony::SingleAnalogous,
        Harmony::SplitComplementary,
        Harmony::Analogous,
        Harmony::Triadic,
        Harmony::Floating,
    ];

    /// Hue offsets of the other dots from the primary, in degrees.
    fn angles(self) -> &'static [f32] {
        match self {
            Harmony::Complementary => &[180.],
            Harmony::SingleAnalogous => &[310.],
            Harmony::SplitComplementary => &[150., 210.],
            Harmony::Analogous => &[50., 310.],
            Harmony::Triadic => &[120., 240.],
            Harmony::Floating => &[],
        }
    }

    fn fits(self, dots: usize) -> bool {
        self.angles().len() + 1 == dots
    }

    fn first_for(dots: usize) -> Harmony {
        Self::ALL
            .into_iter()
            .find(|harmony| harmony.fits(dots))
            .unwrap_or(Harmony::Floating)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DotKind {
    /// Lightness grows with distance from the center; saturation stays near full.
    Free,
    /// Lightness comes from a preset; saturation fades toward the rim.
    FixedLightness,
    /// No saturation; lightness grows with distance from the center.
    Grayscale,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GradientTheme {
    /// Dot centers on the pad, the primary first; the others sit at `harmony`'s angles from it.
    pub dots: Vec<Point<f32>>,
    pub kind: DotKind,
    pub harmony: Harmony,
    /// Lightness, 0 to 100, of `FixedLightness` dots.
    pub lightness: f32,
    /// How opaque the colors lie over the window's blur, from `MIN_OPACITY` to `MAX_OPACITY`.
    pub opacity: f32,
    /// Grain strength in sixteenths, 0 up to 15/16.
    pub texture: f32,
}

impl Default for GradientTheme {
    fn default() -> Self {
        GradientTheme {
            dots: Vec::new(),
            kind: DotKind::Free,
            harmony: Harmony::Floating,
            lightness: 50.,
            opacity: 0.5,
            texture: 0.,
        }
    }
}

impl GradientTheme {
    pub fn color(&self, dot: Point<f32>) -> Rgb {
        let (dx, dy) = (dot.x - COLOR_CENTER, dot.y - COLOR_CENTER);
        let hue = dy.atan2(dx).to_degrees().rem_euclid(360.);
        let reach = ((dx.hypot(dy) - 0.2) / COLOR_RADIUS).clamp(0., 1.);
        let (saturation, lightness) = match self.kind {
            DotKind::Free => (90. + reach * 10., (reach * 100.).round()),
            DotKind::FixedLightness => ((1. - reach) * 100., self.lightness),
            DotKind::Grayscale => (0., (reach * 100.).round()),
        };
        hsl_to_rgb(hue, saturation / 100., lightness / 100.)
    }

    pub fn colors(&self) -> Vec<Rgb> {
        self.dots.iter().map(|&dot| self.color(dot)).collect()
    }

    /// `color` as it lies over the blur: lifted a little toward white, at the theme's opacity.
    pub fn glass(&self, color: Rgb) -> Rgba {
        let floor = self.opacity + MIN_OPACITY;
        let kept = (floor + 0.6 * (1. - floor)).min(1.);
        let [r, g, b] = mix(color, [255; 3], 1. - kept);
        Rgba {
            r: r / 255.,
            g: g / 255.,
            b: b / 255.,
            a: self.opacity,
        }
    }

    /// Moves the primary dot to `at`, kept on the disc, and the others with it; the first
    /// press on an empty pad places it.
    pub fn move_primary(&mut self, at: Point<f32>) {
        let at = onto_disc(at);
        if self.dots.is_empty() {
            self.kind = DotKind::Free;
            self.dots.push(at);
            return;
        }
        self.dots[0] = at;
        if !self.harmony.fits(self.dots.len()) {
            self.harmony = Harmony::first_for(self.dots.len());
        }
        self.harmonize();
    }

    pub fn can_add(&self) -> bool {
        !self.dots.is_empty() && self.dots.len() < MAX_DOTS
    }

    pub fn add_dot(&mut self) {
        if self.can_add() {
            self.harmony = Harmony::first_for(self.dots.len() + 1);
            self.harmonize();
        }
    }

    /// Drops the newest dot and spreads the rest on the first harmony that fits them.
    pub fn remove_dot(&mut self) {
        if self.dots.pop().is_some() {
            self.reharmonize();
        }
    }

    /// Drops the primary dot; the next one takes its place.
    pub fn remove_primary(&mut self) {
        if !self.dots.is_empty() {
            self.dots.remove(0);
            self.reharmonize();
        }
    }

    pub fn can_cycle_harmony(&self) -> bool {
        self.dots.len() > 1
    }

    pub fn cycle_harmony(&mut self) {
        let fitting: Vec<_> = Harmony::ALL
            .into_iter()
            .filter(|harmony| harmony.fits(self.dots.len()))
            .collect();
        if fitting.is_empty() {
            return;
        }
        let next = fitting
            .iter()
            .position(|&harmony| harmony == self.harmony)
            .map_or(0, |index| (index + 1) % fitting.len());
        self.harmony = fitting[next];
        self.harmonize();
    }

    pub fn apply_preset(&mut self, preset: &Preset) {
        self.kind = preset.kind;
        if let Some(lightness) = preset.lightness {
            self.lightness = lightness;
        }
        self.dots = vec![point(preset.position.0, preset.position.1)];
        self.harmony = preset.harmony;
        self.harmonize();
    }

    /// Snaps to the closest of the dial's detents.
    pub fn set_texture(&mut self, turn: f32) {
        self.texture = (turn * TEXTURE_STEPS).round() / TEXTURE_STEPS % 1.;
    }

    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(MIN_OPACITY, MAX_OPACITY);
    }

    fn reharmonize(&mut self) {
        self.harmony = Harmony::first_for(self.dots.len());
        self.harmonize();
    }

    /// Places the other dots at the harmony's angles from the primary, as far out as it is.
    fn harmonize(&mut self) {
        let Some(&primary) = self.dots.first() else {
            return;
        };
        if self.harmony == Harmony::Floating {
            return;
        }
        let center = PAD / 2.;
        let (dx, dy) = (primary.x - center, primary.y - center);
        let angle = dy.atan2(dx);
        let distance = dx.hypot(dy).min(center);
        self.dots.truncate(1);
        self.dots.extend(self.harmony.angles().iter().map(|offset| {
            let angle = angle + offset.to_radians();
            point(
                center + distance * angle.cos(),
                center + distance * angle.sin(),
            )
        }));
    }
}

/// Pulls a point outside the disc onto its rim.
fn onto_disc(at: Point<f32>) -> Point<f32> {
    let center = PAD / 2.;
    let (dx, dy) = (at.x - center, at.y - center);
    let distance = dx.hypot(dy);
    if distance <= center {
        return at;
    }
    point(
        center + dx / distance * center,
        center + dy / distance * center,
    )
}

pub fn rgba([r, g, b]: Rgb, alpha: f32) -> Rgba {
    Rgba {
        r: f32::from(r) / 255.,
        g: f32::from(g) / 255.,
        b: f32::from(b) / 255.,
        a: alpha,
    }
}

/// `amount` of the way from `from` to `to`, per channel.
fn mix(from: Rgb, to: Rgb, amount: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| f32::from(from[i]) * (1. - amount) + f32::from(to[i]) * amount)
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> Rgb {
    let rgba = Rgba::from(Hsla {
        h: hue / 360.,
        s: saturation.clamp(0., 1.),
        l: lightness.clamp(0., 1.),
        a: 1.,
    });
    [rgba.r, rgba.g, rgba.b].map(|channel| (channel * 255.).round() as u8)
}

/// What a preset swatch shows: one color, or three blended the way the window will be.
#[derive(Clone, Copy, Debug)]
pub enum Swatch {
    Solid(Rgb),
    Blend([Rgb; 3]),
}

#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub swatch: Swatch,
    kind: DotKind,
    lightness: Option<f32>,
    harmony: Harmony,
    position: (f32, f32),
}

const fn solid(swatch: u32, lightness: f32, x: f32, y: f32) -> Preset {
    Preset {
        swatch: Swatch::Solid(hex(swatch)),
        kind: DotKind::FixedLightness,
        lightness: Some(lightness),
        harmony: Harmony::Floating,
        position: (x, y),
    }
}

const fn blend(colors: [u32; 3], lightness: f32, x: f32, y: f32) -> Preset {
    Preset {
        swatch: Swatch::Blend([hex(colors[0]), hex(colors[1]), hex(colors[2])]),
        kind: DotKind::FixedLightness,
        lightness: Some(lightness),
        harmony: Harmony::Analogous,
        position: (x, y),
    }
}

const fn gray(swatch: u8, x: f32) -> Preset {
    Preset {
        swatch: Swatch::Solid([swatch; 3]),
        kind: DotKind::Grayscale,
        lightness: None,
        harmony: Harmony::Floating,
        position: (x, 180.),
    }
}

const fn hex(color: u32) -> Rgb {
    [(color >> 16) as u8, (color >> 8) as u8, color as u8]
}

/// The swatch row's pages: light colors, light blends, dark colors, dark blends, and grays.
pub const PAGES: [&[Preset]; 5] = [
    &[
        solid(0xf4efdf, 90., 240., 240.),
        solid(0xf0b8cd, 80., 233., 157.),
        solid(0xe9c3e3, 80., 236., 111.),
        solid(0xda7682, 70., 234., 173.),
        solid(0xeb8570, 70., 220., 187.),
        solid(0xdcce7f, 60., 225., 237.),
        solid(0x5becad, 60., 147., 195.),
        solid(0x919bb5, 50., 81., 84.),
    ],
    &[
        blend([0xf5edd6, 0xddf3d8, 0xf3d8e1], 90., 240., 240.),
        blend([0xf3bede, 0xf7deba, 0xdfc3ee], 85., 233., 157.),
        blend([0xe5b3e4, 0xecacb2, 0xc5b9df], 80., 236., 111.),
        blend([0xeb7a9f, 0xefef76, 0xd285e0], 70., 234., 173.),
        blend([0xf2737b, 0xaff273, 0xe67de8], 70., 220., 187.),
        blend([0xddcd55, 0x61d45e, 0xd75b7c], 60., 225., 237.),
        blend([0x4be7d2, 0x54afde, 0x3ef470], 60., 147., 195.),
        blend([0x7a849e, 0x8975a4, 0x74a2a4], 55., 81., 84.),
    ],
    &[
        solid(0x5d566a, 10., 171., 72.),
        solid(0x997096, 40., 265., 79.),
        solid(0x956066, 35., 301., 176.),
        solid(0x9c6645, 30., 237., 210.),
        solid(0x517b6c, 30., 91., 228.),
        solid(0x576e75, 25., 67., 159.),
        solid(0x836d5f, 20., 314., 235.),
        solid(0x447464, 20., 118., 215.),
    ],
    &[
        blend([0x171122, 0x250e23, 0x121621], 10., 171., 72.),
        blend([0x804c7c, 0x8d3f42, 0x615874], 40., 265., 79.),
        blend([0x7a3840, 0x7e7934, 0x6f446e], 35., 301., 176.),
        blend([0x834116, 0x408019, 0x7a1f5b], 30., 237., 210.),
        blend([0x2d6c55, 0x345565, 0x347623], 30., 91., 228.),
        blend([0x2d4a53, 0x2e3251, 0x265a41], 25., 67., 159.),
        blend([0x402f26, 0x374026, 0x3b2b34], 20., 314., 235.),
        blend([0x16503d, 0x1a3c4c, 0x1b570f], 20., 118., 215.),
    ],
    &[
        gray(224, 340.),
        gray(224, 337.5),
        gray(192, 315.),
        gray(160, 292.5),
        gray(128, 270.),
        gray(96, 247.5),
        gray(64, 225.),
        gray(32, 202.5),
        gray(0, 180.),
    ],
];

/// The theme of the appearance on screen; light and dark each keep their own.
pub fn current(cx: &App) -> &GradientTheme {
    settings::get(cx).theme(cx.theme().mode.is_dark())
}

/// Applies `change` to the theme on screen and repaints every window if it changed anything.
pub fn update(cx: &mut App, change: impl FnOnce(&mut GradientTheme)) {
    let mut theme = current(cx).clone();
    change(&mut theme);
    if theme != *current(cx) {
        let dark = cx.theme().mode.is_dark();
        settings::update(cx, |settings| *settings.theme_mut(dark) = theme);
        cx.refresh_windows();
    }
}
