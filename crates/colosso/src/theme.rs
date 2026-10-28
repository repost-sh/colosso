//! Sizes, the light and dark palettes, and the System/Light/Dark preference that picks one.

use gpui_kit::assets::IconName;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::{Action, App, Hsla, Window, WindowAppearance, actions};
use serde::{Deserialize, Serialize};

use crate::settings;

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
    /// Switched-on switches and links.
    pub accent: Hsla,
}

pub const fn gray(lightness: f32, alpha: f32) -> Hsla {
    Hsla {
        h: 0.,
        s: 0.,
        l: lightness,
        a: alpha,
    }
}

const ACCENT: Hsla = Hsla {
    h: 0.59,
    s: 0.9,
    l: 0.56,
    a: 1.,
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
    set_preference(settings::get(cx).appearance, cx);
}

/// Keeps the theme on the window's appearance: the forced one while a preference forces
/// it, the system's otherwise. gpui only reports appearance changes per window.
pub fn follow_window_appearance(window: &Window) {
    window
        .observe_window_appearance(|window, cx| Theme::sync_system_appearance(Some(window), cx))
        .detach();
}

pub fn palette(cx: &App) -> Palette {
    if cx.theme().mode.is_dark() {
        DARK
    } else {
        LIGHT
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
