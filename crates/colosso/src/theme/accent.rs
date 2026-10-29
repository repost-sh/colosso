//! The accent colour: switched-on switches, links and match highlights, and the gpui-kit
//! components that take theirs from its theme. On macOS it is the system's, read at startup
//! and again whenever System Settings changes it; elsewhere it stays the app's own blue.

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig};
use gpui_kit::{App, Global, Hsla, Rgba, SharedString};

/// The accent under the light and the dark appearance; macOS gives each its own shade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Accents {
    pub light: Hsla,
    pub dark: Hsla,
}

/// The accent once one was read, cached so a frame never asks `AppKit`.
struct Current(Accents);

impl Global for Current {}

/// The accent read, if one was.
pub fn current(cx: &App) -> Option<Accents> {
    cx.try_global::<Current>().map(|Current(accents)| *accents)
}

/// The accent for the light or the dark appearance: the one read, or the app's own before any
/// was.
pub fn get(dark: bool, cx: &App) -> Hsla {
    current(cx).map_or(super::ACCENT, |accents| {
        if dark { accents.dark } else { accents.light }
    })
}

/// A colour from its sRGB components as `AppKit` gives them, 0 to 1; components past that, from
/// a wider gamut, are held to it.
pub fn from_srgb(red: f64, green: f64, blue: f64, alpha: f64) -> Hsla {
    let channel = |value: f64| value.clamp(0., 1.) as f32;
    Rgba {
        r: channel(red),
        g: channel(green),
        b: channel(blue),
        a: channel(alpha),
    }
    .into()
}

/// Takes `accents` up as the accent, in the palette and in gpui-kit's theme, and repaints
/// every window; the accent already in use changes nothing.
///
/// gpui-kit loads a mode's theme over its colours whenever the appearance changes, so the
/// accent goes into the light and the dark theme it loads rather than onto the colours.
pub fn set(accents: Accents, cx: &mut App) {
    if current(cx) == Some(accents) {
        return;
    }
    cx.set_global(Current(accents));
    let theme = Theme::global(cx);
    let light = with_accent(&theme.light_theme, accents.light);
    let dark = with_accent(&theme.dark_theme, accents.dark);
    let mode = theme.mode;
    let theme = Theme::global_mut(cx);
    (theme.light_theme, theme.dark_theme) = (light, dark);
    Theme::change(mode, None, cx);
}

/// `config` with the accent wherever gpui-kit's components draw one: solid in filled controls,
/// primary buttons and drag borders, translucent in focus rings and selected text, and tinting
/// the rows its menus, lists and tables hover over and select, and drop targets.
fn with_accent(config: &ThemeConfig, accent: Hsla) -> Rc<ThemeConfig> {
    let mut config = config.clone();
    let colors = &mut config.colors;
    let solid = hex(accent);
    colors.primary = Some(solid.clone());
    colors.primary_hover = Some(hex(Hsla {
        a: accent.a * 0.9,
        ..accent
    }));
    colors.primary_active = Some(hex(Hsla {
        l: accent.l * 0.85,
        ..accent
    }));
    colors.primary_foreground = Some("#ffffff".into());
    for solid_field in [
        &mut colors.slider_bar,
        &mut colors.list_active_border,
        &mut colors.table_active_border,
        &mut colors.drag_border,
    ] {
        *solid_field = Some(solid.clone());
    }
    colors.ring = Some(hex(translucent(accent, RING)));
    colors.selection = Some(hex(translucent(accent, SELECTION)));
    let tint = hex(translucent(accent, TINT));
    for tinted in [
        &mut colors.accent,
        &mut colors.list_active,
        &mut colors.table_active,
        &mut colors.drop_target,
    ] {
        *tinted = Some(tint.clone());
    }
    Rc::new(config)
}

