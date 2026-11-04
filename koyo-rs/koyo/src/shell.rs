//! The window: translucent rails around a rounded content card, in Orca or Arc layout. The
//! sidebar slot and the card show whichever route the navigation history is on: the
//! workspace, or a settings page.

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::{Disableable as _, Selectable as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, DragMoveEvent, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels,
    Point, Render, SharedString, Styled, Subscription, Window, div, ease_in_out, px,
};

use crate::bottom::StatusBar;
use crate::chat::ChatTabs;
use crate::files::Locations;
use crate::model::{PaneKind, Workspace};
use crate::navigation::{GoBack, GoForward, Navigate, NavigationHistory, OpenSettings, Route};
use crate::right_panel::RightPanel;
use crate::settings::{self, Layout};
use crate::settings_view::{self, SettingsSidebar};
use crate::sidebar::{SidebarFrame, SidebarResize, slide_out};
use crate::theme::Palette;
use crate::{arc, backdrop, bottom, gradient, orca, picker, theme};

/// How long the sidebar takes to slide fully in or out.
const SIDEBAR_SLIDE: Duration = Duration::from_millis(180);

mod actions;
mod browser_tabs;
mod elements;
mod reopen;
mod reorder;
mod splits;
mod state;
mod switcher;
mod tab_face;
mod tab_list;

pub use actions::{
    CloseTab, EditTheme, NewBrowserTab, NewChat, NewEditorTab, NewTerminal, ReopenClosedTab,
    SwitchTab, SwitchTabBack, ToggleBottomPanel, ToggleRightPanel, ToggleSidebar,
    ToggleSidebarMode, init,
};
#[cfg(feature = "visual-check")]
pub use actions::StopTerminal;
use elements::drag_area;
pub use elements::{
    action_button, add_project_row, ellipsized, glass, icon, icon_button, new_tab_menu, no_matches,
    project_error, row, search_field, section_label, solid, tab_close_button,
};
pub use reorder::TabHand;
pub use splits::divider;
pub use tab_list::{tab_list, tab_row};

