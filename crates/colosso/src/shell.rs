//! The window: translucent rails around a rounded content card, in Orca or Arc layout.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Div, ElementId, FocusHandle, InteractiveElement as _, IntoElement, KeyBinding,
    MouseButton, MouseDownEvent, ParentElement as _, Render, ScrollDelta, ScrollEvent, Stateful,
    Styled as _, TouchPhase, Window, actions, div, px,
};

use crate::model::{SidebarMode, Workspace};
use crate::{arc, orca, pane, theme};

actions!(
    colosso,
    [
        ToggleSidebar,
        ToggleSidebarMode,
        ToggleRightPanel,
        ToggleBottomPanel,
        NewChat
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-alt-b", ToggleRightPanel, None),
        KeyBinding::new("cmd-j", ToggleBottomPanel, None),
        KeyBinding::new("cmd-t", NewChat, None),
        KeyBinding::new("cmd-shift-m", ToggleSidebarMode, None),
    ]);
}

/// Horizontal travel, in points, that switches the Arc sidebar to the next project.
const SWIPE_THRESHOLD: f32 = 70.;

pub struct Shell {
    pub workspace: Workspace,
    sidebar_open: bool,
    right_panel_open: bool,
    bottom_panel_open: bool,
    /// Horizontal travel of a two-finger swipe in progress over the Arc sidebar.
    pub swipe: Option<f32>,
    focus: FocusHandle,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Shell {
            workspace: Workspace::sample(),
            sidebar_open: true,
            right_panel_open: false,
            bottom_panel_open: false,
            swipe: None,
            focus,
        }
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }

    fn toggle_sidebar_mode(
        &mut self,
        _: &ToggleSidebarMode,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace.mode = match self.workspace.mode {
            SidebarMode::Orca => SidebarMode::Arc,
            SidebarMode::Arc => SidebarMode::Orca,
        };
        cx.notify();
    }

    fn toggle_right_panel(&mut self, _: &ToggleRightPanel, _: &mut Window, cx: &mut Context<Self>) {
        self.right_panel_open = !self.right_panel_open;
        cx.notify();
    }

    fn toggle_bottom_panel(
        &mut self,
        _: &ToggleBottomPanel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bottom_panel_open = !self.bottom_panel_open;
        cx.notify();
    }

    fn new_chat(&mut self, _: &NewChat, _: &mut Window, cx: &mut Context<Self>) {
        self.workspace.new_chat();
        cx.notify();
    }

    pub fn swipe(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let ScrollDelta::Pixels(delta) = event.delta else {
            return;
        };
        let (dx, dy) = (delta.x.as_f32(), delta.y.as_f32());
        self.swipe = match (event.touch_phase, self.swipe) {
            (TouchPhase::Started, _) => Some(0.),
            // A gesture that starts vertical is a scroll, not a swipe.
            (TouchPhase::Moved, Some(0.)) if dy.abs() > dx.abs() => None,
            (TouchPhase::Moved, Some(travel)) => {
                Some((travel + dx).clamp(-theme::SIDEBAR_WIDTH, theme::SIDEBAR_WIDTH))
            }
            (TouchPhase::Ended, Some(travel)) => {
                if travel <= -SWIPE_THRESHOLD {
                    self.workspace.step_project(1);
                } else if travel >= SWIPE_THRESHOLD {
                    self.workspace.step_project(-1);
                }
                None
            }
            // Momentum events arrive after the fingers lift and carry no swipe.
            _ => None,
        };
        cx.notify();
    }

    fn top_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(theme::TITLE_BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .pr(px(theme::GUTTER))
            .child(drag_area("traffic-lights").w(px(theme::TRAFFIC_LIGHTS_WIDTH)))
            .child(
                icon_button("toggle-sidebar", IconName::PanelLeft, "Toggle sidebar (⌘B)").on_click(
                    cx.listener(|this, _, window, cx| {
                        this.toggle_sidebar(&ToggleSidebar, window, cx)
                    }),
                ),
            )
            .child(icon_button("back", IconName::ArrowLeft, "Back"))
            .child(icon_button("forward", IconName::ArrowRight, "Forward"))
            .child(drag_area("title").flex_1())
            .child(
                icon_button(
                    "toggle-bottom",
                    IconName::PanelBottom,
                    "Toggle bottom panel (⌘J)",
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_bottom_panel(&ToggleBottomPanel, window, cx)
                })),
            )
            .child(
                icon_button(
                    "toggle-right",
                    IconName::PanelRight,
                    "Toggle right panel (⌥⌘B)",
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_right_panel(&ToggleRightPanel, window, cx)
                })),
            )
    }

    fn icon_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let other_mode = match self.workspace.mode {
            SidebarMode::Orca => "Switch to Arc layout (⇧⌘M)",
            SidebarMode::Arc => "Switch to Orca layout (⇧⌘M)",
        };
        div()
            .w(px(theme::ICON_RAIL_WIDTH))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .pb(px(theme::GUTTER))
            .child(icon_button("rail-chats", IconName::MessagesSquare, "Chats"))
            .child(icon_button(
                "rail-changes",
                IconName::GitPullRequest,
                "Changes",
            ))
            .child(div().flex_1())
            .child(
                icon_button("rail-mode", IconName::PanelsTopLeft, other_mode).on_click(
                    cx.listener(|this, _, window, cx| {
                        this.toggle_sidebar_mode(&ToggleSidebarMode, window, cx)
                    }),
                ),
            )
            .child(icon_button("rail-settings", IconName::Settings, "Settings"))
    }

    fn content(&self) -> impl IntoElement {
        let workspace = &self.workspace;
        let branch = workspace.branch();
        let context = format!("{} / {}", workspace.project().name, branch.name);
        div()
            .flex_1()
            .min_h_0()
            .child(match branch.tabs.get(branch.active_tab) {
                Some(tab) => pane::tab(tab, context.into()),
                None => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(theme::muted())
                    .child("No tabs on this branch. ⌘T starts a chat.")
                    .into_any_element(),
            })
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let orca = self.workspace.mode == SidebarMode::Orca;
        let main = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .when(orca, |main| main.child(orca::tab_strip(self, cx)))
            .child(self.content())
            .when(self.bottom_panel_open, |main| main.child(bottom_panel()));
        let card = div()
            .flex_1()
            .min_w_0()
            .mr(px(theme::GUTTER))
            .mb(px(theme::GUTTER))
            .rounded(px(theme::CARD_RADIUS))
            .border_1()
            .border_color(theme::hairline())
            .bg(theme::card())
            .overflow_hidden()
            .flex()
            .when(orca && self.sidebar_open, |card| {
                card.child(orca::sidebar(self, cx))
            })
            .child(main)
            .when(self.right_panel_open, |card| card.child(right_panel()));

        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_sidebar_mode))
            .on_action(cx.listener(Self::toggle_right_panel))
            .on_action(cx.listener(Self::toggle_bottom_panel))
            .on_action(cx.listener(Self::new_chat))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::rail_tint())
            .text_color(theme::text())
            .child(self.top_rail(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.icon_rail(cx))
                    .when(!orca && self.sidebar_open, |body| {
                        body.child(arc::sidebar(self, cx))
                    })
                    .child(card),
            )
    }
}

