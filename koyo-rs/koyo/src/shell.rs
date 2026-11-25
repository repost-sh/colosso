//! The window: translucent rails around a rounded content card, in Orca or Arc layout, folded
//! away with the sidebar in compact mode (`rails`). The sidebar slot holds the layout's sidebar,
//! and the card the tab shown: a checkout's, a pinned or a loose page's, or the Settings tab's
//! pages (`settings_tab`).

use std::rc::Rc;

use gpui_kit::component::Theme;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels, Point, Render,
    SharedString, Styled, Subscription, Window, div, px,
};

use crate::bottom::StatusBar;
use crate::chat::ChatTabs;
use crate::files::Locations;
use crate::language::ServersListed;
use crate::model::{Front, PaneKind, SettingsTab, Tab, Workspace};
use crate::navigation::{OpenSettings, SettingsPage};
use crate::right_panel::RightPanel;
use crate::settings::{self, Layout};
use crate::settings_view::{self, PageList};
use crate::sidebar::{SidebarFrame, slide_out};
use crate::theme::Palette;
use crate::theme::card::{clear_island, part};
use crate::settings::Surface;
use crate::{arc, backdrop, bottom, chat_backdrop, gradient, keyboard, orca, picker, theme};

mod actions;
mod browser_tabs;
#[cfg(test)]
mod budget;
mod closing;
mod device_tabs;
mod edges;
mod elements;
mod gone;
mod kept;
mod keys;
mod leaving;
mod loose;
mod one_tab;
mod pictures;
mod pinned;
mod rails;
mod reopen;
mod reorder;
mod settings_tab;
mod shown_page;
mod sight;
mod splits;
mod state;
mod switcher;
mod tab_face;
mod tab_list;
mod tile_grid;
mod unmade;
mod visits;

pub use actions::{
    CloseTab, EditTheme, NewChat, ReopenClosedTab, SwitchTab, ToggleBottomPanel, ToggleRightPanel,
    ToggleSidebar, ToggleSidebarMode, init,
};
#[cfg(feature = "visual-check")]
pub use actions::{NewTerminal, StopTerminal};
#[cfg(all(target_os = "macos", feature = "visual-check"))]
pub use switcher::visual::run as check_switcher;
use elements::{SearchField, Searched};
use elements::drag_area;
pub use elements::{
    action_button, add_project_row, ellipsized, glass, icon, icon_button, new_tab_button,
    no_matches, project_error, row, row_button, section_label, solid, tab_close_button,
};
pub use kept::{KeptWindow, reopen};
pub use loose::{loose_close, loose_list, loose_row};
pub(crate) use one_tab::Place;
pub use settings_tab::{settings_close, settings_row};
pub use reorder::{Drop, InHand, List, MOTION, Reorder, TabHand, key_of};
pub use splits::{Dragged, DraggedPane, drag_chip};
pub use tab_face::Face;
pub use pinned::{CopyToWorktree, hides_pinned_zone, pinned_picture, pinned_zone_says};
#[cfg(test)]
pub use pinned::{ClosePinned, PinnedMenu, RemovePinned, RepinAtCurrent, Unpin};
pub use tab_list::{tab_list, tab_row, title_with_end};
pub use tile_grid::{Arrange, Landing, TileGrid, TileHand};

pub struct Shell {
    pub workspace: Workspace,
    /// What the Settings tab's list of pages keeps while the tab's closed.
    settings_pages: Entity<PageList>,
    /// Closed, the window is in compact mode: the rails fold away with the sidebar.
    sidebar_open: bool,
    /// The sidebar's width while its edge is dragged, before it's saved.
    sidebar_dragged: Option<f32>,
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
    /// What's still to happen to the keyboard, which only the person moves (`keys`).
    keys: keys::Handing,
    /// The sidebar's search field.
    pub(crate) search: Entity<SearchField>,
    /// What the search field holds, which narrows the sidebar's worktrees.
    query: SharedString,
    /// Where the rails or the Arc sidebar were last right-clicked, for the theme picker their
    /// menu opens.
    pub(crate) rails_menu_at: Point<Pixels>,
    /// The tile or space dot in the Arc sidebar under the pointer, and its card.
    pub(crate) arc_hover: arc::Hover,
    /// The Arc sidebar's controls and address bar for the page shown.
    pub(crate) arc_top: arc::Top,
    /// The top bar's downloads button, beside the sidebar's toggle.
    downloads: Entity<crate::browser::DownloadsButton>,
    /// The tab being dragged along its list, shared by every list of tabs.
    pub tab_hand: Rc<TabHand>,
    /// The row being dragged along its list in either layout's sidebar: a worktree among its
    /// project's, or an Orca project among the projects.
    pub row_hand: Rc<TabHand>,
    /// The project tile being dragged about the Arc sidebar's grid.
    pub tile_hand: Rc<TileHand>,
    /// The pinned page's tile being dragged about its own grid, at the Arc sidebar's head or in
    /// the Orca layout's top rail, and a page from elsewhere landing among them.
    pub pinned_hand: Rc<TileHand>,
    /// The places a symbol led to, listed under the tabs, while they are.
    pub locations: Option<Entity<Locations>>,
    /// What the walks that let go of what's gone went by when they last ran (`gone`).
    let_go_at: gone::Keys,
    _subscriptions: [Subscription; 14],
}