pub struct Shell {
    pub workspace: Workspace,
    history: Entity<NavigationHistory>,
    settings_sidebar: Entity<SettingsSidebar>,
    sidebar_open: bool,
    /// Where the sidebar was and when it started sliding toward `sidebar_open`.
    sidebar_slide: Option<(f32, Instant)>,
    /// The selected checkout's changes or files, beside the chat.
    pub right_panel: RightPanel,
    bottom_panel_open: bool,
    pub status_bar: StatusBar,
    /// The terminals the tabs show.
    pub terminals: crate::terminals::Terminals,
    /// The chats the tabs show.
    pub chat_tabs: ChatTabs,
    /// Horizontal travel of a two-finger swipe in progress over the Arc sidebar.
    pub swipe: Option<f32>,
    pub focus: FocusHandle,
    /// What the sidebar's search field narrows its worktrees to.
    pub search: Entity<InputState>,
    /// Where the rails were last right-clicked, for the theme picker their menu opens.
    rails_menu_at: Point<Pixels>,
    /// The tab being dragged along its list, shared by every list of tabs.
    pub tab_hand: Rc<TabHand>,
    /// The places a symbol led to, listed under the tabs, while they are.
    pub locations: Option<Entity<Locations>>,
    _subscriptions: [Subscription; 8],
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let history = cx.new(|_| NavigationHistory::new());
        let settings_sidebar = cx.new(|cx| SettingsSidebar::new(history.clone(), window, cx));
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search")
                .clean_on_escape()
        });
        let chat_tabs = ChatTabs::new(cx);
        let _subscriptions = [
            settings::observe(cx),
            cx.observe(&history, |_, _, cx| cx.notify()),
            // Worktrees may have come or gone in a terminal while the window was away.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.reload_projects(cx);
                }
            }),
            cx.subscribe(&search, |_, _, event, cx| {
                if let InputEvent::Change = event {
                    cx.notify();
                }
            }),
            // The tab shown is noted as the latest, for ⌃⇥, and the right panel follows what's
            // selected: after each change rather than mid-frame.
            cx.observe_in(&cx.entity(), window, |this, _, window, cx| {
                this.workspace.stamp_shown();
                this.follow_selection(window, cx);
                // Closed terminal tabs let their views go, which detaches them.
                this.terminals.retain(&this.workspace);
                this.chat_tabs.retain(&this.workspace, cx);
            }),
            // What had the keyboard is gone, as a closed panel's list is: actions then start
            // at the window's root, above the shell's, and the top bar's buttons and keys
            // would do nothing until something else took the keyboard.
            cx.on_focus_lost(window, |this, window, cx| window.focus(&this.focus, cx)),
            // Every terminal's program follows the app's theme, its tab shown or not.
            settings::on_change(cx, |this, cx| this.terminals.tell_theme(cx)),
            cx.observe_global::<Theme>(|this, cx| this.terminals.tell_theme(cx)),
        ];
        Self::guard_closing(window, cx);
        let mut shell = Shell {
            workspace: Workspace::new(&settings::get(cx).projects),
            history,
            settings_sidebar,
            sidebar_open: settings::get(cx).sidebar_open_on_launch,
            sidebar_slide: None,
            right_panel: RightPanel::default(),
            bottom_panel_open: false,
            status_bar: StatusBar::new(cx),
            terminals: crate::terminals::Terminals::default(),
            chat_tabs,
            swipe: None,
            focus,
            search,
            rails_menu_at: Point::default(),
            tab_hand: Rc::default(),
            locations: None,
            _subscriptions,
        };
        shell.reload_projects(cx);
        shell.terminals.tell_theme(cx);
        shell
    }

    fn layout(cx: &App) -> Layout {
        settings::get(cx).layout
    }

    pub fn route(&self, cx: &App) -> Route {
        self.history.read(cx).current()
    }

    /// Moves through the history with `step`, which reports whether the route changed.
    fn update_history(
        &mut self,
        step: impl FnOnce(&mut NavigationHistory) -> bool,
        cx: &mut Context<Self>,
    ) {
        self.history.update(cx, |history, cx| {
            if step(history) {
                cx.notify();
            }
        });
    }

    pub fn navigate(&mut self, route: Route, cx: &mut Context<Self>) {
        self.update_history(|history| history.push(route), cx);
    }

    fn open_settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_page(None, cx);
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.set_sidebar_open(!self.sidebar_open, cx);
    }

    /// How far out the sidebar is, from 0 hidden to 1 fully out, and whether it is still sliding.
    fn sidebar_reveal(&self) -> (f32, bool) {
        let target = if self.sidebar_open { 1. } else { 0. };
        let Some((from, start)) = self.sidebar_slide else {
            return (target, false);
        };
        let progress = start.elapsed().as_secs_f32() / SIDEBAR_SLIDE.as_secs_f32();
        if progress >= 1. {
            return (target, false);
        }
        (from + (target - from) * ease_in_out(progress), true)
    }

    fn toggle_bottom_panel(
        &mut self,
        _: &ToggleBottomPanel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_bottom_panel_open(!self.bottom_panel_open, cx);
    }

    pub fn new_tab(&mut self, kind: PaneKind, cx: &mut Context<Self>) {
        if kind == PaneKind::Terminal {
            self.new_terminal(cx);
        } else {
            self.workspace.new_tab(kind);
        }
        if kind == PaneKind::Chat {
            self.focus_chat_next();
        }
        // A new tab is something to look at, so leave settings for it.
        self.navigate(Route::Workspace, cx);
        cx.notify();
    }

    fn resize_sidebar(
        &mut self,
        event: &DragMoveEvent<SidebarResize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Both layouts start the sidebar right after the icon rail.
        let width = (event.event.position.x.as_f32() - theme::ICON_RAIL_WIDTH)
            .clamp(theme::SIDEBAR_MIN_WIDTH, theme::SIDEBAR_MAX_WIDTH);
        settings::update(cx, |settings| settings.sidebar_width = width);
    }

    fn show_rails_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        picker::close(cx);
        self.rails_menu_at = event.position;
        let other_layout = format!("Switch to {} Layout", Self::layout(cx).other().label());
        NativeMenu::new()
            .menu("Edit Theme…", Box::new(EditTheme))
            .submenu("Appearance", theme::preference(cx).menu())
            .separator()
            .menu("Toggle Sidebar", Box::new(ToggleSidebar))
            .menu(other_layout, Box::new(ToggleSidebarMode))
            .separator()
            .menu("Settings…", Box::new(OpenSettings))
            .show(event.position, window, cx);
    }

    fn top_rail(&self, cx: &App) -> impl IntoElement {
        let history = self.history.read(cx);
        div()
            .h(px(theme::TITLE_BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .pr(px(theme::GUTTER))
            .child(drag_area("traffic-lights").w(px(theme::TRAFFIC_LIGHTS_WIDTH)))
            .child(action_button(
                "toggle-sidebar",
                IconName::PanelLeft,
                "Toggle sidebar",
                ToggleSidebar,
                cx,
            ))
            .child(
                action_button("back", IconName::ArrowLeft, "Back", GoBack, cx)
                    .disabled(!history.can_go_back()),
            )
            .child(
                action_button("forward", IconName::ArrowRight, "Forward", GoForward, cx)
                    .disabled(!history.can_go_forward()),
            )
            .child(drag_area("title").flex_1())
            .child(self.changes_button(cx))
            .child(self.files_button(cx))
    }

    fn icon_rail(&self, cx: &App) -> impl IntoElement {
        let in_settings = matches!(self.route(cx), Route::Settings(_));
        let other_layout = format!("Switch to {} layout", Self::layout(cx).other().label());
        div()
            .w(px(theme::ICON_RAIL_WIDTH))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .pb(px(theme::GUTTER))
            .child(
                icon_button("rail-chats", IconName::MessagesSquare, "Chats", cx)
                    .selected(!in_settings)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(Navigate(Route::Workspace)), cx);
                    }),
            )
            .child(icon_button(
                "rail-changes",
                IconName::GitPullRequest,
                "Changes",
                cx,
            ))
            .child(div().flex_1())
            .child(action_button(
                "rail-mode",
                IconName::PanelsTopLeft,
                other_layout,
                ToggleSidebarMode,
                cx,
            ))
            .child(theme::toggle(cx))
            .child(
                action_button(
                    "rail-settings",
                    IconName::Settings,
                    "Settings",
                    OpenSettings,
                    cx,
                )
                .selected(in_settings),
            )
    }

    /// What the sidebar slot holds on the current route.
    fn sidebar(&self, route: Route, layout: Layout, cx: &mut Context<Self>) -> AnyElement {
        let frame = SidebarFrame::new(layout, settings::get(cx).sidebar_width());
        match (route, layout) {
            (Route::Settings(_), _) => frame.child(self.settings_sidebar.clone()),
            (Route::Workspace, Layout::Orca) => frame.child(orca::sidebar(self, cx)),
            (Route::Workspace, Layout::Arc) => frame.child(arc::sidebar(self, cx)),
        }
        .into_any_element()
    }

    /// What the card shows beside the sidebar on the current route.
    fn main(
        &mut self,
        route: Route,
        layout: Layout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::palette(cx);
        let column = div().flex_1().min_w_0().flex().flex_col();
        match route {
            Route::Settings(page) => column
                .child(settings_view::page(page, cx))
                .into_any_element(),
            Route::Workspace => {
                let content = self.workspace_content(p, window, cx);
                column
                    .when(layout == Layout::Orca, |main| {
                        main.child(orca::tab_strip(self, cx))
                    })
                    .child(content)
                    .children(self.locations.clone())
                    .when(self.bottom_panel_open, |main| main.child(bottom::panel(p)))
                    .into_any_element()
            }
        }
    }

    fn workspace_content(
        &mut self,
        p: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        // The shown tab's terminal and chat panes have their views before the panes draw them.
        let terminal = self.make_shown_terminals(window, cx);
        let chat = self.make_shown_chats(window, cx);
        // Shown with nothing else holding the keyboard, one of them takes it. Only then, as
        // reading a view here makes this draw again whenever that view changes.
        if self.focus.is_focused(window) {
            let takes_keys = (terminal.map(|view| view.focus_handle(cx)))
                .or_else(|| chat.map(|view| view.focus_handle(cx)));
            if let Some(focus) = takes_keys {
                cx.defer_in(window, move |_, window, cx| window.focus(&focus, cx));
            }
        }
        let workspace = &self.workspace;
        let note = |text: &'static str| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(p.muted)
                .child(text)
                .into_any_element()
        };
        let content = match (workspace.project(), workspace.branch()) {
            (None, _) => note("Add a project to see its worktrees here."),
            (Some(_), None) => note("No checkout selected."),
            (Some(_), Some(branch)) => match branch.tabs.get(branch.active_tab) {
                Some(tab) => self.tab_content(tab, cx),
                None => note("No tabs on this branch. ⌘T starts a chat, ⌘P opens a file."),
            },
        };
        div().flex_1().min_h_0().child(content)
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let route = self.route(cx);
        let layout = Self::layout(cx);
        let width = settings::get(cx).sidebar_width();
        let (reveal, sliding) = self.sidebar_reveal();
        if sliding {
            window.request_animation_frame();
        } else {
            self.sidebar_slide = None;
        }
        let sidebar =
            (reveal > 0.).then(|| slide_out(self.sidebar(route, layout, cx), width, reveal));
        // Orca keeps the sidebar inside the card, Arc on the glass beside it.
        let (card_sidebar, glass_sidebar) = match layout {
            Layout::Orca => (sidebar, None),
            Layout::Arc => (None, sidebar),
        };
        let shown = route == Route::Workspace;
        let right_panel = (self.right_panel).render(shown, self.sidebar_open, window, cx);
        let main = self.main(route, layout, window, cx);
        let card = div()
            .flex_1()
            .min_w_0()
            .mr(px(theme::GUTTER))
            .rounded(px(theme::CARD_RADIUS))
            .bg(p.card)
            .overflow_hidden()
            .relative()
            .flex()
            .child(theme::card::noted())
            .children(card_sidebar)
            .child(main)
            .children(right_panel)
            // The rails' menu belongs to the rails, not the card.
            .on_mouse_down(MouseButton::Right, |_, _, cx| {
                picker::close(cx);
                cx.stop_propagation();
            });

        crate::key_repeat::track(div(), window, cx)
            .track_focus(&self.focus)
            .when(matches!(route, Route::Settings(_)), |root| {
                root.key_context("Settings")
            })
            .map(|root| Self::handle_actions(root, cx))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            // A gradient theme paints the glass instead of the plain tint.
            .when(gradient::current(cx).dots.is_empty(), |root| {
                root.bg(p.rail_tint)
            })
            .text_color(p.text)
            .child(backdrop::backdrop(cx))
            .child(self.top_rail(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.icon_rail(cx))
                    .children(glass_sidebar)
                    .child(card),
            )
            .child(self.status_bar.render(p))
    }
}

impl Shell {
    /// What the sidebar's search field holds.
    pub fn search_query(&self, cx: &App) -> SharedString {
        self.search.read(cx).value()
    }
}
