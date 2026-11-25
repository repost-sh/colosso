//! A branch's buttons for the right panel: the changes, then the files, where Changes and Files
//! places them (`docs/layout.md`): on the branch's row in the sidebar, or at the top rail's end
//! for the selected branch. Each shows the branch's panel on its view, selecting the branch
//! first if it isn't, and closes it once the branch's selected with that view shown, lit while
//! it is. The changes' says how many lines the selected branch's changes add and remove.
//! Right-clicked, either opens the menu of where the two are placed.

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
use crate::settings;
use crate::shell::{Shell, icon, key_of, placement_menu, row_button, with_menu};
use crate::theme;

impl Shell {
    /// The Changes and Files buttons of `branch`, its place as `(project, branch)` and its
    /// worktree's path, for its row in the sidebar while they're placed on it: both while the
    /// pointer's on the row, faded in as far as `hovered` says (none away from it, all on it),
    /// and otherwise only one whose panel is open, which stays. `look` is how big their glyphs
    /// are and what fills a lit one, as `panel_buttons` takes it. None while they're at the top
    /// rail's end (`Settings::panel_buttons_on_rail`).
    pub(crate) fn row_panel_buttons(
        &self,
        branch: ((usize, usize), &Path),
        look: (f32, Hsla),
        hovered: f32,
        cx: &mut Context<Self>,
    ) -> Vec<Stateful<Div>> {
        if settings::get(cx).panel_buttons_on_rail() {
            return Vec::new();
        }
        (self.panel_buttons(branch, look, hovered > 0., cx).into_iter())
            .map(|(button, lit)| button.opacity(if lit { 1. } else { hovered }))
            .collect()
    }

    /// The changes' and the files' buttons of `branch`, its place as `(project, branch)` and its
    /// worktree's path, each with whether it's lit, its panel open on its view: both if `all`,
    /// and otherwise only one that's lit. Their glyphs are `glyph` big, and a lit one is filled
    /// with `fill`.
    pub(crate) fn panel_buttons(
        &self,
        branch: ((usize, usize), &Path),
        (glyph, fill): (f32, Hsla),
        all: bool,
        cx: &mut Context<Self>,
    ) -> Vec<(Stateful<Div>, bool)> {
        let here = self.has_right_panel();
        [View::Changes, View::Files]
            .into_iter()
            .map(|view| (view, here && self.right_panel.shows_in(branch.1, view)))
            .filter(|&(_, lit)| all || lit)
            .map(|(view, lit)| (self.panel_button(branch, (view, lit), (glyph, fill), cx), lit))
            .collect()
    }

    fn panel_button(
        &self,
        ((project, branch), root): ((usize, usize), &Path),
        (view, lit): (View, bool),
        (glyph, fill): (f32, Hsla),
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
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
        let button = row_button(ElementId::NamedInteger(name.into(), key_of(root)), p)
            .debug_selector(move || format!("{name}-{project}-{branch}"))
            .when(lit, |button| button.bg(fill))
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
        with_menu(button, |_, cx| placement_menu(cx))
    }
}
