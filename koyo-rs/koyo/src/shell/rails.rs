//! The rails round the card: the top rail, with room for the traffic lights, the window's
//! buttons and the right panel's; and the status bar along the bottom. Down the left there's
//! only the gutter, as on the right: settings are in the app's menu, ⌘, opens them. ⌘B folds
//! them away with the sidebar into compact mode, as Arc's: every edge then as thin as the gutter
//! right of the card, and the traffic lights gone. They move on springs, all at once, and turn
//! round mid-way, keeping their speed, when ⌘B is pressed again; with Reduce Motion on in
//! System Settings they fold and come back at once (gpui-base follows it). The status bar is only tall while the
//! sidebar is out and it has language servers to show.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::motion::{Spring, spring};
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnimationPhase, App, Context, Div, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, Window, div, px,
};

use super::{Shell, ToggleSidebar, action_button, drag_area};
use crate::navigation::{GoBack, GoForward};
use crate::settings::{self, Layout};
use crate::theme;

/// How the rails move: without overshooting, as a rail can be neither thinner than the gutter
/// nor wider than itself, and quickly, felt over about 0.12 s.
const MOTION: Spring = Spring::new(Duration::from_millis(120));

/// How far out the top rail is once it's too thin to hold the traffic lights: where they're
/// gone.
const LIGHTS_GONE: f32 = (theme::TRAFFIC_LIGHTS_TOP + theme::TRAFFIC_LIGHT_SIZE - theme::GUTTER)
    / (theme::TITLE_BAR_HEIGHT - theme::GUTTER);

/// How strong the traffic lights are while the pointer isn't on them, as Arc dims its own, and how
/// long they take to come up or settle back.
const LIGHTS_RESTING: f32 = 0.4;
const LIGHTS_EASE: Duration = Duration::from_millis(160);

/// The room the traffic lights sit in: empty rail that moves the window, which notes whether the
/// pointer's on it.
fn lights_area(hover: impl Fn(&bool, &mut Window, &mut App) + 'static) -> gpui_kit::Stateful<Div> {
    drag_area("traffic-lights")
        .w(px(theme::TRAFFIC_LIGHTS_WIDTH))
        .on_hover(hover)
}

/// What the rails follow besides the sidebar.
#[derive(Default)]
pub(super) struct Rails {
    /// Whether the selected checkout has language servers to show, which the status bar is
    /// tall for.
    pub servers: bool,
}

/// Where the rails are this frame, each from as thin as the gutter (0) to its full size (1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Shown {
    /// The top rail, and how far out the sidebar is.
    pub open: AnimationPhase,
    /// The status bar.
    pub status: AnimationPhase,
}

/// A window edge a rail runs along.
#[derive(Clone, Copy)]
enum Edge {
    Top,
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

/// How wide the left gutter and the sidebar, left of the chat, are together once they've
/// settled with the sidebar `open`.
pub(super) fn left_of_chat(open: bool, cx: &App) -> f32 {
    let shown = AnimationPhase::from(open);
    let sidebar = settings::get(cx).sidebar_width();
    theme::GUTTER + shown.interpolate(0., sidebar)
}

/// A rail along `edge`, `shown` this far from the gutter's thickness to `full`, holding what
/// `content` makes laid out at `full`. The content keeps to the card's side of the rail, so as
/// the rail thins it slides out of the window, fading as it goes; once thin, the rail is bare.
fn rail(edge: Edge, full: f32, shown: AnimationPhase, content: impl FnOnce() -> Div) -> Div {
    let thick = thickness(full, shown);
    let rail = div()
        .flex_shrink_0()
        .overflow_hidden()
        .flex()
        .flex_col()
        .h(px(thick));
    if shown.0 <= 0. {
        return rail;
    }
    // How far past the window's edge the content is pushed.
    let past = px(thick - full);
    let content = content().flex_shrink_0().relative().opacity(shown.0);
    rail.child(match edge {
        Edge::Top => content.h(px(full)).top(past),
        Edge::Bottom => content.h(px(full)),
    })
}

impl Shell {
    /// Where the rails are this frame: out with the sidebar, the status bar only with servers
    /// to show too.
    pub(super) fn move_rails(&self, window: &mut Window, cx: &mut App) -> Shown {
        let open = self.sidebar_open;
        Shown {
            open: spring("rails-open", open, MOTION, window, cx),
            status: spring(
                "rails-status",
                open && self.rails.servers,
                MOTION,
                window,
                cx,
            ),
        }
    }

    /// The top rail. The traffic lights fade with it and, as Arc's, rest dimmed into the glass,
    /// coming up in full while the pointer's on them, easing either way. They're set each frame,
    /// as `AppKit` makes them anew when the window leaves full screen; `fade` touches only what
    /// differs.
    pub(super) fn top_rail(
        &self,
        shown: Shown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let (pointed, hover) = theme::hover::eased("rails-lights", LIGHTS_EASE, window, cx);
        let strength = LIGHTS_RESTING + (1. - LIGHTS_RESTING) * pointed;
        #[cfg(target_os = "macos")]
        crate::native::traffic_lights::fade(window, lights(shown.open) * strength);
        let history = self.history.read(cx);
        let toggle = || {
            action_button(
                "toggle-sidebar",
                IconName::PanelLeft,
                "Toggle sidebar",
                ToggleSidebar,
                cx,
            )
        };
        rail(Edge::Top, theme::TITLE_BAR_HEIGHT, shown.open, || {
            let row = div().flex().items_center().gap_1().pr(px(theme::GUTTER));
            let row = if Self::layout(cx) == Layout::Arc {
                // Over the sidebar, as Arc's: the lights and the toggle, and the page's back,
                // forward and reload at the sidebar's edge (`docs/arc-sidebar.md` 1).
                let width = theme::GUTTER + settings::get(cx).sidebar_width();
                row.child(
                    div()
                        .w(px(width))
                        .h_full()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pr(px(theme::GUTTER))
                        .child(lights_area(hover))
                        .child(toggle())
                        .child(drag_area("arc-lights-title").flex_1())
                        .child(crate::arc::controls(self, cx)),
                )
            } else {
                // Orca's: the toggle, the Claude accounts' button (the Arc sidebar's foot has it
                // there), and the page's back and forward.
                row.child(lights_area(hover))
                    .child(toggle())
                    .children(crate::accounts::button(cx))
                    .child(
                        action_button("back", IconName::ArrowLeft, "Back", GoBack, cx)
                            .disabled(!history.can_go_back()),
                    )
                    .child(
                        action_button("forward", IconName::ArrowRight, "Forward", GoForward, cx)
                            .disabled(!history.can_go_forward()),
                    )
            };
            row.child(drag_area("title").flex_1())
                .child(self.changes_button(cx))
                .child(self.files_button(cx))
        })
        // Compact, the thin rail moves the window, as the full one's empty space does.
        .when(shown.open.0 <= 0., |rail| {
            rail.child(drag_area("compact-title").w_full())
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
