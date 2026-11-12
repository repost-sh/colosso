//! What the Arc sidebar shows of something hovered (`docs/arc-sidebar.md` 3, 7): a project's card
//! under its tile, its picture, name, folder and what's in it; or a space's name over its dot. It's
//! on glass, a blurred window of its own tinted as the launcher's is, so it reads as the launcher
//! and the menus do over whatever the window shows. It never takes the keyboard, fades in as it
//! comes, and goes when the pointer leaves what it's about, the window goes to the back, or
//! another opens.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, Bounds, Context, Div, FontWeight, Hsla,
    IntoElement, ParentElement as _, Pixels, Render, SharedString, Styled, TextStyle, Window, div,
    ease_out_quint, img, point, px, size,
};

use crate::popup::{self, Kind, Opening};
use crate::theme::{self, Palette};

/// How long a card takes to fade in.
const FADE: Duration = Duration::from_millis(160);
/// A project card's width, and the room round what it says.
const CARD_WIDTH: f32 = 272.;
const PADDING: f32 = 14.;
/// The picture at a card's head, which its name and folder sit beside.
const PICTURE: f32 = 32.;
/// Between the head and the rule under it, and the rule and the stats.
const SECTION_GAP: f32 = 12.;
/// A stat's count and what it counts, each line this high, and between rows of stats.
const COUNT_LINE: f32 = 20.;
const LABEL_LINE: f32 = 16.;
const STAT_GAP: f32 = 10.;
/// A project card's height: its lines, each as high as it's set, so nothing's cut off at its
/// foot.
const CARD_HEIGHT: f32 =
    2. * PADDING + PICTURE + 2. * SECTION_GAP + 1. + 2. * (COUNT_LINE + LABEL_LINE) + STAT_GAP;
/// A space's name over its dot: its height, the room either side of the name, and its text's
/// size.
const LABEL_HEIGHT: f32 = 28.;
const LABEL_PADDING: f32 = 12.;
const LABEL_TEXT: f32 = 12.;
/// Between what a card is about and the card.
const OFFSET: f32 = 6.;

/// What a card shows.
#[derive(Clone, PartialEq)]
pub enum Card {
    Project(ProjectCard),
    /// A space's name, in its colour's dot.
    Space {
        name: SharedString,
        colour: Hsla,
    },
}

/// A project as its card sums it up.
#[derive(Clone, PartialEq)]
pub struct ProjectCard {
    pub name: SharedString,
    /// Its folder, as the person would type it.
    pub path: SharedString,
    /// The picture its tile shows, or its initials while it has none.
    pub picture: Option<PathBuf>,
    pub initials: SharedString,
    pub worktrees: usize,
    pub tabs: usize,
    pub chats: usize,
    pub terminals: usize,
}

/// Shows `card` about what's at `anchor` in `parent`: a project's under it, a space's name over
/// it, both kept on the window. One that shows the same already stays.
pub fn show(card: Card, anchor: Bounds<Pixels>, parent: &mut Window, cx: &mut App) {
    if popup::root::<Surface>(Kind::Card, cx).is_some_and(|shown| shown.card == card) {
        return;
    }
    let (width, height) = match &card {
        Card::Project(_) => (CARD_WIDTH, CARD_HEIGHT),
        Card::Space { name, .. } => (label_width(name_width(name, parent)), LABEL_HEIGHT),
    };
    let within = parent.bounds();
    let at = match &card {
        Card::Project(_) => point(anchor.left(), anchor.bottom() + px(OFFSET)),
        Card::Space { .. } => point(
            anchor.center().x - px(width / 2.),
            anchor.top() - px(height + OFFSET),
        ),
    };
    let at = point(
        at.x.clamp(px(0.), (within.size.width - px(width)).max(px(0.))),
        at.y.clamp(px(0.), (within.size.height - px(height)).max(px(0.))),
    );
    let bounds = Bounds::new(within.origin + at, size(px(width), px(height)));
    let opening = Opening {
        kind: Kind::Card,
        bounds,
        kept: false,
        source: None,
    };
    popup::open(opening, parent, cx, |_, _| Surface { card });
}

