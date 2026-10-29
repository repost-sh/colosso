//! Asking before changes are thrown away. The dialog names what goes back to how it was and
//! what goes to the trash, and nothing happens until it's confirmed.

use std::collections::HashSet;

use colosso_git::{Change, Checkout as Git, Discarding};
use gpui_kit::{
    App, Context, Entity, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window, div,
};

use super::view::split_path;
use crate::checkout::Checkout;
use crate::modal::{self, Button, LINE, SMALL_LINE, Tone, small_line};
use crate::theme;

const WIDTH: f32 = 400.;
/// How many files the dialog names before summing up the rest.
const LISTED: usize = 5;
const GAP: f32 = 8.;

/// Asks over `window` before discarding `list`, of what `checkout` shows.
pub fn confirm(checkout: Entity<Checkout>, list: Vec<Change>, window: &mut Window, cx: &mut App) {
    // Conflicts are resolved, not discarded.
    let list: Vec<Change> = (list.into_iter())
        .filter(|change| change.discarding() != Discarding::Kept)
        .collect();
    if list.is_empty() {
        return;
    }
    // Some may be the unstaged half of a file partly staged, whose staged half stays.
    let staged: HashSet<&str> = (checkout.read(cx).files().iter())
        .map(|file| &file.diff.change)
        .filter(|change| change.staged)
        .map(|change| change.path.as_str())
        .collect();
    let keeps_staged =
        (list.iter()).any(|change| !change.staged && staged.contains(change.path.as_str()));
    let dialog = DiscardDialog::new(checkout, list, keeps_staged);
    modal::open(WIDTH, dialog.height(), window, cx, |_, _| dialog);
}

struct DiscardDialog {
    checkout: Entity<Checkout>,
    list: Vec<Change>,
    title: SharedString,
    /// The files named, when there's more than one.
    names: Vec<SharedString>,
    confirm: &'static str,
    discarding: bool,
    error: Option<SharedString>,
}

impl DiscardDialog {
    fn new(checkout: Entity<Checkout>, list: Vec<Change>, keeps_staged: bool) -> Self {
        let trashed = (list.iter())
            .filter(|change| change.discarding() == Discarding::Trashed)
            .count();
        let restored = list.len() - trashed;
        let what = if keeps_staged && list.iter().all(|change| !change.staged) {
            "unstaged changes"
        } else {
            "changes"
        };
        let (title, confirm) = match (&list[..], restored, trashed) {
            ([only], ..) => {
                let name = split_path(&only.path).1;
                match only.discarding() {
                    Discarding::Trashed => (format!("Trash {name}?"), "Trash"),
                    Discarding::Restored => (format!("Restore {name}?"), "Restore File"),
                    Discarding::Reverted | Discarding::Kept => {
                        (format!("Discard {what} to {name}?"), "Discard Changes")
                    }
                }
            }
            (_, restored, 0) => (format!("Discard {what} to {restored} files?"), "Discard"),
            (_, 0, trashed) => (format!("Trash {trashed} files?"), "Trash"),
            (_, restored, trashed) => (
                format!("Discard {what} to {restored} files and trash {trashed} files?"),
                "Discard and Trash",
            ),
        };
        let mut names: Vec<SharedString> = match &list[..] {
            [_] => Vec::new(),
            list => (list.iter().take(LISTED))
                .map(|change| split_path(&change.path).1.to_owned().into())
                .collect(),
        };
        if list.len() > LISTED {
            names.push(format!("and {} more…", list.len() - LISTED).into());
        }
        DiscardDialog {
            checkout,
            list,
            title: title.into(),
            names,
            confirm,
            discarding: false,
            error: None,
        }
    }

    /// The window height that fits what the dialog says.
    fn height(&self) -> f32 {
        let names = match self.names.len() {
            0 => 0.,
            lines => GAP + lines as f32 * SMALL_LINE,
        };
        2. * modal::PADDING + LINE + GAP + SMALL_LINE + names + 2. * GAP + modal::FOOTER
    }

    fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.discarding {
            return;
        }
        self.discarding = true;
        self.error = None;
        cx.notify();
        let list = self.list.clone();
        let done = (self.checkout).update(cx, |checkout, cx| checkout.act(Git::discard, list, cx));
        cx.spawn_in(window, async move |this, cx| {
            let done = done.await;
            this.update_in(cx, |this, window, cx| match done {
                Ok(()) => modal::dismiss(window, cx),
                Err(error) => {
                    this.discarding = false;
                    modal::fail(&mut this.error, "discard the changes", error.message);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

impl Render for DiscardDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let trashing = (self.list.iter()).any(|change| change.discarding() == Discarding::Trashed);
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(gpui_kit::px(GAP))
            .child(modal::heading(self.title.clone()))
            .child(div().text_xs().text_color(p.muted).child(if trashing {
                "Changes can't be undone. Trashed files can be put back from the Trash."
            } else {
                "This can't be undone."
            }))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .text_color(p.muted)
                    .children(
                        self.names
                            .iter()
                            .map(|name| small_line().child(name.clone())),
                    ),
            )
            .child(
                modal::footer(self.error.clone(), p)
                    .child(modal::cancel("discard-cancel"))
                    .child(
                        Button::new("discard-confirm", self.confirm, Tone::Danger)
                            .busy(self.discarding)
                            .on_click(cx.listener(|this, _, window, cx| this.discard(window, cx))),
                    ),
            )
    }
}
