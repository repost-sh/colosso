//! The top bar's buttons for the right panel: the changes, then the files. Each opens the
//! panel on its view, and closes it when that view is already shown.

use gpui_kit::assets::IconName;
use gpui_kit::component::Selectable as _;
use gpui_kit::component::button::Button;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, ParentElement as _, Styled as _, div};

use super::{ToggleChanges, ToggleFiles, View};
use crate::changes;
use crate::shell::{Shell, glass, icon};
use crate::theme;

impl Shell {
    /// Opens and closes the changes, saying how many lines they add and remove.
    pub fn changes_button(&self, cx: &App) -> Button {
        let (added, removed) = self.right_panel.totals(cx);
        glass("toggle-changes", theme::palette(cx).text, cx)
            .selected(self.right_panel.shows(View::Changes))
            .tooltip_with_action("Changes", &ToggleChanges, None)
            .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleChanges), cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .when(added + removed > 0, |button| {
                        button.child(changes::counts(added, removed, theme::palette(cx)))
                    })
                    .child(icon(IconName::FileDiff, 16.)),
            )
    }

    /// Opens and closes the checkout's files.
    pub fn files_button(&self, cx: &App) -> Button {
        glass("toggle-files", theme::palette(cx).text, cx)
            .selected(self.right_panel.shows(View::Files))
            .tooltip_with_action("Files", &ToggleFiles, None)
            .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleFiles), cx))
            .child(icon(IconName::FolderTree, 16.))
    }
}