/// How much of the accent shows in a focus ring, in selected text, and in a selected or
/// hovered row or a drop target: as much as gpui-kit lets selections and rows keep.
const RING: f32 = 0.5;
const SELECTION: f32 = 0.3;
const TINT: f32 = 0.2;

fn translucent(color: Hsla, alpha: f32) -> Hsla {
    Hsla { a: alpha, ..color }
}

/// `#rrggbbaa`, as gpui-kit's theme files spell a colour.
pub fn hex(color: Hsla) -> SharedString {
    let Rgba { r, g, b, a } = color.into();
    let byte = |channel: f32| (channel.clamp(0., 1.) * 255.).round() as u8;
    let [r, g, b, a] = [r, g, b, a].map(byte);
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}").into()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::component::{ActiveTheme as _, ThemeMode};
    use gpui_kit::{Hsla, Rgba, TestAppContext};

    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// A colour as eight-bit channels, which is how far a theme file keeps it.
    fn bytes(color: Hsla) -> [u8; 4] {
        let Rgba { r, g, b, a } = color.into();
        [r, g, b, a].map(|channel| (channel * 255.).round() as u8)
    }

    #[test]
    fn srgb_components_become_hsla() {
        // macOS's blue accent, (0, 122, 255): 211.3°.
        let blue = from_srgb(0., 122. / 255., 1., 1.);
        assert!(close(blue.h, 211.3 / 360.), "{blue:?}");
        assert!(
            close(blue.s, 1.) && close(blue.l, 0.5) && close(blue.a, 1.),
            "{blue:?}"
        );
        // Graphite has no hue.
        let graphite = from_srgb(0.6, 0.6, 0.6, 1.);
        assert!(
            close(graphite.s, 0.) && close(graphite.l, 0.6),
            "{graphite:?}"
        );
        // A wider gamut's red is held to sRGB's.
        let wide = from_srgb(1.2, -0.1, 0., 1.);
        assert_eq!(bytes(wide), [255, 0, 0, 255]);
    }

    #[gpui_kit::test]
    fn a_new_system_accent_reaches_the_palette_and_gpui_kits_theme(cx: &mut TestAppContext) {
        let pink = Accents {
            light: from_srgb(1., 45. / 255., 85. / 255., 1.),
            dark: from_srgb(1., 55. / 255., 95. / 255., 1.),
        };
        let heard = Rc::new(Cell::new(0));
        cx.update(|cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Light, None, cx);
            let heard = heard.clone();
            cx.observe_global::<Theme>(move |_| heard.set(heard.get() + 1))
                .detach();
        });
        cx.update(|cx| set(pink, cx));
        assert!(heard.get() > 0, "nothing heard the accent change");
        let follows = |cx: &App, accent: Hsla| {
            let theme = cx.theme();
            assert_eq!(bytes(super::super::palette(cx).accent), bytes(accent));
            assert_eq!(bytes(theme.primary), bytes(accent));
            // A switch that's on.
            assert_eq!(bytes(theme.tokens.primary.color), bytes(accent));
            assert_eq!(bytes(theme.slider_bar), bytes(accent));
            assert_eq!(bytes(theme.drag_border), bytes(accent));
            assert_eq!(bytes(theme.list_active), bytes(translucent(accent, TINT)));
            assert_eq!(bytes(theme.accent), bytes(translucent(accent, TINT)));
            assert_eq!(bytes(theme.ring), bytes(translucent(accent, RING)));
            assert_eq!(
                bytes(theme.selection),
                bytes(translucent(accent, SELECTION))
            );
        };
        cx.read(|cx| follows(cx, pink.light));
        // The dark appearance loads its own theme, with its own shade in it.
        cx.update(|cx| Theme::change(ThemeMode::Dark, None, cx));
        cx.read(|cx| follows(cx, pink.dark));
        // The accent already in use changes nothing and repaints nothing.
        let before = heard.get();
        cx.update(|cx| set(pink, cx));
        assert_eq!(heard.get(), before);
    }
}
