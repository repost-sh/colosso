//! How a tab shows in a list: its pane's icon, a page's own icon for a browser or the
//! file's for a file, and its title, in italics for a preview; a split shows each of its
//! panes as a chip in one capsule, side by side, each with its own button that closes that
//! pane alone, a dot in its place while the pane's file has unsaved edits.

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Axis, Context, Div, Image, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Styled, div, img, px,
};

use super::Shell;
use super::elements::{ellipsized, icon, pane_close_button};
use super::tab_list::title_with_end;
use crate::files::{FilePane, file_icon};
use crate::model::{Content, Editor, Pane, Tab};
use crate::pane::{CLOSE, ROW_GLYPH, row_sized};
use crate::theme::Palette;

/// The widest a chip of a split gets in the strip.
const CHIP_MAX: f32 = 180.;
/// How round the corners of a page's own icon are, wherever a tab shows it: a row down the
/// sidebar, a tab along the top rail, a pinned page's tile.
pub const FAVICON_CORNERS: f32 = 2.;

/// What a pane shows as in a tab: its icon, or its page's own, and its title.
#[derive(Clone)]
pub struct Face {
    /// The pane's.
    pub id: u64,
    pub icon: IconName,
    pub favicon: Option<Arc<Image>>,
    pub title: SharedString,
    /// Whether it's a file the next file previewed takes the place of.
    pub preview: bool,
    /// Whether it's a file with unsaved edits.
    pub dirty: bool,
}

impl Face {
    pub fn of(pane: &Pane, cx: &App) -> Self {
        let file = pane.file().map(|file| file.read(cx));
        Face {
            id: pane.id,
            icon: match &pane.content {
                Content::Editor(Editor::File(_)) => file_icon(&pane.title),
                Content::Editor(Editor::Changes(_)) => IconName::FileDiff,
                Content::Editor(Editor::Untitled)
                | Content::Chat(_)
                | Content::Terminal(_)
                | Content::Browser(_)
                | Content::Desktop(_)
                | Content::Device(_) => pane.kind().icon(),
            },
            favicon: pane.favicon(),
            title: pane.title.clone(),
            preview: file.is_some_and(FilePane::is_preview),
            dirty: file.is_some_and(FilePane::is_dirty),
        }
    }

    /// Its title, cut short to fit, in italics for a preview.
    pub fn label(&self) -> Div {
        ellipsized(self.title.clone()).when(self.preview, Styled::italic)
    }

    /// Each pane of `tab`, left to right.
    pub fn all(tab: &Tab, cx: &App) -> Vec<Self> {
        tab.panes()
            .into_iter()
            .map(|pane| Face::of(pane, cx))
            .collect()
    }

    /// Its icon, `size` points square: the page's own when it has one, its corners a little
    /// rounded.
    pub fn icon(&self, size: f32) -> AnyElement {
        match &self.favicon {
            Some(favicon) => img(favicon.clone())
                .size(px(size))
                .rounded(px(FAVICON_CORNERS))
                .flex_shrink_0()
                .into_any_element(),
            None => icon(self.icon, size).into_any_element(),
        }
    }

    /// Its icon and title as a chip, as each pane of a split shows.
    pub fn chip(&self) -> Div {
        div()
            .h(px(22.))
            .min_w_0()
            .flex()
            .items_center()
            .gap_1p5()
            .rounded(px(5.))
            .child(self.icon(13.))
            .child(self.label().flex_grow(1.))
    }
}

/// A split tab's panes, by their `faces`, in a list along `axis`. Down the sidebar they're as
/// Arc has them (`split_halves`); along the strip they're chips, each as wide as its title, to a
/// point, each with its close button.
pub fn split_chips(faces: &[Face], axis: Axis, p: Palette, cx: &mut Context<Shell>) -> Div {
    if axis == Axis::Vertical {
        return split_halves(faces, p, cx);
    }
    let chips = faces.iter().map(|face| {
        let close = pane_close_button("pane-chip-close", face.id, (face.dirty, CLOSE), p, cx);
        face.chip()
            .max_w(px(CHIP_MAX))
            .pl_1p5()
            .pr_0p5()
            .bg(p.hover)
            .child(close)
    });
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap_1()
        .children(chips)
}