/// Empty rail space that moves the window, since the app owns titlebar dragging.
fn drag_area(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .h_full()
        .on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
            if event.click_count == 2 {
                window.titlebar_double_click();
            } else {
                window.start_window_move();
            }
        })
}

pub fn icon_button(id: &'static str, icon: IconName, tooltip: &'static str) -> Button {
    Button::new(id).ghost().small().icon(icon).tooltip(tooltip)
}

pub fn icon(name: IconName, size: f32) -> Icon {
    Icon::new(name).size(px(size))
}

/// A sidebar row: highlighted when selected, tinted on hover otherwise.
pub fn row(id: impl Into<ElementId>, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(theme::ROW_HEIGHT))
        .flex_shrink_0()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(6.))
        .text_sm()
        .cursor_pointer()
        .when(selected, |row| row.bg(theme::selected()))
        .when(!selected, |row| row.hover(|style| style.bg(theme::hover())))
}

pub fn ellipsized(text: impl IntoElement) -> Div {
    div()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text)
}

fn right_panel() -> impl IntoElement {
    let files = [
        (0., "src/"),
        (12., "ledger.rs"),
        (12., "payouts.rs"),
        (0., "Cargo.toml"),
        (0., "README.md"),
    ];
    div()
        .w(px(260.))
        .flex_shrink_0()
        .h_full()
        .p_2()
        .border_l_1()
        .border_color(theme::hairline())
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .px_2()
                .h(px(28.))
                .flex()
                .items_center()
                .text_xs()
                .text_color(theme::muted())
                .child("Files"),
        )
        .children(files.iter().map(|(indent, file)| {
            div()
                .px_2()
                .pl(px(8. + indent))
                .h(px(24.))
                .flex()
                .items_center()
                .text_sm()
                .child(*file)
        }))
}

fn bottom_panel() -> impl IntoElement {
    div()
        .h(px(200.))
        .flex_shrink_0()
        .border_t_1()
        .border_color(theme::hairline())
        .p_3()
        .font_family("Menlo")
        .text_sm()
        .text_color(theme::muted())
        .bg(theme::sunken())
        .child("Terminal")
}
