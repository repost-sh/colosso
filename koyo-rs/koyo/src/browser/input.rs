//! The person's input to a page, as far as the app hears of it: `AppKit` hands a page its
//! pointer, keys and text itself, and the app hears a press on its way there, a key of the
//! person's the host tells of, and the edit commands the app binds. Each takes the page for
//! the person (`control`).

use gpui_kit::{
    Context, CursorStyle, Div, InteractiveElement as _, MouseButton, MouseDownEvent,
    NavigationDirection, Stateful, Window,
};
use koyo_browser_ipc::{self as ipc, AppRequest, Cursor, Go};

use super::control::Mode;
use super::view::BrowserView;
use super::{
    Back, Copy, Cut, Forward, Paste, Redo, Reload, SelectAll, ToggleDevTools, ToggleResponsive,
    Undo,
};

impl BrowserView {
    /// A press on the page, on its way to it: GPUI's focus follows it into the page, and the
    /// person has the page. The mouse's side buttons step through the page's history, as in
    /// any browser; `DevTools` have none worth stepping through.
    pub(super) fn pressed_at(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let MouseButton::Navigate(direction) = event.button {
            if self.inspected().is_none() {
                self.go(history_step(direction), cx);
            }
            return;
        }
        window.focus(&self.focus, cx);
        self.person_acts(Mode::Implicit, cx);
        self.resolve_menu(cx);
    }

    /// The person pressed a key in the page, as the host says (`HostEvent::Typed`): `AppKit`
    /// hands the page its keys without GPUI hearing of them. They're at the page, as when they
    /// press on it.
    pub(super) fn typed(&mut self, cx: &mut Context<Self>) {
        self.person_acts(Mode::Implicit, cx);
    }

    fn edit(&mut self, command: ipc::Edit, cx: &mut Context<Self>) {
        self.person_acts(Mode::Implicit, cx);
        let pane = self.pane;
        self.send_to_page(AppRequest::Edit { pane, command }, cx);
    }
}

/// The edits Koyo binds (⌘C and the rest), done in the page. All a page hears of the keyboard
/// through GPUI: `AppKit` hands it its keys itself, GPUI hearing only the shortcuts it's asked
/// about first.
pub(super) fn edits(page: Stateful<Div>, cx: &mut Context<BrowserView>) -> Stateful<Div> {
    page.on_action(cx.listener(|this, _: &Copy, _, cx| this.edit(ipc::Edit::Copy, cx)))
        .on_action(cx.listener(|this, _: &Cut, _, cx| this.edit(ipc::Edit::Cut, cx)))
        .on_action(cx.listener(|this, _: &Paste, _, cx| this.edit(ipc::Edit::Paste, cx)))
        .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.edit(ipc::Edit::SelectAll, cx)))
        .on_action(cx.listener(|this, _: &Undo, _, cx| this.edit(ipc::Edit::Undo, cx)))
        .on_action(cx.listener(|this, _: &Redo, _, cx| this.edit(ipc::Edit::Redo, cx)))
}

/// `view` hearing the keys that steer the page from anywhere in its pane: the page, the
/// address bar and the find bar.
pub(super) fn steer(view: Div, cx: &mut Context<BrowserView>) -> Div {
    view.on_action(cx.listener(|this, _: &Back, _, cx| this.go(Go::Back, cx)))
        .on_action(cx.listener(|this, _: &Forward, _, cx| this.go(Go::Forward, cx)))
        .on_action(cx.listener(|this, _: &Reload, _, cx| this.go(Go::Reload, cx)))
        .on_action(cx.listener(|this, _: &ToggleDevTools, window, cx| {
            this.toggle_devtools(window, cx);
        }))
        .on_action(cx.listener(|this, _: &ToggleResponsive, _, cx| this.toggle_responsive(cx)))
}

/// Where a mouse side button takes the page.
pub(super) fn history_step(direction: NavigationDirection) -> Go {
    match direction {
        NavigationDirection::Back => Go::Back,
        NavigationDirection::Forward => Go::Forward,
    }
}

pub(super) fn cursor_style(cursor: Cursor) -> CursorStyle {
    match cursor {
        Cursor::Arrow | Cursor::Wait | Cursor::None => CursorStyle::Arrow,
        Cursor::Pointer => CursorStyle::PointingHand,
        Cursor::Text => CursorStyle::IBeam,
        Cursor::Crosshair => CursorStyle::Crosshair,
        Cursor::NotAllowed => CursorStyle::OperationNotAllowed,
        Cursor::Grab => CursorStyle::OpenHand,
        Cursor::Grabbing => CursorStyle::ClosedHand,
        Cursor::ResizeColumn => CursorStyle::ResizeLeftRight,
        Cursor::ResizeRow => CursorStyle::ResizeUpDown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mouses_side_buttons_step_through_the_pages_history() {
        assert_eq!(history_step(NavigationDirection::Back), Go::Back);
        assert_eq!(history_step(NavigationDirection::Forward), Go::Forward);
    }
}
