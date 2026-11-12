//! The window: translucent rails around a rounded content card, in Orca or Arc layout, folded
//! away with the sidebar in compact mode (`rails`). The sidebar slot and the card show
//! whichever route the navigation history is on: the workspace, or a settings page.

use std::rc::Rc;

use gpui_kit::component::Theme;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, DragMoveEvent, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels,
    Point, Render, SharedString, Styled, Subscription, Window, div, px,
};

use crate::bottom::StatusBar;
use crate::chat::ChatTabs;
use crate::files::Locations;
use crate::language::ServersListed;
use crate::model::{PaneKind, Workspace};
use crate::navigation::{NavigationHistory, OpenSettings, Route};
use crate::right_panel::RightPanel;
use crate::settings::{self, Layout};
use crate::settings_view::SettingsPage::{Accounts, AgentDesktop};
use crate::settings_view::{self, SettingsSidebar};
use crate::sidebar::{SidebarFrame, SidebarResize, slide_out};
use crate::theme::Palette;
use crate::theme::card::{island, part};
use crate::{arc, backdrop, bottom, gradient, orca, picker, theme};

mod actions;
mod browser_tabs;
mod elements;
mod kept;
mod rails;
mod reopen;
mod reorder;
mod shown_page;
mod splits;
mod state;
mod switcher;
mod tab_face;
mod tab_list;
mod visits;

pub use actions::{
    CloseTab, EditTheme, NewChat, ReopenClosedTab, SwitchTab, SwitchTabBack, ToggleBottomPanel,
    ToggleRightPanel, ToggleSidebar, ToggleSidebarMode, init,
};
#[cfg(feature = "visual-check")]
pub use actions::{NewTerminal, StopTerminal};
use elements::drag_area;
pub use elements::{
    action_button, add_project_row, ellipsized, glass, icon, icon_button, new_tab_button,
    no_matches, project_error, row, search_field, section_label, solid, tab_close_button,
};
pub use kept::KeptWindow;
pub use reorder::TabHand;
pub use splits::{Dragged, DraggedPane};
pub use tab_list::{tab_list, tab_row};

