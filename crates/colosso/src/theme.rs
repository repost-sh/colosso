//! Sizes, the light and dark palettes, and the System/Light/Dark preference that picks one.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::{
    Action, App, Hsla, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, Window, WindowAppearance, actions, div,
};
use serde::{Deserialize, Serialize};

use crate::settings;
use crate::shell::action_button;

mod accent;
#[cfg(target_os = "macos")]
mod system_accent;
#[cfg(feature = "visual-check")]
pub mod visual;

pub const TITLE_BAR_HEIGHT: f32 = 40.;
pub const TRAFFIC_LIGHT_SIZE: f32 = 14.;
/// Room left of the top-rail buttons for the traffic lights.
pub const TRAFFIC_LIGHTS_WIDTH: f32 = 78.;
pub const ICON_RAIL_WIDTH: f32 = 48.;
pub const GUTTER: f32 = 8.;
pub const CARD_RADIUS: f32 = 10.;
pub const SIDEBAR_WIDTH: f32 = 260.;
pub const SIDEBAR_MIN_WIDTH: f32 = 200.;
pub const SIDEBAR_MAX_WIDTH: f32 = 480.;
/// The changes panel's width beside the chat, unless widened to the whole card.
pub const CHANGES_WIDTH: f32 = 960.;
/// Room for the narrowest file list beside the narrowest diffs.
pub const CHANGES_MIN_WIDTH: f32 = CHANGES_FILES_MIN_WIDTH + DIFFS_MIN_WIDTH;
pub const CHANGES_MAX_WIDTH: f32 = 1400.;
/// The changes panel's file list, beside the diffs.
pub const CHANGES_FILES_WIDTH: f32 = 320.;
pub const CHANGES_FILES_MIN_WIDTH: f32 = 180.;
pub const CHANGES_FILES_MAX_WIDTH: f32 = 640.;
/// What the file list leaves of the changes panel for the diffs, at its widest.
pub const DIFFS_MIN_WIDTH: f32 = 360.;
/// The Files panel's width beside the chat, unless widened to the whole card: the tree and
/// the file it shows.
pub const FILES_WIDTH: f32 = 960.;
/// Room for the narrowest tree beside the narrowest viewer.
pub const FILES_MIN_WIDTH: f32 = FILES_TREE_MIN_WIDTH + VIEWER_MIN_WIDTH;
pub const FILES_MAX_WIDTH: f32 = 1400.;
/// The Files panel's tree, beside the file it shows.
pub const FILES_TREE_WIDTH: f32 = 300.;
pub const FILES_TREE_MIN_WIDTH: f32 = 180.;
pub const FILES_TREE_MAX_WIDTH: f32 = 640.;
/// What the tree leaves of the Files panel for the file it shows, at its widest.
pub const VIEWER_MIN_WIDTH: f32 = 360.;
/// What the right panel leaves of the card for the chat beside it, at its widest.
pub const CHAT_MIN_WIDTH: f32 = 320.;
pub const TAB_STRIP_HEIGHT: f32 = 38.;
pub const ROW_HEIGHT: f32 = 30.;
/// How wide a settings page's column grows before it centers in the card.
pub const SETTINGS_PAGE_WIDTH: f32 = 680.;
pub const MONO: &str = "Menlo";

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// The window's blur shows through this, so its alpha sets how glassy the rails look.
    pub rail_tint: Hsla,
    /// Painted over `rail_tint`, which covers the whole window, so the two alphas stack.
    pub card: Hsla,
    pub sunken: Hsla,
    pub raised: Hsla,
    pub hairline: Hsla,
    pub text: Hsla,
    /// Solid, for marks drawn on a `text`-colored fill.
    pub on_text: Hsla,
    pub muted: Hsla,
    pub hover: Hsla,
    pub selected: Hsla,
    /// Dropdown menus sit in a blurred window of their own; this tints that glass.
    pub menu: Hsla,
    /// Switched-on switches and links: the system's accent on macOS, `ACCENT` elsewhere.
    pub accent: Hsla,
    /// Errors, and what can't be undone.
    pub danger: Hsla,
    /// Lines and files a change added, and the counts of them.
    pub added: Hsla,
    /// Lines and files a change removed.
    pub removed: Hsla,
    /// Files changed in place.
    pub modified: Hsla,
    /// The colours of code by kind of token.
    pub code: Code,
}

