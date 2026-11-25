//! The shell's side of the Settings tab (`docs/sidebar.md` 11.9): opened by ⌘,, the menus and
//! app control, or picked again if it's open; its pages shown from its own list, Back and Forward
//! walking them; closed by its ×, a middle click, ⌘W or Escape, and by showing anything else
//! (`model/settings_tab.rs`). Its row or tab comes after the space's loose pages.

use gpui_kit::{
    Context, Div, InteractiveElement as _, MouseButton, Stateful,
    StatefulInteractiveElement as _,
};

use super::Shell;
use crate::model::SettingsTab;
use crate::navigation::SettingsPage;

/// `row`, the Settings tab's in a list of tabs: closed by a middle click, as a loose page's is.
pub fn settings_row(row: Stateful<Div>, cx: &mut Context<Shell>) -> Stateful<Div> {
    row.on_mouse_down(
        MouseButton::Middle,
        cx.listener(|this, _, _, cx| this.close_settings(cx)),
    )
}

/// `button`, the Settings tab's ×: pressed, the tab closes.
pub fn settings_close(button: Stateful<Div>, cx: &mut Context<Shell>) -> Stateful<Div> {
    button
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, _, cx| {
            cx.stop_propagation();
            this.close_settings(cx);
        }))
}

impl Shell {
    /// Opens the Settings tab on `page`, over the tab shown, as ⌘, does: picked again if it's
    /// open, on `page` if one's asked for, else on the page it shows, or the last one showed.
    pub(crate) fn open_settings_page(
        &mut self,
        page: Option<SettingsPage>,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.open_settings(page) {
            self.went(cx);
        }
    }

    /// Moves through the Settings tab's pages with `step`, as Back and Forward do, which reports
    /// whether it showed another page. Nothing while the tab isn't open.
    pub(super) fn step_settings(
        &mut self,
        step: impl FnOnce(&mut SettingsTab) -> bool,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.settings_mut().is_some_and(step) {
            self.went(cx);
        }
    }

    /// Closes the Settings tab, as its ×, ⌘W, Escape and app control's `settings_close` do: the
    /// tab it was shown over shows again.
    pub(crate) fn close_settings(&mut self, cx: &mut Context<Self>) {
        if self.workspace.close_settings() {
            cx.notify();
        }
    }

    /// The Settings tab shows another page: Devices lists Koyo's simulators afresh each time it
    /// shows, and the window is drawn again.
    fn went(&mut self, cx: &mut Context<Self>) {
        if self.shows(SettingsPage::Devices) {
            crate::devices::devices(cx).update(cx, crate::devices::Devices::refresh);
        }
        cx.notify();
    }

    /// Whether the Settings tab shows `page`.
    pub(super) fn shows(&self, page: SettingsPage) -> bool {
        (self.workspace.settings()).is_some_and(|tab| tab.page() == page)
    }
}

#[cfg(test)]
mod tests;
