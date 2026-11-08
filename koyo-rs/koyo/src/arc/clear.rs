//! Clear (`docs/arc-sidebar.md` 5): asks before closing every tab of a worktree, which stops
//! their terminals and chats. A worktree with no tabs offers Delete instead, through the Delete
//! Worktree dialog.

use std::path::PathBuf;

use gpui_kit::{
    AnyWindowHandle, App, Context, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, WeakEntity, Window, div, px,
};

use crate::modal::{self, Button, LINE, Tone};
use crate::shell::Shell;
use crate::theme;

const WIDTH: f32 = 400.;
const GAP: f32 = 14.;

/// Asks about clearing the checkout at `path`, called `name`, which has `tabs` tabs, over
/// `window`.
pub fn open(
    shell: WeakEntity<Shell>,
    path: PathBuf,
    name: &str,
    tabs: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let dialog = Clear {
        shell,
        path,
        title: format!("Clear {name}?").into(),
        tabs,
        behind: window.window_handle(),
    };
    let title = modal::heading_height(&dialog.title, WIDTH, window);
    let height = 2. * modal::PADDING + title + 8. + LINE + GAP + modal::FOOTER;
    modal::open(WIDTH, height, window, cx, move |_, _| dialog);
}

pub(super) struct Clear {
    shell: WeakEntity<Shell>,
    path: PathBuf,
    title: SharedString,
    tabs: usize,
    /// The window the dialog sits over, where the tabs close.
    behind: AnyWindowHandle,
}

impl Clear {
    pub(super) fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (shell, path, behind) = (self.shell.clone(), self.path.clone(), self.behind);
        modal::dismiss(window, cx);
        let cleared = behind.update(cx, |_, window, cx| {
            shell.update(cx, |shell, cx| shell.clear_worktree(&path, window, cx))
        });
        if let Err(error) = cleared.and_then(|cleared| cleared) {
            log::warn!("couldn't clear the worktree: {error}");
        }
    }
}

impl Render for Clear {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let said = match self.tabs {
            1 => "Its tab closes, stopping its terminal or chat.".to_owned(),
            tabs => format!("Its {tabs} tabs close, stopping their terminals and chats."),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(modal::heading(self.title.clone()))
            .child(div().mt(px(8.)).h(px(LINE)).text_color(p.muted).child(said))
            .child(div().flex_1())
            .child(
                modal::footer(None, p)
                    .child(modal::cancel("clear-cancel"))
                    .child(
                        Button::new("clear-confirm", "Clear", Tone::Danger)
                            .on_click(cx.listener(|this, _, window, cx| this.clear(window, cx))),
                    ),
            )
    }
}
