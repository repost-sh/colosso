//! The rails round the card: the top rail, with room for the traffic lights, the window's
//! buttons and the right panel's; the icon rail down the left; and the status bar along the
//! bottom. ⌘B folds them away with the sidebar into compact mode, as Arc's: every edge then as
//! thin as the gutter right of the card, and the traffic lights gone. They move on springs, all
//! at once, and turn round mid-way, keeping their speed, when ⌘B is pressed again. The status
//! bar is only tall while the sidebar is out and it has language servers to show.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::motion::{Spring, spring};
use gpui_kit::component::{Disableable as _, Selectable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{AnimationPhase, App, Div, ParentElement as _, Styled as _, Window, div, px};

use super::{Shell, ToggleSidebar, ToggleSidebarMode, action_button, drag_area, icon_button};
use crate::navigation::{GoBack, GoForward, Navigate, OpenSettings, Route};
use crate::{settings, theme};

/// How the rails move: without overshooting, as a rail can be neither thinner than the gutter
/// nor wider than itself, and felt over about 0.3 s.
const MOTION: Spring = Spring::new(Duration::from_millis(300));

/// How far out the top rail is once it's too thin to hold the traffic lights: where they're
/// gone.
const LIGHTS_GONE: f32 = (theme::TRAFFIC_LIGHTS_TOP + theme::TRAFFIC_LIGHT_SIZE - theme::GUTTER)
    / (theme::TITLE_BAR_HEIGHT - theme::GUTTER);

/// What the rails follow besides the sidebar.
#[derive(Default)]
pub(super) struct Rails {
    /// Whether the selected checkout has language servers to show, which the status bar is
    /// tall for.
    pub servers: bool,
    /// Whether the traffic lights were left faded, so they're shown again once the top rail is
    /// out.
    faded: bool,
}

/// Where the rails are this frame, each from as thin as the gutter (0) to its full size (1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Shown {
    /// The top and icon rails, and how far out the sidebar is.
    pub open: AnimationPhase,
    /// The status bar.
    pub status: AnimationPhase,
}

/// A window edge a rail runs along.
#[derive(Clone, Copy)]
enum Edge {
    Top,
    Left,
    Bottom,
}

/// How much of the traffic lights shows with the top rail `open` this far: they fade as it
/// folds, gone once it's too thin to hold them.
fn lights(open: AnimationPhase) -> f32 {
    open.interpolate_between_clamped(LIGHTS_GONE..=1., 0., 1.)
}

/// How thick a rail of `full` thickness is `shown` this far out, from the gutter's thickness.
fn thickness(full: f32, shown: AnimationPhase) -> f32 {
    shown.interpolate(theme::GUTTER, full)
}

/// How wide the icon rail and the sidebar, left of the chat, are together once they've settled
/// with the sidebar `open`.
pub(super) fn left_of_chat(open: bool, cx: &App) -> f32 {
    let shown = AnimationPhase::from(open);
    let sidebar = settings::get(cx).sidebar_width();
    thickness(theme::ICON_RAIL_WIDTH, shown) + shown.interpolate(0., sidebar)
}

/// A rail along `edge`, `shown` this far from the gutter's thickness to `full`, holding what
/// `content` makes laid out at `full`. The content keeps to the card's side of the rail, so as
/// the rail thins it slides out of the window, fading as it goes; once thin, the rail is bare.
fn rail(edge: Edge, full: f32, shown: AnimationPhase, content: impl FnOnce() -> Div) -> Div {
    let thick = thickness(full, shown);
    let rail = div().flex_shrink_0().overflow_hidden().flex();
    let rail = match edge {
        Edge::Top | Edge::Bottom => rail.flex_col().h(px(thick)),
        Edge::Left => rail.w(px(thick)),
    };
    if shown.0 <= 0. {
        return rail;
    }
    // How far past the window's edge the content is pushed.
    let past = px(thick - full);
    let content = content().flex_shrink_0().relative().opacity(shown.0);
    rail.child(match edge {
        Edge::Top => content.h(px(full)).top(past),
        Edge::Left => content.w(px(full)).h_full().left(past),
        Edge::Bottom => content.h(px(full)),
    })
}

impl Shell {
    /// Where the rails are this frame: out with the sidebar, the status bar only with servers
    /// to show too. The traffic lights fade with the top rail.
    pub(super) fn move_rails(&mut self, window: &mut Window, cx: &mut App) -> Shown {
        let open = self.sidebar_open;
        let shown = Shown {
            open: spring("rails-open", open, MOTION, window, cx),
            status: spring(
                "rails-status",
                open && self.rails.servers,
                MOTION,
                window,
                cx,
            ),
        };
        // Out and at rest, the lights are as AppKit made them. Otherwise they're set each
        // frame, as AppKit makes them anew when the window leaves full screen.
        let lights = lights(shown.open);
        if lights < 1. || self.rails.faded {
            #[cfg(target_os = "macos")]
            crate::native::traffic_lights::fade(window, lights);
            self.rails.faded = lights < 1.;
        }
        shown
    }

    pub(super) fn top_rail(&self, shown: Shown, cx: &App) -> Div {
        let history = self.history.read(cx);
        rail(Edge::Top, theme::TITLE_BAR_HEIGHT, shown.open, || {
            div()
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
        })
        // Compact, the thin rail moves the window, as the full one's empty space does.
        .when(shown.open.0 <= 0., |rail| {
            rail.child(drag_area("compact-title").w_full())
        })
    }

    pub(super) fn icon_rail(&self, route: Route, shown: Shown, cx: &App) -> Div {
        rail(Edge::Left, theme::ICON_RAIL_WIDTH, shown.open, || {
            let in_settings = matches!(route, Route::Settings(_));
            let other_layout = format!("Switch to {} layout", Self::layout(cx).other().label());
            div()
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
        })
    }

    pub(super) fn status_rail(&self, shown: Shown, cx: &App) -> Div {
        let p = theme::palette(cx);
        rail(Edge::Bottom, theme::STATUS_BAR_HEIGHT, shown.status, || {
            self.status_bar.render(p)
        })
    }
}

#[cfg(test)]
mod tests;
