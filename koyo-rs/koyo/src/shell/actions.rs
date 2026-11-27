//! The window's actions: those the shell answers itself, their keys, and the handlers on
//! the window's root that hear them wherever the keyboard is.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Div, InteractiveElement as _, KeyBinding, KeyContext, ModifiersChangedEvent,
    MouseButton, MouseDownEvent, NavigationDirection, ParentElement as _, Window, actions,
};

use super::layout_menus::layout_items;
use super::{Shell, shown_page, spaces, switcher, with_menu};
use crate::files;
use crate::model::PaneKind;
use crate::model::SettingsTab;
use crate::navigation::{CloseSettings, GoBack, GoForward};
use crate::projects::{self, NewWorktree};
use crate::settings;
use crate::settings_view::{
    self, ClearClaudePath, EditBranchPrefix, EditClaudePath, EditPagesOpenAt, ShowPage,
};
use crate::{browser, picker, terminals};

actions!(
    koyo,
    [
        ToggleSidebar,
        ToggleRightPanel,
        ToggleBottomPanel,
        NewChat,
        NewTerminal,
        StopTerminal,
        ReopenClosedTab,
        CloseTab,
        SwitchTab,
        SwitchTabBack,
        EditTheme,
        /// Minimizes the window to the Dock. No key: ⌘M would be a key the window has never
        /// taken, and isn't bound until it's been seen on screen.
        Minimize,
        /// Zooms the window, or back. No key, as macOS gives none.
        Zoom
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-alt-b", ToggleRightPanel, None),
        KeyBinding::new("cmd-j", ToggleBottomPanel, None),
        KeyBinding::new("cmd-shift-t", ReopenClosedTab, None),
        KeyBinding::new("alt-cmd-l", browser::ToggleDownloads, None),
        KeyBinding::new("cmd-n", NewWorktree, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", SwitchTab, None),
        KeyBinding::new("ctrl-shift-tab", SwitchTabBack, None),
    ]);
}

impl Shell {
    /// The key context of the window's root: the settings' while the Settings tab is shown, else
    /// the spaces' (`spaces::bind_keys`).
    pub(super) fn root_keys(settings: bool) -> KeyContext {
        let mut keys = KeyContext::default();
        keys.add(if settings { "Settings" } else { spaces::SPACES });
        keys
    }

    /// What the window does with its actions, presses and drags, wherever they land.
    pub(super) fn handle_actions(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, ShowPage(page), _, cx| {
            this.step_settings(|tab| tab.go(*page), cx);
        }))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(|this, _: &CloseSettings, _, cx| this.close_settings(cx)))
            .on_action(cx.listener(|this, _: &GoBack, _, cx| {
                this.step_settings(SettingsTab::back, cx);
            }))
            .on_action(cx.listener(|this, _: &GoForward, _, cx| {
                this.step_settings(SettingsTab::forward, cx);
            }))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, _, cx| this.step_settings(SettingsTab::back, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _, _, cx| this.step_settings(SettingsTab::forward, cx)),
            )
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(|this, _: &browser::ToggleDownloads, window, cx| {
                browser::DownloadsButton::toggle(&this.downloads, window, cx);
            }))
            .map(|root| Self::handle_right_panel(root, cx))
            .on_action(cx.listener(Self::toggle_bottom_panel))
            .on_action(cx.listener(|this, _: &NewChat, _, cx| this.new_tab(PaneKind::Chat, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                if this.workspace.settings_shown().is_some() {
                    this.close_settings(cx);
                } else if !this.close_apart_shown(window, cx)
                    && let Some(at) = this.workspace.shown_tab()
                {
                    this.close_tab_at(at, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SwitchTab, window, cx| {
                switcher::switch(this, cx.entity(), 1, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwitchTabBack, window, cx| {
                switcher::switch(this, cx.entity(), -1, window, cx);
            }))
            .on_modifiers_changed(|event: &ModifiersChangedEvent, _, cx| {
                if !event.modifiers.control {
                    switcher::released(cx);
                }
            })
            .on_action(
                cx.listener(|this, _: &NewTerminal, _, cx| this.new_tab(PaneKind::Terminal, cx)),
            )
            .on_action(cx.listener(|this, _: &StopTerminal, window, cx| {
                this.stop_shown_terminal(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ReopenClosedTab, window, cx| {
                this.reopen_closed_tab(window, cx);
            }))
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &Zoom, window, _| window.zoom_window())
            .map(|root| Self::handle_launcher(root, cx))
            .map(|root| projects::handle_actions(root, cx))
            .map(|root| spaces::handle_actions(root, cx))
            .map(|root| shown_page::handle_actions(root, cx))
            .map(|root| Self::handle_pinned(root, cx))
            .map(|root| crate::chat::heard_at_window(root, cx))
            .map(browser::handle_actions)
            .map(|root| terminals::handle_actions(root, cx))
            .on_action(|_: &EditBranchPrefix, window, cx| {
                settings_view::edit_branch_prefix(window, cx);
            })
            .on_action(|_: &EditClaudePath, window, cx| {
                settings_view::edit_claude_path(window, cx);
            })
            .on_action(|_: &EditPagesOpenAt, window, cx| {
                settings_view::edit_pages_open_at(window, cx);
            })
            .on_action(|_: &ClearClaudePath, _, cx| {
                settings::update(cx, |settings| settings.claude_path = None);
            })
            .on_drag_move(cx.listener(Self::resize_sidebar))
            .child(Self::hearing_let_go(cx))
            .child(crate::popup::listener())
            .on_action(cx.listener(|this, _: &EditTheme, window, cx| {
                picker::open(this.right_clicked_at, window, cx);
            }))
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, _, _| {
                if event.button == MouseButton::Right {
                    this.right_clicked_at = event.position;
                }
            }))
            .map(|root| with_menu(root, |_, cx| layout_items(NativeMenu::new(), cx)))
    }
}

impl Shell {
    /// Has `window`'s red button ask about what closing it would lose. No key closes the window:
    /// ⌘W closes what's shown in it, and the dock icon opens it again once closed.
    pub(super) fn guard_closing(window: &Window, cx: &mut Context<Self>) {
        let shell = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            (shell.update(cx, |shell, cx| shell.ask_to_close(window, cx))).is_err()
        });
    }

    /// Closes the window once the unsaved edits in it are saved or let go and the person may leave
    /// its pages; asked already, it waits for the answers.
    pub(super) fn ask_to_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if files::asking(cx) {
            return;
        }
        let closing = window.window_handle();
        let close = move |cx: &mut App| {
            (closing.update(cx, |_, window, _| window.remove_window())).ok();
        };
        self.ask_all_then(close, window, cx);
    }

    /// Does `then` once the unsaved edits of every file are saved or let go, and the person may
    /// leave every page.
    pub(crate) fn ask_all_then(
        &mut self,
        then: impl FnOnce(&mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (unsaved, pages) = (self.unsaved(cx), self.pages_of(|_| true));
        let then = move |_: &mut Self, cx: &mut Context<Self>| cx.defer(then);
        self.ask_about_files_and_pages_then(unsaved, pages, then, window, cx);
    }
}
