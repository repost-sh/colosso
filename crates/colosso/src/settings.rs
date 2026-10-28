//! The user's settings, kept in `settings.json` under the app's folder in Application Support
//! (`COLOSSO_CONFIG_DIR` points elsewhere). Each module reads its part at startup and writes
//! changes back through `update`, which saves once they settle.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, Context, Global, Subscription, Task};
use serde::{Deserialize, Serialize};

use crate::gradient::GradientTheme;
use crate::theme::{self, ThemePreference};

const APP_ID: &str = "dev.colosso.desktop";
const FILE: &str = "settings.json";
/// How long changes must settle before they are written, so a drag saves once.
const SAVE_DELAY: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Layout {
    #[default]
    Orca,
    Arc,
}

/// Switches the window to a layout, from a menu or a key.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetLayout(pub Layout);

impl Layout {
    pub const ALL: [Layout; 2] = [Layout::Orca, Layout::Arc];

    pub fn label(self) -> &'static str {
        match self {
            Layout::Orca => "Orca",
            Layout::Arc => "Arc",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Layout::Orca => Layout::Arc,
            Layout::Arc => Layout::Orca,
        }
    }

    /// A menu of the layouts with `self` checked.
    pub fn menu(self) -> NativeMenu {
        Layout::ALL
            .into_iter()
            .fold(NativeMenu::new(), |menu, layout| {
                menu.menu_with_check(layout.label(), layout == self, Box::new(SetLayout(layout)))
            })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: ThemePreference,
    pub light_theme: GradientTheme,
    pub dark_theme: GradientTheme,
    pub layout: Layout,
    pub sidebar_width: f32,
    pub sidebar_open_on_launch: bool,
}

impl Settings {
    /// The gradient theme for the dark or the light appearance.
    pub fn theme(&self, dark: bool) -> &GradientTheme {
        if dark {
            &self.dark_theme
        } else {
            &self.light_theme
        }
    }

    pub fn theme_mut(&mut self, dark: bool) -> &mut GradientTheme {
        if dark {
            &mut self.dark_theme
        } else {
            &mut self.light_theme
        }
    }

    /// The sidebar's width, kept within its limits even if the file says otherwise.
    pub fn sidebar_width(&self) -> f32 {
        self.sidebar_width
            .clamp(theme::SIDEBAR_MIN_WIDTH, theme::SIDEBAR_MAX_WIDTH)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            appearance: ThemePreference::default(),
            light_theme: GradientTheme::default(),
            dark_theme: GradientTheme::default(),
            layout: Layout::default(),
            sidebar_width: theme::SIDEBAR_WIDTH,
            sidebar_open_on_launch: true,
        }
    }
}

struct Store {
    settings: Settings,
    path: Option<PathBuf>,
    /// The save waiting for changes to settle; replacing it cancels it.
    pending: Option<Task<()>>,
}

impl Global for Store {}

/// Loads the settings, falling back to the defaults for a missing or unreadable file.
pub fn init(cx: &mut App) {
    let path = std::env::var_os("COLOSSO_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join(APP_ID)))
        .map(|dir| dir.join(FILE));
    let settings = path.as_deref().map(load).unwrap_or_default();
    cx.set_global(Store {
        settings,
        path,
        pending: None,
    });
    cx.on_action(|SetLayout(layout): &SetLayout, cx| {
        update(cx, |settings| settings.layout = *layout)
    });
    // A change still settling when the app quits is written on the way out.
    cx.on_app_quit(|cx| {
        if cx.global_mut::<Store>().pending.take().is_some()
            && let Some((path, json)) = encode(cx)
        {
            write(&path, &json);
        }
        async {}
    })
    .detach();
}

pub fn get(cx: &App) -> &Settings {
    &cx.global::<Store>().settings
}

/// Where the settings are saved, if the app has a folder to save them in.
pub fn path(cx: &App) -> Option<&Path> {
    cx.global::<Store>().path.as_deref()
}

/// Re-renders the observing view whenever a setting changes.
pub fn observe<V: 'static>(cx: &mut Context<V>) -> Subscription {
    cx.observe_global::<Store>(|_, cx| cx.notify())
}

/// Applies `change` and saves once changes settle, if it changed anything. Observers only
/// hear about changes, so a change to nothing does not re-render them.
pub fn update(cx: &mut App, change: impl FnOnce(&mut Settings)) {
    let mut settings = get(cx).clone();
    change(&mut settings);
    if settings == *get(cx) {
        return;
    }
    cx.global_mut::<Store>().settings = settings;
    let save = cx.spawn(async move |cx| {
        cx.background_executor().timer(SAVE_DELAY).await;
        if let Some((path, json)) = cx.update(encode) {
            cx.background_executor()
                .spawn(async move { write(&path, &json) })
                .await;
        }
    });
    cx.global_mut::<Store>().pending = Some(save);
}

fn load(path: &Path) -> Settings {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            eprintln!("colosso: ignoring unreadable {}: {error}", path.display());
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

/// The settings file's path and the JSON to write there.
fn encode(cx: &mut App) -> Option<(PathBuf, Vec<u8>)> {
    let store = cx.global::<Store>();
    let path = store.path.clone()?;
    match serde_json::to_vec_pretty(&store.settings) {
        Ok(json) => Some((path, json)),
        Err(error) => {
            eprintln!("colosso: could not encode settings: {error}");
            None
        }
    }
}

/// Writes through a temporary file, so a crash mid-write never leaves half a file behind.
fn write(path: &Path, json: &[u8]) {
    let temporary = path.with_extension("json.tmp");
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&temporary, json))
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = written {
        eprintln!("colosso: could not save {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::point;

    use super::*;
    use crate::gradient::Harmony;

    #[test]
    fn settings_survive_a_round_trip() {
        let mut settings = Settings {
            appearance: ThemePreference::Dark,
            layout: Layout::Arc,
            sidebar_width: 320.,
            ..Settings::default()
        };
        settings.dark_theme.dots = vec![point(250., 120.), point(110., 240.)];
        settings.dark_theme.harmony = Harmony::Complementary;
        settings.light_theme.texture = 0.5;
        let json = serde_json::to_vec(&settings).unwrap();
        assert_eq!(serde_json::from_slice::<Settings>(&json).unwrap(), settings);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let settings: Settings = serde_json::from_str(r#"{ "layout": "Arc" }"#).unwrap();
        assert_eq!(settings.layout, Layout::Arc);
        assert_eq!(settings.sidebar_width, theme::SIDEBAR_WIDTH);
        assert_eq!(settings.dark_theme, GradientTheme::default());
    }
}
