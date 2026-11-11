//! A question before Koyo does what the person may not want (a simulator past their limit, one
//! deleted with all it held): what it is, Cancel, and the way on. Asked over the window they
//! clicked in; what's said yes to is done once.

use gpui_kit::{App, Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div, px};

use super::{Button, Tone};
use crate::theme;

const WIDTH: f32 = 420.;
const GAP: f32 = 10.;
/// The lines what it says may take.
const LINES: f32 = 3.;

/// What the person is asked: its title, what it means, and the button that goes on.
pub struct Confirm {
    pub title: String,
    pub text: String,
    pub yes: &'static str,
    pub tone: Tone,
}

/// What's done once the person says yes.
type Then = Box<dyn FnOnce(&mut App)>;

struct Asking {
    confirm: Confirm,
    then: Option<Then>,
}

/// Asks `confirm` over `window`; `then` once the person says yes.
pub fn confirm(confirm: Confirm, then: impl FnOnce(&mut App) + 'static, window: &mut Window, cx: &mut App) {
    let title: SharedString = confirm.title.clone().into();
    let heading = super::heading_height(&title, WIDTH, window);
    let height = 2. * super::PADDING + heading + GAP + LINES * super::SMALL_LINE + GAP + super::FOOTER;
    let asking = Asking {
        confirm,
        then: Some(Box::new(then)),
    };
    super::open(WIDTH, height, window, cx, |_, _| asking);
}

impl Render for Asking {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let yes = Button::new("confirm-yes", self.confirm.yes, self.confirm.tone).on_click(cx.listener(|this, _, window, cx| {
            if let Some(then) = this.then.take() {
                super::dismiss(window, cx);
                then(cx);
            }
        }));
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(super::heading(self.confirm.title.clone()))
            .child(div().flex_1().text_xs().text_color(p.muted).child(self.confirm.text.clone()))
            .child(super::footer(None, p).child(super::cancel("confirm-cancel")).child(yes))
    }
}
