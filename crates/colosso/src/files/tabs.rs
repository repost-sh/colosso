//! The files open beside the tree, each in a tab: one of them may be a preview, shown in
//! italics, which the next file previewed takes the place of until it's pinned by a double
//! click on it or on its tab, or by Enter in the tree. A tab whose file has unsaved edits
//! shows a dot in place of its close button, and asks before it closes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, ClickEvent, Context, Div, EmptyView, Entity, EntityId,
    InteractiveElement as _, IntoElement as _, MouseButton, ParentElement as _, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled, Subscription, TaskExt as _, Window, div,
    px,
};
pub use strip::Hand;
use strip::Strip;

use super::panel::FilesPanel;
use super::{FileEditor, Place, closing, file_icon};
use crate::checkout::Document;
use crate::language::{self, Found};
use crate::pane::{close_button, tab_button};
use crate::settings;
use crate::shell::{ellipsized, icon, row};
use crate::theme::Palette;

/// A file open in a tab, and the editor showing it.
pub struct Viewed {
    /// Tells the tab apart for as long as it's open: the preview tab keeps its key as its
    /// file changes.
    pub key: EntityId,
    /// Its absolute path.
    pub path: PathBuf,
    /// Its path as the tree names it; `None` for a file outside the checkout.
    pub relative: Option<Arc<str>>,
    /// The checkout's document for it, held open by the tab.
    pub document: Entity<Document>,
    pub editor: Entity<FileEditor>,
    /// Kept open when another file is previewed, rather than giving way to it.
    pub pinned: bool,
    /// Its turn the last time it was shown, later tabs' higher: the switcher lists the latest
    /// first.
    pub shown_at: u64,
    _going: Subscription,
    /// The places a symbol in it led to are listed under it.
    _found: Subscription,
    /// Drawn again as its document's state changes: unsaved, or saved.
    _document: Subscription,
}

impl FilesPanel {
    /// The file shown beside the tree, and the editor showing it.
    pub fn viewing(&self) -> Option<&Viewed> {
        self.tabs.get(self.shown)
    }

    /// The files open, in their tabs' order.
    #[cfg(test)]
    pub fn tabs(&self) -> &[Viewed] {
        &self.tabs
    }

    /// Shows the file `place` is in, in a tab of its own; picks it in the tree when it's in
    /// the checkout; puts the cursor where the place says; and gives the editor the keyboard.
    pub fn view(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        self.open(place, true, true, window, cx);
    }

    /// Shows the file `place` is in, in the preview tab, unless it's open already or
    /// previews are turned off, when it's in a tab of its own; and gives the editor the
    /// keyboard.
    pub fn preview(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        let pinned = !settings::get(cx).preview_files;
        self.open(place, pinned, true, window, cx);
    }

    /// As `preview`, the keyboard staying where it is: for the tree and the lists, which
    /// show what's moved over.
    pub fn peek(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        let pinned = !settings::get(cx).preview_files;
        self.open(place, pinned, false, window, cx);
    }

    /// `focus` says whether the editor shown takes the keyboard.
    pub(super) fn open(
        &mut self,
        place: Place,
        pinned: bool,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = self.checkout.read(cx).root().clone();
        let path = root.join(&place.path);
        let at = if let Some(at) = self.tabs.iter().position(|tab| tab.path == path) {
            self.tabs[at].pinned |= pinned;
            at
        } else {
            let viewed = self.viewed(path, &root, pinned, window, cx);
            let preview = self.tabs.iter().position(|tab| !tab.pinned);
            if let Some(at) = preview.filter(|_| !pinned) {
                let key = self.tabs[at].key;
                self.tabs[at] = Viewed { key, ..viewed };
                at
            } else {
                let at = (self.shown + 1).min(self.tabs.len());
                self.tabs.insert(at, viewed);
                at
            }
        };
        self.tabs[at].editor.update(cx, |editor, cx| {
            if let Some(at) = place.at {
                editor.reveal(at, cx);
            }
            if focus {
                editor.focus_editor(window, cx);
            }
        });
        self.show_tab(at, cx);
    }

