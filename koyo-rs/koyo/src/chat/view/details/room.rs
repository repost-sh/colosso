//! Where the card is (`docs/chat.md` 5.3): beside the chat where the pane has room for both, the
//! chat then keeping to what's left of the card, or over the chat where there's none, only when
//! asked for; and how tall it can get in each.

use gpui_kit::{App, Context, IntoElement, Pixels, Styled as _, canvas, px};

use super::super::ChatView;
use super::{COMPOSER, CONTROLS, Details, GAP, MARGIN};
use crate::settings;

/// The narrowest the chat gets beside the card: the composer's width at which three of its four
/// chips still say what they are (`ROOM_FOR_LABELS`), and a column a few words a line wider than
/// a phone's.
pub(super) const CHAT_ROOM: f32 = 480.;
/// The narrowest the card gets, beside the chat or over it.
pub(super) const NARROWEST: f32 = 240.;

impl Details {
    /// Whether the pane has room for the card beside the chat: the chat at its narrowest and the
    /// card at its narrowest, a margin either side of it.
    pub(super) fn has_room(&self) -> bool {
        self.pane.0 >= px(CHAT_ROOM + NARROWEST + 2. * MARGIN)
    }

    pub(in super::super) fn is_up(&self, cx: &App) -> bool {
        if self.resizing() {
            true
        } else if self.has_room() {
            settings::get(cx).chat_details.open
        } else {
            self.over
        }
    }

    /// How much of the pane's right the chat leaves the card: the card and a margin either side of
    /// it while it's up beside the chat, nothing while it's down or over the chat.
    pub(in super::super) fn aside(&self, cx: &App) -> Pixels {
        if self.has_room() && self.is_up(cx) {
            px(self.width(cx) + 2. * MARGIN)
        } else {
            Pixels::ZERO
        }
    }

    /// The widest the card can be: beside the chat, as leaves the chat its narrowest; over it, the
    /// pane less its margins.
    pub(super) fn widest(&self) -> f32 {
        let chat = if self.has_room() { CHAT_ROOM } else { 0. };
        self.pane.0.as_f32() - chat - 2. * MARGIN
    }

    /// The tallest the card can be: the pane's height less its controls and a margin under it,
    /// beside the chat; over it, less the composer's room too.
    pub(super) fn tallest(&self) -> f32 {
        let under = if self.has_room() { MARGIN } else { COMPOSER };
        (self.pane.1.as_f32() - (MARGIN + CONTROLS + GAP + under)).max(120.)
    }

    /// The card's up or it isn't, for this pane or wherever there's room, as it has any.
    pub(in super::super) fn toggle(&mut self, cx: &mut App) {
        if self.has_room() {
            settings::update(cx, |settings| settings.chat_details.open ^= true);
        } else {
            self.over = !self.over;
        }
    }

    fn measured(&mut self, pane: (Pixels, Pixels)) -> bool {
        let new = self.pane != pane;
        self.pane = pane;
        new
    }
}

/// Keeps how big the pane is, which is drawn before the card is.
pub(in super::super) fn measure(cx: &Context<ChatView>) -> impl IntoElement + use<> {
    let view = cx.weak_entity();
    canvas(
        move |bounds, _, cx| {
            let pane = (bounds.size.width, bounds.size.height);
            cx.defer(move |cx| {
                view.update(cx, |view, cx| {
                    if view.details.measured(pane) {
                        cx.notify();
                    }
                })
                .ok();
            });
        },
        |_, (), _, _| {},
    )
    .absolute()
    .size_full()
}