/// A split tab's panes down the sidebar, as Arc has them (`docs/sidebar.md` 5).
fn split_halves(faces: &[Face], p: Palette, cx: &mut Context<Shell>) -> Div {
    let mut parts: Vec<AnyElement> = Vec::new();
    for (at, face) in faces.iter().enumerate() {
        if at > 0 {
            parts.push(
                div()
                    .w(px(1.))
                    .h(px(16.))
                    .flex_shrink_0()
                    .bg(p.text.opacity(0.2))
                    .into_any_element(),
            );
        }
        let close = pane_close_button("pane-chip-close", face.id, (face.dirty, ROW_GLYPH), p, cx);
        let close = row_sized(close).when(!face.dirty, |close| {
            close
                .opacity(0.)
                .group_hover("pane-half", |style| style.opacity(1.))
        });
        let title = div()
            .id(("pane-half-title", face.id))
            .flex()
            .child(face.label().flex_1());
        parts.push(
            div()
                .group("pane-half")
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(p.text.opacity(0.75))
                        .child(face.icon(16.)),
                )
                .child(title_with_end(title, close, face.dirty, "pane-half"))
                .into_any_element(),
        );
    }
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .items_center()
        .gap(px(10.))
        .children(parts)
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use gpui_kit::assets::IconName;

    use super::Face;
    use crate::files::tests::support::{editing, typed, with_shell};
    use crate::files::{At, Place};
    use crate::model::{Pane, PaneKind};
    use crate::test_support::{repo, select, shell_with};

    /// A tab shows the icon of what its pane shows: a file's own, the diffs', else its kind's,
    /// whether the file or the diffs are open yet or wait for the tab to be shown.
    #[gpui_kit::test]
    fn a_tab_shows_the_icon_of_what_its_pane_shows(cx: &mut TestAppContext) {
        let icon = |pane: Pane, cx: &mut TestAppContext| cx.update(|cx| Face::of(&pane, cx).icon);
        let file = Pane::of_path("/src/app/src/main.rs".into());
        assert_eq!(icon(file, cx), IconName::FileCode);
        let changes = Pane::of_changes(None);
        assert_eq!(icon(changes, cx), IconName::FileDiff);
        let kinds = PaneKind::ALL.map(|kind| icon(Pane::new(kind), cx));
        let icons = [
            IconName::MessageSquare,
            IconName::SquareTerminal,
            IconName::Globe,
            IconName::FileCode,
            IconName::Bot,
            IconName::Smartphone,
        ];
        assert_eq!(kinds, icons, "chat, terminal, page, editor, desktop, device");
    }

    /// An open file's tab shows its icon, and says whether it's the preview and whether it
    /// has unsaved edits.
    #[gpui_kit::test]
    fn a_files_tab_says_whether_its_the_preview_and_unsaved(cx: &mut TestAppContext) {
        let root = repo("tab-face-file", &[("README.md", "hi\n")]);
        let (window, shell) = shell_with(&[&root], cx);
        select(&shell, &root, cx);
        let face = |cx: &mut TestAppContext| {
            cx.read(|cx| {
                let shell = shell.read(cx);
                let pane = shell.workspace.tab_shown().expect("the file's tab").first();
                let face = Face::of(pane, cx);
                (face.icon, face.preview, face.dirty)
            })
        };
        with_shell(window, &shell, cx, |shell, window, cx| {
            shell.preview_file(Place::file("README.md"), window, cx);
        });
        cx.run_until_parked();
        assert_eq!(face(cx), (IconName::FileText, true, false), "previewed");
        let (_, editor, _) = editing(window, &shell, "README.md", At::Bytes(0..0), cx);
        typed(window, &editor, "x", cx);
        assert_eq!(face(cx), (IconName::FileText, false, true), "kept, and typed in");
    }
}