    /// The file at `path` in an editor of its own, which shows the definitions it leads to
    /// in other files as previews.
    fn viewed(
        &self,
        path: PathBuf,
        root: &Path,
        pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Viewed {
        let checkout = self.checkout.clone();
        let document = checkout.update(cx, |checkout, cx| checkout.document(&path, cx));
        let relative = (path.strip_prefix(root).ok())
            .and_then(Path::to_str)
            .map(Arc::from);
        // A file of the checkout is edited in place; one outside it is only read.
        let editable = relative.is_some();
        let editor = cx.new(|cx| FileEditor::new(&document, editable, window, cx));
        language::attach(&checkout, &editor, cx);
        let _going = cx.subscribe_in(&editor, window, |this, _, place: &Place, window, cx| {
            this.preview(place.clone(), window, cx);
        });
        let _found = cx.subscribe_in(&editor, window, |this, _, found: &Found, window, cx| {
            this.show_locations(found.clone(), window, cx);
        });
        // A preview typed into is kept: the next file previewed mustn't take its edits.
        let _document = cx.observe(&document, |this, document, cx| {
            if document.read(cx).is_dirty()
                && let Some(tab) = this.tabs.iter_mut().find(|tab| tab.document == document)
            {
                tab.pinned = true;
            }
            cx.notify();
        });
        Viewed {
            key: editor.entity_id(),
            path,
            relative,
            document,
            editor,
            pinned,
            shown_at: 0,
            _going,
            _found,
            _document,
        }
    }

    /// Shows the tab at `at`, its file picked in the tree.
    pub(super) fn show_tab(&mut self, at: usize, cx: &mut Context<Self>) {
        self.shown = at;
        let latest = self.tabs.iter().map(|tab| tab.shown_at).max().unwrap_or(0);
        if let Some(tab) = self.tabs.get_mut(at) {
            tab.shown_at = latest + 1;
        }
        let relative = self.viewing().and_then(|viewed| viewed.relative.clone());
        self.set_active(relative, cx);
        cx.notify();
    }

    /// Shows the tab whose key is `key`, if it's still open, and gives its editor the keyboard.
    pub(super) fn switch_to(&mut self, key: EntityId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(at) = self.tabs.iter().position(|tab| tab.key == key) {
            self.show_tab(at, cx);
            (self.tabs[at].editor).update(cx, |editor, cx| editor.focus_editor(window, cx));
        }
    }

    /// Keeps the tab at `at` open when another file's previewed.
    pub(super) fn pin(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(at) {
            tab.pinned = true;
            cx.notify();
        }
    }

    /// The editors of the files open with unsaved edits.
    pub fn unsaved(&self, cx: &App) -> Vec<Entity<FileEditor>> {
        (self.tabs.iter())
            .filter(|tab| tab.document.read(cx).is_dirty())
            .map(|tab| tab.editor.clone())
            .collect()
    }

    /// Closes the tab at `at`, asking first when its file has unsaved edits.
    pub(super) fn close(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(at) else {
            return;
        };
        if tab.document.read(cx).is_dirty() {
            let (editors, then) = (vec![tab.editor.clone()], closing::Then::CloseTab(cx.entity()));
            closing::ask(editors, then, window, cx);
        } else {
            self.remove(at, cx);
        }
    }

    /// Closes the tab showing `editor`, if it's still open, whatever its file has: what was
    /// asked about it is answered.
    pub(super) fn remove_view(&mut self, editor: &Entity<FileEditor>, cx: &mut Context<Self>) {
        if let Some(at) = self.tabs.iter().position(|tab| tab.editor == *editor) {
            self.remove(at, cx);
        }
    }

    /// Writes the file shown, when it has unsaved edits.
    pub(super) fn save_shown(&self, cx: &mut Context<Self>) {
        if let Some(tab) = self.viewing() {
            (tab.editor.update(cx, FileEditor::save)).detach_and_log_err(cx);
        }
    }

    /// Closes the tab at `at`, showing the one after it, or else the one before. Its
    /// document goes with it, unless another tab shows the file.
    fn remove(&mut self, at: usize, cx: &mut Context<Self>) {
        self.tabs.remove(at);
        if at < self.shown || self.shown == self.tabs.len() {
            self.shown = self.shown.saturating_sub(1);
        }
        self.show_tab(self.shown, cx);
    }

    /// Moves the tab at `from` to `to`, the tab shown staying shown.
    fn reorder(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let shown = self.viewing().map(|tab| tab.key);
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.shown = (self.tabs.iter().position(|tab| Some(tab.key) == shown)).unwrap_or(0);
        cx.notify();
    }

    /// Lets go of the tab in hand, in the slot it's over.
    fn let_go(&mut self, cx: &mut Context<Self>) {
        if let Some((key, to)) = self.hand.release()
            && let Some(from) = self.tabs.iter().position(|tab| tab.key == key)
        {
            self.reorder(from, to, cx);
        }
    }

    /// The tabs, in the panel's header, scrolled sideways when they don't fit.
    pub(super) fn tab_strip(&self, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let tabs = (self.tabs.iter().enumerate())
            .map(|(at, tab)| (tab.key, self.tab(at, tab, p, cx).into_any_element()))
            .collect();
        // While a tab's in hand the window's drawn on each move of the mouse, which the
        // strip reads for itself. Let go wherever the pointer is.
        div()
            .id("files-tabs")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .overflow_x_scroll()
            .capture_any_mouse_up(cx.listener(|this, _, _, cx| this.let_go(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.let_go(cx)))
            .child(Strip::new(tabs, self.hand.clone(), px(GAP)))
    }

    fn tab(&self, at: usize, tab: &Viewed, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let shown = at == self.shown;
        let key = tab.key;
        let lifted = self.hand.holding() == Some(key);
        let name = name(&tab.path);
        let dirty = tab.document.read(cx).is_dirty();
        let close = if dirty {
            tab_button(("files-tab-close", key.as_u64()), p).child(unsaved_dot(p))
        } else {
            close_button(("files-tab-close", key.as_u64()), p)
        };
        let close = close.on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            // Let go of a tab taken up by its button, it's not a click.
            if !cx.has_active_drag() {
                this.close(at, window, cx);
            }
        }));
        let hand = self.hand.clone();
        row(("files-tab", key.as_u64()), shown, p)
            .h(px(TAB))
            .flex_shrink_0()
            .max_w(px(200.))
            .pl_2()
            .pr_0p5()
            .gap_1p5()
            .rounded(px(5.))
            .text_xs()
            .when(!shown, |tab| tab.text_color(p.muted))
            // In hand, it's lifted off the strip, over the tabs it passes.
            .when(lifted, |tab| tab.bg(p.hover).shadow_sm())
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() > 1 {
                    this.pin(at, cx);
                }
                this.show_tab(at, cx);
            }))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, window, cx| this.close(at, window, cx)),
            )
            // The tab itself follows the mouse; nothing else is drawn under it.
            .on_drag(DraggedTab, move |_, grab, _, cx| {
                hand.take(key, at, grab.x);
                cx.new(|_| EmptyView)
            })
            .child(icon(file_icon(&name), 13.).text_color(p.muted))
            .child(ellipsized(SharedString::from(name)).when(!tab.pinned, Styled::italic))
            .child(close)
    }
}

/// The dot that marks a file with unsaved edits, on its tab and in the switcher.
pub(super) fn unsaved_dot(p: Palette) -> Div {
    div().size(px(7.)).flex_shrink_0().rounded_full().bg(p.text)
}

/// A file's name, as its tab shows it: the whole path when it has none.
pub(super) fn name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// A tab's height, in the header.
const TAB: f32 = 26.;
/// The space between tabs.
const GAP: f32 = 2.;

/// What gpui carries while a tab's dragged: nothing, the strip drawing the tab itself.
struct DraggedTab;

mod strip;

#[cfg(test)]
mod tests;
