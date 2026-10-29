//! The files open beside the tree, each in a tab: one of them may be a preview, shown in
//! italics, which the next file previewed takes the place of until it's pinned by a double
//! click on it or on its tab, or by Enter in the tree.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, ClickEvent, Context, Div, Entity, InteractiveElement as _, MouseButton,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled,
    Subscription, Window, div, px,
};

use super::panel::FilesPanel;
use super::{FileEditor, Place, file_icon};
use crate::language;
use crate::pane::close_button;
use crate::settings;
use crate::shell::{ellipsized, icon, row};
use crate::theme::Palette;

/// A file open in a tab, and the editor showing it.
pub struct Viewed {
    /// Its absolute path.
    pub path: PathBuf,
    /// Its path as the tree names it; `None` for a file outside the checkout.
    pub relative: Option<Arc<str>>,
    pub editor: Entity<FileEditor>,
    /// Kept open when another file is previewed, rather than giving way to it.
    pub pinned: bool,
    _going: Subscription,
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
    /// the checkout; and puts the cursor where the place says.
    pub fn view(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        self.open(place, true, window, cx);
    }

    /// Shows the file `place` is in, in the preview tab, unless it's open already or
    /// previews are turned off, when it's in a tab of its own.
    pub fn preview(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        let pinned = !settings::get(cx).preview_files;
        self.open(place, pinned, window, cx);
    }

    fn open(&mut self, place: Place, pinned: bool, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.checkout.read(cx).root().clone();
        let path = root.join(&place.path);
        let at = if let Some(at) = self.tabs.iter().position(|tab| tab.path == path) {
            self.tabs[at].pinned |= pinned;
            at
        } else {
            let viewed = self.viewed(path, &root, pinned, window, cx);
            let preview = self.tabs.iter().position(|tab| !tab.pinned);
            if let Some(at) = preview.filter(|_| !pinned) {
                self.tabs[at] = viewed;
                at
            } else {
                let at = (self.shown + 1).min(self.tabs.len());
                self.tabs.insert(at, viewed);
                at
            }
        };
        if let Some(place) = place.at {
            (self.tabs[at].editor).update(cx, |editor, cx| editor.reveal(place, window, cx));
        }
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
        let editor = cx.new(|cx| FileEditor::new(document, window, cx));
        language::attach(&checkout, &editor, cx);
        let _going = cx.subscribe_in(&editor, window, |this, _, place: &Place, window, cx| {
            this.preview(place.clone(), window, cx);
        });
        let relative = (path.strip_prefix(root).ok())
            .and_then(Path::to_str)
            .map(Arc::from);
        Viewed {
            path,
            relative,
            editor,
            pinned,
            _going,
        }
    }

    /// Shows the tab at `at`, its file picked in the tree.
    pub(super) fn show_tab(&mut self, at: usize, cx: &mut Context<Self>) {
        self.shown = at;
        let relative = self.viewing().and_then(|viewed| viewed.relative.clone());
        self.set_active(relative, cx);
        cx.notify();
    }

    /// Keeps the tab at `at` open when another file's previewed.
    pub(super) fn pin(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(at) {
            tab.pinned = true;
            cx.notify();
        }
    }

    /// Closes the tab at `at`, showing the one after it, or else the one before.
    pub(super) fn close(&mut self, at: usize, cx: &mut Context<Self>) {
        if at >= self.tabs.len() {
            return;
        }
        self.tabs.remove(at);
        if at < self.shown || self.shown == self.tabs.len() {
            self.shown = self.shown.saturating_sub(1);
        }
        self.show_tab(self.shown, cx);
    }

    /// The tabs, in the panel's header, scrolled sideways when they don't fit.
    pub(super) fn tab_strip(&self, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let tabs: Vec<_> = (self.tabs.iter().enumerate())
            .map(|(at, tab)| self.tab(at, tab, p, cx))
            .collect();
        div()
            .id("files-tabs")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap_0p5()
            .overflow_x_scroll()
            .children(tabs)
    }

    fn tab(&self, at: usize, tab: &Viewed, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let shown = at == self.shown;
        let name = (tab.path.file_name())
            .map_or_else(|| tab.path.display().to_string(), |name| {
                name.to_string_lossy().into_owned()
            });
        let close = close_button(("files-tab-close", at), p).on_click(cx.listener(
            move |this, _, _, cx| {
                cx.stop_propagation();
                this.close(at, cx);
            },
        ));
        row(("files-tab", at), shown, p)
            .h(px(TAB))
            .flex_shrink_0()
            .max_w(px(200.))
            .pl_2()
            .pr_0p5()
            .gap_1p5()
            .rounded(px(5.))
            .text_xs()
            .when(!shown, |tab| tab.text_color(p.muted))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() > 1 {
                    this.pin(at, cx);
                }
                this.show_tab(at, cx);
            }))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, _, cx| this.close(at, cx)),
            )
            .child(icon(file_icon(&name), 13.).text_color(p.muted))
            .child(ellipsized(SharedString::from(name)).when(!tab.pinned, Styled::italic))
            .child(close)
    }
}

/// A tab's height, in the header.
const TAB: f32 = 26.;

#[cfg(test)]
mod tests;