pub struct Shell {
    pub workspace: Workspace,
    history: Entity<NavigationHistory>,
    settings_sidebar: Entity<SettingsSidebar>,
    /// Closed, the window is in compact mode: the rails fold away with the sidebar.
    sidebar_open: bool,
    rails: rails::Rails,
    /// The selected checkout's changes or files, beside the chat.
    pub right_panel: RightPanel,
    bottom_panel_open: bool,
    pub status_bar: StatusBar,
    /// The terminals the tabs show.
    pub terminals: crate::terminals::Terminals,
    /// The chats the tabs show.
    pub chat_tabs: ChatTabs,
    /// The window as it is, kept for the next one.
    pub kept: KeptWindow,
    /// Where the Arc sidebar's space is as two fingers move it between spaces, or the spring
    /// settles it.
    pub swipe: arc::Swipe,
    pub focus: FocusHandle,
    /// What the sidebar's search field narrows its worktrees to.
    pub search: Entity<InputState>,
    /// Where the rails or the Arc sidebar were last right-clicked, for the theme picker their
    /// menu opens.
    pub(crate) rails_menu_at: Point<Pixels>,
    /// The project selected when the Arc sidebar last followed it into its space.
    /// The tile or space dot in the Arc sidebar under the pointer, and its card.
    pub(crate) arc_hover: arc::Hover,
    /// The tab being dragged along its list, shared by every list of tabs.
    pub tab_hand: Rc<TabHand>,
    /// The project tile being dragged about the Arc sidebar's grid.
    pub tile_hand: Rc<arc::TileHand>,
    /// The places a symbol led to, listed under the tabs, while they are.
    pub locations: Option<Entity<Locations>>,
    _subscriptions: [Subscription; 12],
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
        let kept = KeptWindow::new(cx);
        let status_bar = StatusBar::new(cx);
        let _subscriptions = [
            settings::observe(cx),
            // The person taking the window out of full screen takes a page out with it.
            cx.observe_window_bounds(window, |_, window, cx| {
                crate::browser::fullscreen::window_resized(window, cx);
            }),
            cx.observe(&history, |_, _, cx| cx.notify()),
            // Worktrees may have come or gone in a terminal while the window was away, and the
            // Claude login's usage may be due a reading.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.reload_projects(cx);
                    crate::accounts::window_activated(cx);
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
                this.follow_space(cx);
                this.follow_selection(window, cx);
                this.make_shown_editors(window, cx);
                // Closed terminal tabs let their views go, which detaches them.
                this.terminals.retain(&this.workspace);
                this.chat_tabs.retain(&this.workspace, cx);
                this.keep_window(cx);
                // What floats over the window goes as it comes to show something else.
                let tab = (this.workspace.shown_tab()).and_then(|at| this.workspace.tab(at));
                let pane = tab.and_then(|tab| Some(tab.panes().first()?.id));
                let showing = (this.route(cx), Self::layout(cx), this.sidebar_open, pane);
                crate::popup::showing(window.window_handle(), &showing, cx);
            }),
            // What had the keyboard is gone, as a closed panel's list is: actions then start
            // at the window's root, above the shell's, and the top bar's buttons and keys
            // would do nothing until something else took the keyboard.
            cx.on_focus_lost(window, |this, window, cx| window.focus(&this.focus, cx)),
            // Every terminal's program follows the app's theme, its tab shown or not; each
            // project goes by the name the person gave it in Koyo, or its folder's.
            settings::on_change(cx, |this, cx| {
                this.terminals.tell_theme(cx);
                this.workspace
                    .name_projects(&settings::get(cx).project_looks);
            }),
            cx.observe_global::<Theme>(|this, cx| this.terminals.tell_theme(cx)),
            // Settings → Agent desktop shows how the host stands as it changes, and Settings →
            // Accounts the accounts. Only while the page shows: the desk changes with all an
            // agent does on its desktop.
            settings_view::redraws_while_shown(AgentDesktop, &crate::agent_desktop::desk(cx), cx),
            settings_view::redraws_while_shown(Accounts, &crate::accounts::accounts(cx), cx),
            // The status bar is tall only with servers to show.
            cx.subscribe(status_bar.servers(), |this, _, ServersListed(any), cx| {
                this.rails.servers = *any;
                cx.notify();
            }),
        ];
        Self::guard_closing(window, cx);
        let mut workspace = Workspace::new(&settings::get(cx).projects);
        workspace.name_projects(&settings::get(cx).project_looks);
        let mut shell = Shell {
            workspace,
            history,
            settings_sidebar,
            sidebar_open: settings::get(cx).sidebar_open_on_launch,
            rails: rails::Rails::default(),
            right_panel: RightPanel::default(),
            bottom_panel_open: false,
            status_bar,
            terminals: crate::terminals::Terminals::default(),
            chat_tabs,
            kept,
            swipe: arc::Swipe::default(),
            focus,
            search,
            rails_menu_at: Point::default(),
            arc_hover: arc::Hover::default(),
            tab_hand: Rc::default(),
            tile_hand: Rc::default(),
            locations: None,
            _subscriptions,
        };
        // On the space shown, its project selected, as a switch to it leaves them.
        shell.show_space(settings::get(cx).space, cx);
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

    /// Goes back to `route`, as Back does, when something opened out of sight just left it
    /// for the workspace.
    pub(crate) fn come_back(&mut self, route: Route, cx: &mut Context<Self>) {
        if self.route(cx) != route {
            self.update_history(NavigationHistory::go_back, cx);
        }
    }

    fn open_settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_page(None, cx);
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.set_sidebar_open(!self.sidebar_open, cx);
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
        match kind {
            PaneKind::Terminal => self.new_terminal(cx),
            PaneKind::Desktop => {
                self.desktop_pane(true, cx);
            }
            PaneKind::Chat | PaneKind::Browser | PaneKind::Editor => self.workspace.new_tab(kind),
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
        // Both layouts start the sidebar right after the left gutter.
        let width = (event.event.position.x.as_f32() - theme::GUTTER)
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

    /// What the sidebar slot holds on the current route.
    fn sidebar(
        &self,
        route: Route,
        layout: Layout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let frame = SidebarFrame::new(layout, settings::get(cx).sidebar_width());
        match (route, layout) {
            (Route::Settings(_), _) => frame.child(self.settings_sidebar.clone()),
            (Route::Workspace, Layout::Orca) => frame.child(orca::sidebar(self, cx)),
            (Route::Workspace, Layout::Arc) => frame.child(arc::sidebar(self, window, cx)),
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
        // In Arc each part an island of its own, the glass between them; in Orca on the one card.
        let islands = theme::card::islands(cx);
        let column = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .when(islands, |column| column.gap(px(theme::GUTTER)));
        match route {
            Route::Settings(page) => column
                .child(
                    part(p, islands)
                        .flex_1()
                        .child(settings_view::page(page, cx)),
                )
                .into_any_element(),
            Route::Workspace => {
                let content = self.workspace_content(p, window, cx);
                column
                    .when(layout == Layout::Orca, |main| {
                        main.child(orca::tab_strip(self, cx))
                    })
                    .child(content)
                    .children(
                        (self.locations.clone())
                            .map(|locations| part(p, islands).flex_shrink_0().child(locations)),
                    )
                    .when(self.bottom_panel_open, |main| {
                        main.child(part(p, islands).flex_shrink_0().child(bottom::panel(p)))
                    })
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
        // The shown tab's terminal, chat and browser panes have their views before the panes
        // draw them.
        self.make_shown_terminals(window, cx);
        self.make_shown_chats(window, cx);
        self.make_shown_browsers(window, cx);
        self.give_shown_tab_the_keyboard(window, cx);
        let workspace = &self.workspace;
        let islands = theme::card::islands(cx);
        let note = |text: &'static str| {
            part(p, islands)
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
            (None, _) if workspace.projects.is_empty() => {
                note("Add a project to see its worktrees here.")
            }
            // No project shown: an empty space, which in Arc the + below fills.
            (None, _) if Self::layout(cx) == Layout::Arc => {
                note("This space has no projects. Add one with the + below.")
            }
            (None, _) => note("No project selected."),
            (Some(_), None) => note("No checkout selected."),
            (Some(_), Some(branch)) => match branch.tabs.get(branch.active_tab) {
                Some(tab) => self.tab_content(tab, cx),
                None => note("No tabs on this branch. ⌘T opens a tab, ⌘P opens a file."),
            },
        };
        div().flex_1().min_h_0().child(content)
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = crate::key_repeat::track(div(), window, cx)
            .track_focus(&self.focus)
            .map(|root| Self::handle_actions(root, cx))
            .size_full();
        // A page full screen on the whole screen is all the window shows (`browser/fullscreen.rs`).
        if let Some(page) = crate::browser::fullscreen::settle(window, cx) {
            return root.bg(gpui_kit::black()).child(page).into_any_element();
        }
        // A space settling after a swipe moves a frame on first, so all that's drawn agrees.
        self.step_swipe(window, cx);
        let p = theme::palette(cx);
        let route = self.route(cx);
        let layout = Self::layout(cx);
        let width = settings::get(cx).sidebar_width();
        let shown = self.move_rails(window, cx);
        let sidebar = (shown.open.0 > 0.)
            .then(|| slide_out(self.sidebar(route, layout, window, cx), width, shown.open.0));
        // Orca keeps the sidebar on its card, Arc on the glass beside its islands.
        let (card_sidebar, glass_sidebar) = match layout {
            Layout::Orca => (sidebar, None),
            Layout::Arc => (None, sidebar),
        };
        let islands = theme::card::islands(cx);
        let on_workspace = route == Route::Workspace;
        let beside = rails::left_of_chat(self.sidebar_open, cx);
        let right_panel = (self.right_panel).render(on_workspace, beside, window, cx);
        let main = self.main(route, layout, window, cx);
        // What the rails hold: in Arc the islands side by side, the glass between them; in Orca
        // one card holding them all.
        let card = if islands { div() } else { island(p) }
            .debug_selector(|| "shell-card".to_owned())
            .flex_1()
            .min_w_0()
            .mr(px(theme::GUTTER))
            .relative()
            .flex()
            .children(card_sidebar)
            .child(main)
            .children(right_panel)
            // The rails' menu belongs to the rails, not the card.
            .on_mouse_down(MouseButton::Right, |_, _, cx| {
                picker::close(cx);
                cx.stop_propagation();
            });

        root.when(matches!(route, Route::Settings(_)), |root| {
            root.key_context("Settings")
        })
        .font_family(theme::fonts::interface(cx))
        .relative()
        .flex()
        .flex_col()
        // A gradient theme paints the glass instead of the plain tint.
        .when(gradient::current(cx).dots.is_empty(), |root| {
            root.bg(p.rail_tint)
        })
        .text_color(p.text)
        .child(backdrop::backdrop(cx))
        .child(self.top_rail(shown, window, cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                // The window's left edge, as thin as its right.
                .child(div().w(px(theme::GUTTER)).flex_shrink_0())
                .children(glass_sidebar)
                .child(card),
        )
        .child(self.status_rail(shown, cx))
        .into_any_element()
    }
}

impl Shell {
    /// What the sidebar's search field holds.
    pub fn search_query(&self, cx: &App) -> SharedString {
        self.search.read(cx).value()
    }
}
