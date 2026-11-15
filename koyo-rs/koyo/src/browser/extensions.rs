//! Browser extensions as the app shows them (`docs/extensions.md`): each profile's, as the
//! engine lists them, and what each set its toolbar action to, for one page or for all. The
//! engine runs them and keeps them; this is what the icons are drawn from, and it's gone
//! with the app.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::{Image, ImageFormat};
use koyo_browser_ipc::{Extension, ExtensionAction, PaneId};

pub(super) mod icon;
pub(super) mod menu;

/// The Chrome Web Store's extensions, where "+" goes.
pub(super) const STORE: &str = "https://chromewebstore.google.com/category/extensions";
/// Chrome's own page of a profile's extensions.
pub(super) const MANAGE: &str = "chrome://extensions/";

/// What an extension set its action to: for every page, and for single pages.
#[derive(Default)]
struct Actions {
    all: ExtensionAction,
    panes: HashMap<PaneId, ExtensionAction>,
}

/// Every profile's extensions and their actions, as the engine last said.
#[derive(Default)]
pub(super) struct Store {
    /// A profile's extensions, by name, as listed.
    lists: HashMap<String, Vec<Extension>>,
    /// What each set its action to, by profile and extension.
    actions: HashMap<(String, String), Actions>,
    /// Icons read and decoded once: by file, or by the `data:` address an extension set.
    icons: HashMap<String, Option<Arc<Image>>>,
    /// How many times what's kept here changed: what draws from it knows by this to draw
    /// again.
    changes: u64,
}

/// A badge on an icon: its text, and its colours when the extension chose them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Badge {
    pub(super) text: String,
    /// Red, green, blue and alpha, each 0 to 255.
    pub(super) color: Option<[u8; 4]>,
    pub(super) text_color: Option<[u8; 4]>,
}

/// An extension's icon for one page, as it's drawn.
#[derive(Clone)]
pub(super) struct Shown {
    pub(super) id: String,
    pub(super) name: String,
    /// What the tooltip says: the action's title, else the extension's name.
    pub(super) title: String,
    pub(super) icon: Option<Arc<Image>>,
    pub(super) badge: Option<Badge>,
    /// Whether a press does anything on this page: the action isn't disabled here.
    pub(super) enabled: bool,
    pub(super) pinned: bool,
    pub(super) removable: bool,
    /// Its options page and its home page; empty when it has none.
    pub(super) options: String,
    pub(super) homepage: String,
}

impl Store {
    /// The engine listed `profile`'s extensions. Actions of those that went are forgotten.
    pub(super) fn listed(&mut self, profile: String, extensions: Vec<Extension>) {
        self.actions.retain(|(of, id), _| {
            *of != profile || extensions.iter().any(|extension| extension.id == *id)
        });
        self.lists.insert(profile, extensions);
        self.changes += 1;
    }

    /// How many times what the engine said changed.
    pub(super) fn changes(&self) -> u64 {
        self.changes
    }

    /// Whether the engine runs `profile`'s extensions with their icons and popups: it has
    /// listed them, were it none. It doesn't where pages aren't native, or the engine is
    /// CEF's own (`docs/extensions.md` 2).
    pub(super) fn known(&self, profile: &str) -> bool {
        self.lists.contains_key(profile)
    }

    /// An extension set its action, for `pane`'s page or for every page.
    pub(super) fn acted(
        &mut self,
        profile: String,
        extension: String,
        pane: Option<PaneId>,
        action: ExtensionAction,
    ) {
        let actions = self.actions.entry((profile, extension)).or_default();
        match pane {
            Some(pane) => drop(actions.panes.insert(pane, action)),
            None => actions.all = action,
        }
        self.changes += 1;
    }

    /// `pane`'s page is gone, and what was set for it.
    pub(super) fn pane_closed(&mut self, pane: PaneId) {
        for actions in self.actions.values_mut() {
            actions.panes.remove(&pane);
        }
    }

    /// How many of `profile`'s extensions are turned on, and so have an icon.
    pub(super) fn count(&self, profile: &str) -> usize {
        (self.lists.get(profile)).map_or(0, |list| list.iter().filter(|extension| extension.enabled).count())
    }

