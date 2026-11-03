//! What a page asks the person, through the app: its dialogs and permission prompts, the
//! context menu, and the answers.
use serde::{Deserialize, Serialize};

/// A row of the engine's context menu.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MenuEntry {
    /// A command.
    Command {
        /// The engine's id for it.
        command: i32,
        /// Its label, without mnemonics.
        label: String,
        /// Whether it can be picked.
        enabled: bool,
        /// For check and radio rows, whether it is on.
        checked: Option<bool>,
    },
    /// A rule between groups.
    Separator,
    /// A nested menu.
    Submenu {
        /// Its label.
        label: String,
        /// Its rows.
        entries: Vec<MenuEntry>,
    },
}

/// What was under the pointer when a context menu opened.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuTarget {
    /// A link's address.
    #[serde(default)]
    pub link: String,
    /// An image's or other media's address.
    #[serde(default)]
    pub media: String,
    /// The selected text.
    #[serde(default)]
    pub selection: String,
    /// Whether it is a text field.
    #[serde(default)]
    pub editable: bool,
}

/// A question the page or the engine asks the person.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Dialog {
    /// `alert()`: a message and OK.
    Alert {
        /// The page's text.
        message: String,
    },
    /// `confirm()`: OK or Cancel.
    Confirm {
        /// The page's text.
        message: String,
    },
    /// `prompt()`: a line of text, or Cancel.
    Prompt {
        /// The page's text.
        message: String,
        /// The text the field starts with.
        default_text: String,
    },
    /// The page has unsaved changes and asks before it is left.
    BeforeUnload {
        /// Whether the page is being reloaded rather than left.
        reload: bool,
    },
    /// A server or proxy asks for a user name and password.
    Auth {
        /// The host asking, with its port when it is not the default.
        host: String,
        /// The realm the server names, if any.
        realm: String,
        /// A proxy rather than the site.
        proxy: bool,
    },
    /// The page asks to use a device or capability.
    Permission {
        /// What it asks for.
        permissions: Vec<Permission>,
    },
    /// The site's certificate is not trusted.
    Certificate {
        /// What is wrong, for the person.
        error: String,
    },
    /// The page asks the person to choose files to upload or a place to
    /// save one.
    File {
        /// What may be chosen.
        mode: FileMode,
        /// The page's title for the chooser, if any.
        title: String,
        /// The suggested path or file name.
        default_path: String,
        /// Accepted types: MIME types (`image/*`) or extensions (`.png`).
        accept: Vec<String>,
    },
}

/// What a page may ask to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Permission {
    /// The camera.
    Camera,
    /// The microphone.
    Microphone,
    /// The screen, a window or a tab.
    Screen,
    /// The location.
    Location,
    /// Notifications.
    Notifications,
    /// Reading the clipboard.
    Clipboard,
    /// MIDI devices.
    Midi,
    /// Motion and light sensors.
    Sensors,
    /// Installed fonts.
    Fonts,
    /// Window placement across displays.
    Windows,
    /// Devices on the local network.
    LocalNetwork,
    /// Files and folders on this computer.
    Files,
    /// Several downloads at once.
    Downloads,
    /// Anything else the engine asks for.
    #[serde(other)]
    Other,
}

/// What a file chooser lets the person pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileMode {
    /// One existing file.
    Open,
    /// Any number of existing files.
    OpenMultiple,
    /// One folder.
    Folder,
    /// A new or existing file to write.
    Save,
}

/// The person's answer to a [`Dialog`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Answer {
    /// Cancel, Block, Stay, Back to safety: whatever says no.
    Dismiss,
    /// OK, Allow, Leave, Proceed.
    Accept,
    /// The text entered into a prompt.
    Text {
        /// The text.
        text: String,
    },
    /// The user name and password entered for [`Dialog::Auth`].
    Credentials {
        /// The user name.
        username: String,
        /// The password.
        password: String,
    },
    /// The paths chosen for [`Dialog::File`].
    Files {
        /// Absolute paths.
        paths: Vec<String>,
    },
}