/// Puts the card away, if one's up.
pub fn hide(cx: &mut App) {
    popup::close(Kind::Card, cx);
}

/// Whether a card's up, so the next one shows at once rather than after a wait.
pub fn is_shown(cx: &App) -> bool {
    popup::window(Kind::Card, cx).is_some()
}

/// How wide a space's name is drawn on its label, measured as the label is sized, before it's
/// drawn, in the window's own text.
fn name_width(name: &SharedString, window: &Window) -> Pixels {
    let style = TextStyle {
        font_weight: FontWeight::MEDIUM,
        ..TextStyle::default()
    };
    let run = style.to_run(name.len());
    (window.text_system())
        .shape_line(name.clone(), px(LABEL_TEXT), &[run], None)
        .width
}

/// How wide a label is for a name `name` wide: the name, its dot and the room round them,
/// within reason.
fn label_width(name: Pixels) -> f32 {
    (name.as_f32() + 2. * LABEL_PADDING + 14.).clamp(64., 240.)
}

/// The glass a card sits on, and what it shows.
struct Surface {
    card: Card,
}

impl Render for Surface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let content = match &self.card {
            Card::Project(project) => project_card(project, p).into_any_element(),
            Card::Space { name, colour } => space_label(name, *colour, p).into_any_element(),
        };
        div()
            .size_full()
            .bg(p.menu)
            .text_color(p.text)
            .child(content)
            .with_animation(
                "arc-card-in",
                Animation::new(FADE).with_easing(ease_out_quint()),
                Styled::opacity,
            )
    }
}

/// A count with what it counts under it, as a card's stats show them.
fn stat(count: usize, one: &str, many: &str, p: Palette) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(COUNT_LINE))
                .line_height(px(COUNT_LINE))
                .text_size(px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(count.to_string()),
        )
        .child(
            div()
                .h(px(LABEL_LINE))
                .line_height(px(LABEL_LINE))
                .text_xs()
                .text_color(p.muted)
                .truncate()
                .child(if count == 1 { one } else { many }.to_owned()),
        )
}

fn project_card(project: &ProjectCard, p: Palette) -> Div {
    let picture: AnyElement = match &project.picture {
        Some(picture) => img(picture.clone())
            .size(px(PICTURE))
            .rounded(px(8.))
            .into_any_element(),
        None => div()
            .size(px(PICTURE))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .bg(p.hover)
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(p.muted)
            .child(project.initials.clone())
            .into_any_element(),
    };
    div()
        .size_full()
        .overflow_hidden()
        .p(px(PADDING))
        .flex()
        .flex_col()
        .gap(px(SECTION_GAP))
        .child(
            div()
                .h(px(PICTURE))
                .flex()
                .items_center()
                .gap(px(10.))
                .child(div().flex_shrink_0().child(picture))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(project.name.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                // Cut from the start, so the folder's own name stays in view.
                                .text_ellipsis_start()
                                .child(project.path.clone()),
                        ),
                ),
        )
        .child(div().h(px(1.)).bg(p.hairline))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(STAT_GAP))
                .child(
                    div()
                        .flex()
                        .gap(px(12.))
                        .child(stat(project.worktrees, "worktree", "worktrees", p))
                        .child(stat(project.tabs, "tab open", "tabs open", p)),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(12.))
                        .child(stat(project.chats, "chat working", "chats working", p))
                        .child(stat(project.terminals, "terminal", "terminals", p)),
                ),
        )
}

fn space_label(name: &SharedString, colour: Hsla, p: Palette) -> Div {
    div()
        .size_full()
        .px(px(LABEL_PADDING))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .text_size(px(LABEL_TEXT))
        .font_weight(FontWeight::MEDIUM)
        .child(crate::pane::dot(colour))
        .child(div().truncate().text_color(p.text).child(name.clone()))
}

#[cfg(test)]
mod tests {
    use gpui_kit::px;

    use super::label_width;

    #[test]
    fn a_label_fits_its_name_within_reason() {
        assert!(label_width(px(10.)) >= 64.);
        assert!(label_width(px(1000.)) <= 240.);
        assert!(label_width(px(80.)) > label_width(px(20.)));
    }
}
