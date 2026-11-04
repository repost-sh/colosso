//! The card's way to open a chat's checkout in an editor: the one last chosen, or the first of
//! those installed, and any other from a menu.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use gpui_kit::assets::IconName;
use gpui_kit::{
    Action, App, AppContext as _, Div, Focusable as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::super::ChatView;
use super::{clickable, control, row};
use crate::menu::{self, MenuItem};
use crate::settings;
use crate::shell::ellipsized;
use crate::theme::Palette;

/// The editors a checkout can open in, as their apps are called, the likeliest first.
const EDITORS: [&str; 15] = [
    "Cursor",
    "Visual Studio Code",
    "Visual Studio Code - Insiders",
    "VSCodium",
    "Zed",
    "Windsurf",
    "Xcode",
    "Sublime Text",
    "Nova",
    "IntelliJ IDEA",
    "WebStorm",
    "PyCharm",
    "RustRover",
    "GoLand",
    "Android Studio",
];

/// Opens the chat's checkout in an editor, from the card's menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = chat, no_json)]
pub(in super::super) struct OpenIn(pub(in super::super) &'static str);

/// The editors here, looked for once: an app put there meanwhile shows the next time Koyo starts.
fn installed() -> &'static [&'static str] {
    static INSTALLED: OnceLock<Vec<&'static str>> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let mine = dirs::home_dir().map(|home| home.join("Applications"));
        let folders: Vec<PathBuf> = [Some(PathBuf::from("/Applications")), mine].into_iter().flatten().collect();
        let here = |editor: &&str| folders.iter().any(|folder| folder.join(format!("{editor}.app")).exists());
        EDITORS.into_iter().filter(here).collect()
    })
}

fn open(editor: &'static str, folder: &Path, cx: &App) {
    let folder = folder.to_owned();
    cx.background_spawn(async move {
        let opened = Command::new("/usr/bin/open").arg("-a").arg(editor).arg(&folder).status();
        if let Err(error) = opened {
            log::warn!("couldn't open {} in {editor}: {error}", folder.display());
        }
    })
    .detach();
}

/// A person picked `editor`, which is the one offered from now on.
pub(in super::super) fn opened_in(view: &ChatView, OpenIn(editor): &OpenIn, cx: &mut App) {
    if let Some(place) = &view.details.place {
        open(editor, &place.folder, cx);
    }
    settings::update(cx, |settings| settings.chat_details.editor = Some((*editor).to_owned()));
}

/// The line that opens the checkout in the editor chosen, with the rest in a menu at its end.
pub(super) fn line(view: &ChatView, p: Palette, cx: &App) -> Option<Div> {
    let place = view.details.place.as_ref()?;
    let editors = installed();
    let chosen = settings::get(cx).chat_details.editor.as_deref();
    let editor = (chosen.and_then(|chosen| editors.iter().find(|editor| **editor == chosen)))
        .or_else(|| editors.first())
        .copied()?;
    let folder = place.folder.clone();
    let opens = row(IconName::SquareArrowOutUpRight, p).flex_1().min_w_0().child(ellipsized(format!("Open in {editor}")));
    let opens = clickable(opens, "chat-details-open-in", p)
        .on_click(move |_, _, cx| open(editor, &folder, cx));
    let others = menu::dropdown(
        "chat-details-editors",
        control("chat-details-editors-trigger", IconName::ChevronDown, p),
        view.focus_handle(cx),
        move |_| {
            installed()
                .iter()
                .map(|editor| MenuItem::new(None, *editor, Box::new(OpenIn(editor))))
                .collect()
        },
    );
    Some(div().flex().items_center().gap(px(2.)).child(opens).child(others))
}
