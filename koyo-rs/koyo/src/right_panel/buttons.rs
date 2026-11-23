//! A worktree's buttons for the right panel: the changes, then the files, on its header in the
//! Arc sidebar and at the Orca top rail's end for the selected worktree. Each shows the
//! worktree's panel on its view, selecting the worktree first if it isn't, and closes it once the
//! worktree's selected with that view shown, lit while it is. The changes' says how many lines
//! the selected worktree's changes add and remove.

use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, Context, Div, ElementId, Hsla, InteractiveElement as _, MouseButton,
    ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _, px,
};

use super::{ToggleChanges, ToggleFiles, View};
use crate::changes;
use crate::shell::{Shell, icon, key_of, row_button};
use crate::theme;

impl Shell {
    /// The changes' and the files' buttons of the worktree at `at`, as `(project, branch)`, each
    /// with whether it's lit: both if `all`, and otherwise only one that's lit. Their glyphs are
    /// `glyph` big, and a lit one is filled with `lit`.
    pub(crate) fn panel_buttons(
        &self,
        at: (usize, usize),
        (glyph, lit): (f32, Hsla),
        all: bool,
        cx: &mut Context<Self>,
    ) -> Vec<(Stateful<Div>, bool)> {
        let unit = (self.workspace.projects.get(at.0)).and_then(|it| it.branches.get(at.1));
        let Some(root) = unit.map(|unit| unit.worktree.path.as_path()) else {
            return Vec::new();
        };
        [View::Changes, View::Files]
            .into_iter()
            .filter(|&view| all || self.right_panel.shows_in(root, view))
            .map(|view| self.panel_button((at, root), view, (glyph, lit), cx))
            .collect()
    }

    fn panel_button(
        &self,
        ((project, branch), root): ((usize, usize), &Path),
        view: View,
        (glyph, lit_in): (f32, Hsla),
        cx: &mut Context<Self>,
    ) -> (Stateful<Div>, bool) {
        let (name, icon_name, says, action, (added, removed)): (_, _, _, Box<dyn Action>, _) =
            match view {
                View::Changes => (
                    "toggle-changes",
                    IconName::FileDiff,
                    "Changes",
                    Box::new(ToggleChanges),
                    self.right_panel.totals_in(root),
                ),
                View::Files => (
                    "toggle-files",
                    IconName::FolderTree,
                    "Files",
                    Box::new(ToggleFiles),
                    (0, 0),
                ),
            };
        let p = theme::palette(cx);
        let lit = self.right_panel.shows_in(root, view);
        let button = row_button(ElementId::NamedInteger(name.into(), key_of(root)), p)
            .debug_selector(move || format!("{name}-{project}-{branch}"))
            .when(lit, |button| button.bg(lit_in))
            .when(added + removed > 0, |button| {
                let counts = changes::counts(added, removed, p);
                button.w_auto().px(px(6.)).gap_1p5().child(counts)
            })
            .child(icon(icon_name, glyph))
            .tooltip(move |window, cx| {
                Tooltip::new(says)
                    .action(action.as_ref(), None)
                    .build(window, cx)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_in((project, branch), view, cx);
                }),
            );
        (button, lit)
    }
}