impl Palette {
    /// The fill of what floats over a page in a native layer: the card's colour, solid, as
    /// the page under it is no blurred glass that `card`'s tint would read on.
    pub fn floating(self) -> Hsla {
        Hsla { a: 1., ..self.card }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Code {
    pub comment: Hsla,
    pub keyword: Hsla,
    pub string: Hsla,
    pub number: Hsla,
    pub function: Hsla,
    pub kind: Hsla,
    pub tag: Hsla,
    pub attribute: Hsla,
}

const fn hsl(h: f32, s: f32, l: f32) -> Hsla {
    Hsla { h, s, l, a: 1. }
}

pub const fn gray(lightness: f32, alpha: f32) -> Hsla {
    Hsla {
        h: 0.,
        s: 0.,
        l: lightness,
        a: alpha,
    }
}

const ACCENT: Hsla = hsl(0.59, 0.9, 0.56);
const DANGER: Hsla = hsl(0., 0.75, 0.6);

const DARK_CODE: Code = Code {
    comment: gray(1., 0.42),
    keyword: hsl(0.78, 0.6, 0.74),
    string: hsl(0.27, 0.42, 0.66),
    number: hsl(0.07, 0.7, 0.67),
    function: hsl(0.58, 0.75, 0.72),
    kind: hsl(0.12, 0.65, 0.68),
    tag: hsl(0.99, 0.62, 0.68),
    attribute: hsl(0.08, 0.6, 0.7),
};

const LIGHT_CODE: Code = Code {
    comment: gray(0., 0.45),
    keyword: hsl(0.78, 0.55, 0.45),
    string: hsl(0.33, 0.6, 0.3),
    number: hsl(0.07, 0.75, 0.43),
    function: hsl(0.6, 0.7, 0.42),
    kind: hsl(0.1, 0.8, 0.33),
    tag: hsl(0.98, 0.6, 0.45),
    attribute: hsl(0.07, 0.7, 0.4),
};

const DARK: Palette = Palette {
    rail_tint: gray(0.08, 0.55),
    card: gray(0.1, 0.6),
    sunken: gray(0., 0.2),
    raised: gray(1., 0.05),
    hairline: gray(1., 0.08),
    text: gray(1., 0.9),
    on_text: gray(0.1, 1.),
    muted: gray(1., 0.5),
    hover: gray(1., 0.05),
    selected: gray(1., 0.1),
    menu: gray(1., 0.06),
    accent: ACCENT,
    danger: DANGER,
    added: hsl(0.38, 0.55, 0.58),
    removed: hsl(0.0, 0.7, 0.64),
    modified: hsl(0.11, 0.75, 0.6),
    code: DARK_CODE,
};

const LIGHT: Palette = Palette {
    rail_tint: gray(0.96, 0.55),
    card: gray(1., 0.65),
    sunken: gray(0., 0.03),
    raised: gray(0., 0.05),
    hairline: gray(0., 0.09),
    text: gray(0., 0.87),
    on_text: gray(1., 1.),
    muted: gray(0., 0.5),
    hover: gray(0., 0.04),
    selected: gray(0., 0.07),
    menu: gray(1., 0.35),
    accent: ACCENT,
    danger: DANGER,
    added: hsl(0.38, 0.6, 0.36),
    removed: hsl(0.0, 0.65, 0.48),
    modified: hsl(0.1, 0.8, 0.4),
    code: LIGHT_CODE,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    pub const ALL: [ThemePreference; 3] = [
        ThemePreference::System,
        ThemePreference::Light,
        ThemePreference::Dark,
    ];

    fn next(self) -> Self {
        match self {
            ThemePreference::System => ThemePreference::Light,
            ThemePreference::Light => ThemePreference::Dark,
            ThemePreference::Dark => ThemePreference::System,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemePreference::System => "System",
            ThemePreference::Light => "Light",
            ThemePreference::Dark => "Dark",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            ThemePreference::System => IconName::SunMoon,
            ThemePreference::Light => IconName::Sun,
            ThemePreference::Dark => IconName::Moon,
        }
    }

    pub fn action(self) -> Box<dyn Action> {
        match self {
            ThemePreference::System => Box::new(UseSystemTheme),
            ThemePreference::Light => Box::new(UseLightTheme),
            ThemePreference::Dark => Box::new(UseDarkTheme),
        }
    }

    /// System, Light and Dark, with `self` checked.
    pub fn menu(self) -> NativeMenu {
        ThemePreference::ALL
            .into_iter()
            .fold(NativeMenu::new(), |menu, option| {
                menu.menu_with_check(option.label(), option == self, option.action())
            })
    }

    fn forced_appearance(self) -> Option<WindowAppearance> {
        match self {
            ThemePreference::System => None,
            ThemePreference::Light => Some(WindowAppearance::Light),
            ThemePreference::Dark => Some(WindowAppearance::Dark),
        }
    }
}

actions!(
    theme,
    [CycleTheme, UseSystemTheme, UseLightTheme, UseDarkTheme]
);

pub fn init(cx: &mut App) {
    cx.on_action(|_: &CycleTheme, cx| set_preference(preference(cx).next(), cx));
    cx.on_action(|_: &UseSystemTheme, cx| set_preference(ThemePreference::System, cx));
    cx.on_action(|_: &UseLightTheme, cx| set_preference(ThemePreference::Light, cx));
    cx.on_action(|_: &UseDarkTheme, cx| set_preference(ThemePreference::Dark, cx));
    lift_dark_popovers(cx);
    #[cfg(target_os = "macos")]
    system_accent::follow(cx);
    set_preference(settings::get(cx).appearance, cx);
}

/// gpui-kit's dark popovers are near black, a hole in the window under a hover card or a
/// menu; they're lifted to a grey above the card instead, as the light ones sit white on it.
fn lift_dark_popovers(cx: &mut App) {
    let theme = Theme::global_mut(cx);
    let mut dark = (*theme.dark_theme).clone();
    dark.colors.popover = Some(accent::hex(DARK_POPOVER));
    theme.dark_theme = Rc::new(dark);
}

const DARK_POPOVER: Hsla = gray(0.17, 1.);

/// Keeps the theme on the window's appearance: the forced one while a preference forces
/// it, the system's otherwise. gpui only reports appearance changes per window.
pub fn follow_window_appearance(window: &Window) {
    window
        .observe_window_appearance(|window, cx| Theme::sync_system_appearance(Some(window), cx))
        .detach();
}

/// The palette of the appearance on screen, in the accent in use.
pub fn palette(cx: &App) -> Palette {
    let dark = cx.theme().mode.is_dark();
    let palette = if dark { DARK } else { LIGHT };
    Palette {
        accent: accent::get(dark, cx),
        ..palette
    }
}

pub fn preference(cx: &App) -> ThemePreference {
    settings::get(cx).appearance
}

/// Forcing the native appearance too keeps the window frame and blur on the theme.
fn set_preference(preference: ThemePreference, cx: &mut App) {
    settings::update(cx, |settings| settings.appearance = preference);
    let forced = preference.forced_appearance();
    cx.set_window_appearance(forced);
    Theme::change(forced.unwrap_or_else(|| cx.window_appearance()), None, cx);
}

/// The rail's theme button: a click cycles System, Light and Dark; a right-click picks one
/// from a native menu.
pub fn toggle(cx: &App) -> impl IntoElement {
    let preference = preference(cx);
    div()
        .on_mouse_down(
            MouseButton::Right,
            move |event: &MouseDownEvent, window, cx| {
                // This menu stands in for the one the rest of the rails open.
                cx.stop_propagation();
                preference.menu().show(event.position, window, cx);
            },
        )
        .child(action_button(
            "rail-theme",
            preference.icon(),
            format!("Theme: {}", preference.label()),
            CycleTheme,
            cx,
        ))
}
