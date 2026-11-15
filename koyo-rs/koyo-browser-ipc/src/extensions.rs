//! Browser extensions (`docs/extensions.md`): what the host says of a profile's extensions
//! and of their toolbar actions, and what the app asks to be done with one.
use serde::{Deserialize, Serialize};

/// An extension installed in a profile, as its icon and the list of extensions show it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extension {
    /// Chromium's id for it.
    pub id: String,
    /// Its name, in the person's language where it has one.
    pub name: String,
    /// Its version.
    pub version: String,
    /// What it says it does.
    #[serde(default)]
    pub description: String,
    /// Whether it's turned on.
    pub enabled: bool,
    /// Whether the person may turn it off or remove it: one a policy installed may not be.
    pub removable: bool,
    /// Whether the person pinned it to the toolbar.
    pub pinned: bool,
    /// Its options page; empty when it has none.
    #[serde(default)]
    pub options_url: String,
    /// Its home page, the Web Store's page for one from there; empty when it has none.
    #[serde(default)]
    pub homepage_url: String,
    /// The file of its toolbar icon as its manifest names it, the largest up to 128 pixels;
    /// empty when it has none.
    #[serde(default)]
    pub icon_path: String,
    /// Where it came from.
    pub source: ExtensionSource,
}

/// Where an extension came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExtensionSource {
    /// The Chrome Web Store.
    Store,
    /// A folder, loaded from `chrome://extensions` or the command line.
    Unpacked,
    /// A policy of the machine's.
    Policy,
    /// Anything else.
    #[default]
    Other,
}

/// What an extension set its toolbar action to, for one page or for every page. An empty
/// value is one it didn't set: for a page, what's set for every page shows; for every page,
/// the manifest's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionAction {
    /// The icon, as a PNG `data:` address.
    #[serde(default)]
    pub icon: String,
    /// The title, which the icon's tooltip says.
    #[serde(default)]
    pub title: String,
    /// The badge's text.
    #[serde(default)]
    pub badge_text: String,
    /// The badge's colour, as `#RRGGBBAA`.
    #[serde(default)]
    pub badge_color: String,
    /// The colour of the badge's text, as `#RRGGBBAA`.
    #[serde(default)]
    pub badge_text_color: String,
    /// Whether the action is enabled: `chrome.action.disable` greys the icon out.
    pub enabled: bool,
}

/// What the person does with an installed extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ExtensionChange {
    /// Turns it on or off.
    Enabled {
        /// Whether it's on.
        on: bool,
    },
    /// Pins it to the toolbar or takes it off.
    Pinned {
        /// Whether it's pinned.
        on: bool,
    },
    /// Removes it from the profile.
    Remove,
}