    /// The extension `id` of `profile`.
    pub(super) fn get(&self, profile: &str, id: &str) -> Option<&Extension> {
        self.lists.get(profile)?.iter().find(|extension| extension.id == id)
    }

    /// The icons of `profile`'s extensions that are turned on, as `pane`'s page shows them:
    /// pinned ones first, each group by name. With `pinned_only`, only those.
    pub(super) fn shown(&mut self, profile: &str, pane: PaneId, pinned_only: bool) -> Vec<Shown> {
        let Some(list) = self.lists.get(profile) else {
            return Vec::new();
        };
        let mut list: Vec<&Extension> = (list.iter())
            .filter(|extension| extension.enabled && (extension.pinned || !pinned_only))
            .collect();
        // Stable: the engine's list is by name.
        list.sort_by_key(|extension| !extension.pinned);
        let mut shown = Vec::with_capacity(list.len());
        for extension in list {
            let actions = self.actions.get(&(profile.to_owned(), extension.id.clone()));
            let resolved = resolve(actions, pane);
            let source = if resolved.icon.is_empty() { &extension.icon_path } else { &resolved.icon };
            let icon = if source.is_empty() {
                None
            } else {
                (self.icons.entry(source.clone()).or_insert_with(|| image(source))).clone()
            };
            shown.push(Shown {
                id: extension.id.clone(),
                name: extension.name.clone(),
                title: if resolved.title.is_empty() { extension.name.clone() } else { resolved.title },
                icon,
                badge: (!resolved.badge_text.is_empty()).then(|| Badge {
                    text: resolved.badge_text,
                    color: rgba(&resolved.badge_color),
                    text_color: rgba(&resolved.badge_text_color),
                }),
                enabled: resolved.enabled,
                pinned: extension.pinned,
                removable: extension.removable,
                options: extension.options_url.clone(),
                homepage: extension.homepage_url.clone(),
            });
        }
        shown
    }
}

/// What shows for `pane`: each value the page's own where the extension set one, else the
/// one for every page. An extension that said nothing yet has an enabled action.
fn resolve(actions: Option<&Actions>, pane: PaneId) -> ExtensionAction {
    let Some(actions) = actions else {
        return ExtensionAction { enabled: true, ..ExtensionAction::default() };
    };
    let all = &actions.all;
    let Some(own) = actions.panes.get(&pane) else {
        return all.clone();
    };
    let pick = |own: &String, all: &String| if own.is_empty() { all.clone() } else { own.clone() };
    ExtensionAction {
        icon: pick(&own.icon, &all.icon),
        title: pick(&own.title, &all.title),
        badge_text: pick(&own.badge_text, &all.badge_text),
        badge_color: pick(&own.badge_color, &all.badge_color),
        badge_text_color: pick(&own.badge_text_color, &all.badge_text_color),
        // The engine says a page's own with what's set for all already counted in.
        enabled: own.enabled,
    }
}

/// A colour written `#RRGGBBAA`.
fn rgba(text: &str) -> Option<[u8; 4]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 8 || !hex.is_ascii() {
        return None;
    }
    let part = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some([part(0)?, part(2)?, part(4)?, part(6)?])
}