impl Shell {
    pub fn new(window: &mut Window, kept: KeptWindow, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let settings_pages = cx.new(|cx| PageList::new(window, cx));
        let search = cx.new(|cx| SearchField::new(window, cx));
        let chat_tabs = ChatTabs::new(cx);
        Self::keep_on_the_way_out(cx);
        let status_bar = StatusBar::new(cx);
        let downloads = cx.new(crate::browser::DownloadsButton::new);
        let _subscriptions = [
            settings::observe(cx),
            cx.observe_window_bounds(window, |_, window, cx| {
                crate::browser::fullscreen::window_resized(window, cx);
            }),
            cx.observe(&settings_pages, |_, _, cx| cx.notify()),
            cx.observe_window_visibility(window, |this, visibility, window, cx| {
                this.tell_sight(window, cx);
                if visibility.is_visible() {
                    window.refresh();
                    chat_backdrop::look_again(cx);
                }
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                keyboard::follow_focus(window, cx);
                keyboard::key_state(window);
                if window.is_window_active() {
                    this.reload_projects(cx);
                    crate::accounts::window_activated(cx);
                }
            }),
            cx.subscribe(&search, |this, _, Searched(query), cx| {
                this.searched(query.clone(), cx);
            }),
            cx.observe_in(&cx.entity(), window, |this, _, window, cx| {
                this.workspace.stamp_shown();
                this.follow_space(cx);
                this.let_go_of_what_is_gone(cx);
                this.follow_selection(window, cx);
                this.make_shown(window, cx);
                this.keep_window(cx);
                this.tell_arc_top(cx);
                let pane = (this.workspace.tab_shown()).map(|tab| tab.first().id);
                let settings = this.workspace.settings().map(SettingsTab::page);
                let showing = (settings, Self::layout(cx), this.sidebar_open, pane);
                crate::popup::showing(window.window_handle(), &showing, cx);
            }),
            cx.on_focus_lost(window, |this, window, cx| window.focus(&this.focus, cx)),
            cx.on_focus_in(&focus, window, |_, window, cx| keyboard::follow_focus(window, cx)),
            settings::observe_in(window, cx, Self::settings_changed),
            cx.observe_global::<Theme>(|this, cx| this.terminals.tell_theme(cx)),
            crate::browser::profiles::on_change(cx, |this, cx| {
                if this.shows(SettingsPage::Browser) {
                    cx.notify();
                }
            }),
            cx.subscribe(status_bar.servers(), |this, _, ServersListed(any), cx| {
                this.rails.servers = *any;
                cx.notify();
            }),
            cx.subscribe(&downloads, |_, _, _: &crate::browser::CameOrWent, cx| cx.notify()),
        ];
        Self::guard_closing(window, cx);
        let mut workspace = Workspace::new(&settings::get(cx).projects);
        workspace.name_projects(&settings::get(cx).project_looks);
        let mut shell = Shell {
            workspace,
            settings_pages,
            sidebar_open: settings::get(cx).sidebar_open_on_launch,
            sidebar_dragged: None,
            rails: rails::Rails::default(),
            right_panel: RightPanel::default(),
            bottom_panel_open: false,
            status_bar,
            terminals: crate::terminals::Terminals::default(),
            chat_tabs,
            keys: keys::Handing::default(),
            kept,
            swipe: arc::Swipe::default(),
            focus,
            search,
            query: SharedString::default(),
            rails_menu_at: Point::default(),
            arc_hover: arc::Hover::default(),
            arc_top: arc::Top::new(cx),
            downloads,
            tab_hand: Rc::default(),
            row_hand: Rc::default(),
            tile_hand: Rc::default(),
            pinned_hand: Rc::default(),
            locations: None,
            let_go_at: gone::Keys::default(),
            _subscriptions,
        };
        shell.restore_pages(cx);
        shell.show_space(settings::get(cx).space, cx);
        shell.reload_projects(cx);
        shell.terminals.tell_theme(cx);
        shell.tell_arc_top(cx);
        shell
    }

    fn layout(cx: &App) -> Layout {
        settings::get(cx).layout
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
            PaneKind::Device => {
                self.device_pane(None, true, cx);
            }
            PaneKind::Chat | PaneKind::Browser | PaneKind::Editor => self.workspace.new_tab(kind),
        }
        self.keys_to_new_tab(kind, cx);
        cx.notify();
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

