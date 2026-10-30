//! The window's actions: those the shell answers itself, their keys, and the handlers on
//! the window's root that hear them wherever the keyboard is.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Div, InteractiveElement as _, KeyBinding, MouseButton, Window, actions,
};

use super::Shell;
use crate::files::{self, CloseTab};
use crate::model::PaneKind;
use crate::navigation::{CloseSettings, GoBack, GoForward, Navigate, NavigationHistory, Route};
use crate::projects::{self, NewWorktree};
use crate::settings;
use crate::settings_view::{self, EditBranchPrefix};
use crate::{modal, picker};

actions!(
    colosso,
    [
        ToggleSidebar,
        ToggleSidebarMode,
        ToggleRightPanel,
        ToggleBottomPanel,
        NewChat,
        NewTerminal,
        NewBrowserTab,
        NewEditorTab,
        EditTheme
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-alt-b", ToggleRightPanel, None),
        KeyBinding::new("cmd-j", ToggleBottomPanel, None),
        KeyBinding::new("cmd-t", NewChat, None),
        KeyBinding::new("cmd-shift-m", ToggleSidebarMode, None),
        KeyBinding::new("cmd-n", NewWorktree, None),
    ]);
    // The layout is a setting, so this works from any window or menu.
    cx.on_action(|_: &ToggleSidebarMode, cx| {
        settings::update(cx, |settings| settings.layout = settings.layout.other());
    });
}

impl Shell {
    /// What the window does with its actions, presses and drags, wherever they land.
    pub(super) fn handle_actions(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, Navigate(route), _, cx| this.navigate(*route, cx)))
            .on_action(cx.listener(Self::open_settings))
            .on_action(
                cx.listener(|this, _: &CloseSettings, _, cx| this.navigate(Route::Workspace, cx)),
            )
            .on_action(cx.listener(|this, _: &GoBack, _, cx| {
                this.update_history(NavigationHistory::go_back, cx);
            }))
            .on_action(cx.listener(|this, _: &GoForward, _, cx| {
                this.update_history(NavigationHistory::go_forward, cx);
            }))
            .on_action(cx.listener(Self::toggle_sidebar))
            .map(|root| Self::handle_right_panel(root, cx))
            .on_action(cx.listener(Self::toggle_bottom_panel))
            .on_action(cx.listener(|this, _: &NewChat, _, cx| this.new_tab(PaneKind::Chat, cx)))
            // ⌘W closes the tab in the middle once the Files panel, heard first, has passed
            // on it: no settings page or file shown to close instead.
            .on_action(cx.listener(|this, _: &CloseTab, _, cx| {
                this.workspace.close_active_tab();
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &NewTerminal, _, cx| this.new_tab(PaneKind::Terminal, cx)),
            )
            .on_action(
                cx.listener(|this, _: &NewBrowserTab, _, cx| this.new_tab(PaneKind::Browser, cx)),
            )
            .on_action(
                cx.listener(|this, _: &NewEditorTab, _, cx| this.new_tab(PaneKind::Editor, cx)),
            )
            .map(|root| projects::handle_actions(root, cx))
            .on_action(|_: &EditBranchPrefix, window, cx| {
                settings_view::edit_branch_prefix(window, cx);
            })
            .on_drag_move(cx.listener(Self::resize_sidebar))
            // A press in the window puts the picker and any dialog over it away, so what's
            // typed next goes where the user pressed. It's heard on the way down: text fields
            // stop presses from reaching anything around them.
            .capture_any_mouse_down(|_, _, cx| {
                picker::close(cx);
                modal::close(cx);
            })
            .on_action(cx.listener(|this, _: &EditTheme, window, cx| {
                picker::open(this.rails_menu_at, window, cx);
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::show_rails_menu))
    }
}

impl Shell {
    /// Has `window`'s red button ask about unsaved edits. No key closes the window: ⌘W closes
    /// what's shown in it, and the dock icon opens it again once closed.
    pub(super) fn guard_closing(window: &Window, cx: &mut Context<Self>) {
        let shell = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            (shell.update(cx, |shell, cx| shell.may_close(window, cx))).unwrap_or(true)
        });
    }

    /// Whether the window may close now: with unsaved edits in it, it asks about them
    /// instead, and the answer closes it; asked already, it waits for the answer.
    fn may_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if files::asking(cx) {
            return false;
        }
        let unsaved = self.right_panel.unsaved(cx);
        if unsaved.is_empty() {
            return true;
        }
        files::ask_before_closing(unsaved, window, cx);
        false
    }
}