/// The picture at `source`: a `data:` address of a PNG, as an extension sets one, or a file
/// of the extension's own. None when it can't be read or isn't a picture the app draws.
fn image(source: &str) -> Option<Arc<Image>> {
    use base64::Engine as _;
    if let Some(data) = source.strip_prefix("data:image/png;base64,") {
        let png = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
        return Some(Arc::new(Image::from_bytes(ImageFormat::Png, png)));
    }
    let format = match source.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::Webp,
        "gif" => ImageFormat::Gif,
        "svg" => ImageFormat::Svg,
        _ => return None,
    };
    let bytes = std::fs::read(source).ok()?;
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extension(id: &str, name: &str, pinned: bool, enabled: bool) -> Extension {
        Extension {
            id: id.into(),
            name: name.into(),
            version: "1".into(),
            enabled,
            removable: true,
            pinned,
            ..Extension::default()
        }
    }

    fn set(badge: &str, title: &str, enabled: bool) -> ExtensionAction {
        ExtensionAction {
            badge_text: badge.into(),
            title: title.into(),
            enabled,
            ..ExtensionAction::default()
        }
    }

    fn names(shown: &[Shown]) -> Vec<&str> {
        shown.iter().map(|shown| shown.name.as_str()).collect()
    }

    #[test]
    fn pinned_extensions_come_first_and_those_turned_off_not_at_all() {
        let mut store = Store::default();
        store.listed(
            "p".into(),
            vec![
                extension("a", "Alpha", false, true),
                extension("b", "Beta", true, true),
                extension("c", "Gamma", false, false),
                extension("d", "Delta", true, true),
            ],
        );
        assert_eq!(names(&store.shown("p", 1, false)), ["Beta", "Delta", "Alpha"]);
        assert_eq!(names(&store.shown("p", 1, true)), ["Beta", "Delta"]);
        assert!(store.shown("other", 1, false).is_empty());
        assert_eq!((store.count("p"), store.count("other")), (3, 0));
        assert!(store.known("p") && !store.known("other"));
    }

    #[test]
    fn a_page_shows_what_was_set_for_it_else_what_was_set_for_all() {
        let mut store = Store::default();
        store.listed("p".into(), vec![extension("a", "Alpha", true, true)]);
        // Nothing said yet: the name, no badge, enabled.
        let shown = &store.shown("p", 1, false)[0];
        assert_eq!((shown.title.as_str(), shown.badge.clone(), shown.enabled), ("Alpha", None, true));

        store.acted("p".into(), "a".into(), None, set("3", "Blocking", true));
        store.acted("p".into(), "a".into(), Some(7), set("12", "", false));
        let on_all = &store.shown("p", 1, false)[0];
        assert_eq!(on_all.title, "Blocking");
        assert_eq!(on_all.badge.as_ref().map(|badge| badge.text.as_str()), Some("3"));
        assert!(on_all.enabled);
        let on_seven = &store.shown("p", 7, false)[0];
        assert_eq!(on_seven.title, "Blocking", "the title wasn't set for the page");
        assert_eq!(on_seven.badge.as_ref().map(|badge| badge.text.as_str()), Some("12"));
        assert!(!on_seven.enabled);

        // The page goes, and what was set for it.
        store.pane_closed(7);
        let back = &store.shown("p", 7, false)[0];
        assert_eq!(back.badge.as_ref().map(|badge| badge.text.as_str()), Some("3"));
    }

    #[test]
    fn an_extension_that_goes_takes_its_action_with_it() {
        let mut store = Store::default();
        store.listed("p".into(), vec![extension("a", "Alpha", true, true)]);
        store.acted("p".into(), "a".into(), None, set("3", "", true));
        store.listed("p".into(), Vec::new());
        assert!(store.shown("p", 1, false).is_empty());
        // Installed again, it starts with nothing set.
        store.listed("p".into(), vec![extension("a", "Alpha", true, true)]);
        assert_eq!(store.shown("p", 1, false)[0].badge, None);
    }

    #[test]
    fn a_badges_colours_are_read() {
        assert_eq!(rgba("#FF000080"), Some([255, 0, 0, 128]));
        assert_eq!(rgba("#0b57d0ff"), Some([11, 87, 208, 255]));
        assert_eq!(rgba(""), None);
        assert_eq!(rgba("#FFF"), None);
        assert_eq!(rgba("#GG0000FF"), None);
    }

    #[test]
    fn an_icon_is_a_png_an_extension_set_or_a_file_of_its_own() {
        // A one-pixel PNG, as `chrome.action.setIcon` arrives.
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        assert!(image(&format!("data:image/png;base64,{png}")).is_some());
        assert!(image("data:image/png;base64,not base64!").is_none());
        assert!(image("/no/such/icon.png").is_none());
        assert!(image("/etc/hosts").is_none(), "not a picture by its name");
    }
}