    /// What the sidebar slot holds in `layout`.
    fn sidebar(&self, layout: Layout, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        #[cfg(target_os = "macos")]
        let blur = settings::get(cx).sidebar_blur_scale().map(crate::native::sidebar_blur::behind);
        #[cfg(not(target_os = "macos"))]
        let blur = None;
        let frame = SidebarFrame::new(layout, self.sidebar_width(cx)).blur(blur);
        match layout {
            Layout::Orca => frame
                .children(chat_backdrop::backdrop(Surface::Sidebar, "sidebar-backdrop", cx))
                .child(orca::sidebar(self, cx)),
            Layout::Arc => frame.child(arc::sidebar(self, window, cx)),
        }
        .into_any_element()
    }

    /// What the card shows beside the sidebar: the Settings tab while it's in front, else the
    /// tab shown.
    fn main(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let islands = theme::card::islands(cx);
        let column = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .when(islands, |column| column.gap(px(theme::GUTTER)));
        if let Some(Front::Settings(tab)) = self.workspace.front() {
            let settings = settings_view::tab(&self.settings_pages, tab.page(), cx);
            return column
                .child(part(p, islands).flex_1().child(settings))
                .into_any_element();
        }
        let content = self.workspace_content(p, window, cx);
        column
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

    fn workspace_content(
        &mut self,
        p: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        self.make_shown(window, cx);
        self.tab_takes_keys(window, cx);
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
        let content = match (workspace.tab_shown(), workspace.project(), workspace.branch()) {
            (Some(tab), ..) => self.tab_content(tab, cx),
            (None, None, _) if workspace.projects.is_empty() => {
                note("Add a project to see its worktrees here.")
            }
            (None, None, _) if Self::layout(cx) == Layout::Arc => {
                note("No tab open. ⌘T opens a page, and the + below adds a project.")
            }
            (None, None, _) => note("No project selected."),
            (None, Some(_), None) => note("No checkout selected."),
            (None, Some(_), Some(_)) => {
                note("No tabs on this branch. ⌘T opens a tab, ⌘P opens a file.")
            }
        };
        pictures::watched(div().flex_1().min_h_0().child(content), self.shown_tab_key())
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        crate::budget::shell_rendered();
        let (settings, layout) = (self.workspace.settings().is_some(), Self::layout(cx));
        let root = crate::key_repeat::track(div(), window, cx)
            .key_context(Self::root_keys(settings, layout))
            .track_focus(&self.focus)
            .map(|root| Self::handle_actions(root, cx))
            .size_full();
        self.keys_follow_settings(settings, window, cx);
        self.tell_sight(window, cx);
        if let Some(page) = crate::browser::fullscreen::settle(window, cx) {
            return root.bg(gpui_kit::black()).child(page).into_any_element();
        }
        self.step_swipe(window, cx);
        let p = theme::palette(cx);
        let width = self.sidebar_width(cx);
        let shown = self.move_rails(window, cx);
        let sidebar = (shown.open.0 > 0.).then(|| {
            let sidebar = self.sidebar(layout, window, cx);
            slide_out(sidebar, width, shown.open.0).into_any_element()
        });
        let (card_sidebar, glass_sidebar) = match layout {
            Layout::Orca => (sidebar, None),
            Layout::Arc => (None, sidebar),
        };
        let islands = theme::card::islands(cx);
        let beside = rails::left_of_chat(self.sidebar_open, width);
        let next_to_page = self.workspace.tab_shown().is_some_and(Tab::shows_page);
        let right_panel = (self.right_panel).render(!settings, beside, next_to_page, window, cx);
        let content = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .when(!islands, |content| content.bg(p.card))
            .child(self.main(window, cx))
            .children(right_panel);
        let card = if islands { div() } else { clear_island() }
            .debug_selector(|| "shell-card".to_owned())
            .flex_1()
            .min_w_0()
            .min_h_0()
            .mr(px(theme::GUTTER))
            .relative()
            .flex()
            .children(card_sidebar)
            .child(content)
            .on_mouse_down(MouseButton::Right, |_, _, cx| {
                picker::close(cx);
                cx.stop_propagation();
            });

        root.font_family(theme::fonts::interface(cx))
            .relative()
            .flex()
            .flex_col()
            .when(gradient::current(cx).dots.is_empty(), |root| root.bg(p.rail_tint))
            .text_color(p.text)
            .child(backdrop::backdrop(cx))
            .child(match layout {
                Layout::Orca => self.orca_frame(shown, card, window, cx),
                Layout::Arc => self.arc_frame(shown, (glass_sidebar, card), window, cx),
            })
            .child(self.status_rail(shown, cx))
            .child(pictures::taker())
            .into_any_element()
    }
}
