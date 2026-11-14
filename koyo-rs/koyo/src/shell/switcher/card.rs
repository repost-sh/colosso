//! A tab as the switcher shows it (`docs/tab-switcher.md` 3), as Arc's switcher does: a picture
//! of it (`shell/pictures.rs`) over its icon and title, in a card that's lit, as macOS's
//! switcher lights the app it picks, when it's the one picked. A tab of another project than
//! the one shown carries that project's picture on its own's corner.

use std::path::Path;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, FontWeight, InteractiveElement as _, IntoElement, MouseMoveEvent,
    ObjectFit, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled, div, img,
    px, surface,
};
use objc2_core_foundation::CFRetained;
use objc2_core_video::CVPixelBuffer;

use super::Switcher;
use crate::arc::avatar;
use crate::model::{Spot, Visited, Workspace};
use crate::pane::unsaved_dot;
use crate::shell::elements::ellipsized;
use crate::shell::pictures;
use crate::shell::tab_face::Face;
use crate::theme::Palette;

// The sizes here are in points at the panel's full size, which a narrow screen shrinks.

/// The room round what's in a card, and between its picture and its title.
const INSET: f32 = 9.;
/// The picture's size and corners: about 16:10, its corners concentric with the card's.
const PICTURE_WIDTH: f32 = 156.;
const PICTURE_HEIGHT: f32 = 98.;
const PICTURE_RADIUS: f32 = CARD_RADIUS - INSET;
/// The row under the picture: its height, the tab's icon, its title's size and the room
/// between the two; and the icon large in a picture there's none of yet.
const TITLE_ROW: f32 = 18.;
const ICON: f32 = 16.;
const TITLE: f32 = 14.;
const TITLE_GAP: f32 = 6.;
const LARGE_ICON: f32 = 32.;
/// A card's size, round its picture and its title, its corners and the room between cards:
/// five of them a little wider than macOS's ⌘⇥ with five apps.
pub(super) const CARD_WIDTH: f32 = PICTURE_WIDTH + 2. * INSET;
pub(super) const CARD_HEIGHT: f32 = PICTURE_HEIGHT + TITLE_ROW + 3. * INSET;
pub(super) const CARD_RADIUS: f32 = 21.;
pub(super) const CARD_GAP: f32 = 12.;
/// The project's picture on the corner of its tab's: its size, its corners, its initials' size
/// while it has none, and how far in from the picture's corner.
const BADGE: f32 = 21.;
const BADGE_RADIUS: f32 = 7.;
const BADGE_INITIALS: f32 = 10.;
const BADGE_IN: f32 = 6.;

/// A tab as its card shows it.
pub(super) struct Card {
    /// Its first pane's face, whose icon it shows.
    face: Face,
    /// Its panes' titles, a split's side by side.
    pub(super) title: SharedString,
    /// The picture of it, as the window last drew it: none until it's been shown.
    picture: Option<CFRetained<CVPixelBuffer>>,
    preview: bool,
    dirty: bool,
    /// The project it's of, when that's not the one shown.
    project: Option<Badge>,
}

/// A project as its badge shows it: its tile's picture, or its initials until it has one.
struct Badge {
    picture: Option<Arc<Path>>,
    initials: SharedString,
}

impl Card {
    /// `visited` as its card shows it in `workspace`, whose project shown its badge is told
    /// from; none for a tab with no pane, which there isn't.
    pub(super) fn of(visited: &Visited<'_>, workspace: &Workspace, cx: &mut App) -> Option<Card> {
        let faces = Face::all(visited.tab, cx);
        let titles: Vec<&str> = faces.iter().map(|face| face.title.as_ref()).collect();
        let title = titles.join("  ·  ").into();
        let preview = matches!(faces.as_slice(), [only] if only.preview);
        let dirty = faces.iter().any(|face| face.dirty);
        let project = match visited.spot {
            Spot::Checkout(project, ..) if Some(project) != workspace.active_project() => {
                workspace.projects.get(project).map(|project| Badge {
                    picture: avatar::picture(&project.path, cx).map(Arc::from),
                    initials: project.initials(),
                })
            }
            _ => None,
        };
        let face = faces.into_iter().next()?;
        Some(Card {
            picture: pictures::of(face.id, cx),
            face,
            title,
            preview,
            dirty,
            project,
        })
    }

    /// What its tab is known by wherever it's moved: its first pane.
    pub(super) fn key(&self) -> u64 {
        self.face.id
    }

    /// Whether it has a picture of its tab to show, as a test asks.
    #[cfg(test)]
    pub(super) fn has_picture(&self) -> bool {
        self.picture.is_some()
    }

    /// The card at `index`, lit if `picked`, at `scale` of its full size: picked as the pointer
    /// moves over it, and its tab shown once it's clicked.
    pub(super) fn element(
        &self,
        index: usize,
        picked: bool,
        scale: f32,
        p: Palette,
        cx: &mut Context<Switcher>,
    ) -> AnyElement {
        let at = |points: f32| px(points * scale);
        let title = div()
            .h(at(TITLE_ROW))
            .min_w_0()
            .flex()
            .items_center()
            .gap(at(TITLE_GAP))
            .text_size(at(TITLE))
            .child(self.face.icon(ICON * scale))
            .child(
                ellipsized(self.title.clone())
                    .flex_1()
                    .when(self.preview, Styled::italic),
            )
            .when(self.dirty, |title| title.child(unsaved_dot(p)));
        div()
            .id(("tab-switcher-card", index))
            .debug_selector(move || format!("tab-switcher-card-{index}"))
            .flex_shrink_0()
            .w(at(CARD_WIDTH))
            .h(at(CARD_HEIGHT))
            .p(at(INSET))
            .flex()
            .flex_col()
            .gap(at(INSET))
            .rounded(at(CARD_RADIUS))
            .when(picked, |card| card.bg(p.text.alpha(0.12)))
            .child(self.picture(scale, p))
            .child(title)
            .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                this.select(index, cx);
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select(index, cx);
                this.pick(cx);
            }))
            .into_any_element()
    }

    /// Its picture, with its project's on the corner when it's another's than the one shown.
    /// Until there's a picture of it, its icon, large.
    fn picture(&self, scale: f32, p: Palette) -> impl IntoElement {
        let at = |points: f32| px(points * scale);
        let taken = self.picture.clone().map(|picture| {
            (surface(picture).object_fit(ObjectFit::Fill))
                .absolute()
                .top_0()
                .left_0()
                .size_full()
        });
        let badge = self.project.as_ref().map(|badge| {
            let face = match &badge.picture {
                Some(picture) => img(picture.clone()).size_full().into_any_element(),
                None => div()
                    .text_size(at(BADGE_INITIALS))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.muted)
                    .child(badge.initials.clone())
                    .into_any_element(),
            };
            div()
                .absolute()
                .top(at(BADGE_IN))
                .left(at(BADGE_IN))
                .size(at(BADGE))
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .rounded(at(BADGE_RADIUS))
                .bg(p.card)
                .border_1()
                .border_color(p.hairline)
                .child(face)
        });
        div()
            .relative()
            .w(at(PICTURE_WIDTH))
            .h(at(PICTURE_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .rounded(at(PICTURE_RADIUS))
            .bg(p.card)
            .when(taken.is_none(), |picture| {
                picture.child(self.face.icon(LARGE_ICON * scale))
            })
            .children(taken)
            .children(badge)
    }
}
