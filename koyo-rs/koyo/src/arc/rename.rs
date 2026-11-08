//! A dialog asking for a name (`docs/arc-sidebar.md` 3, 6): a project's in Koyo, or a space's.
//! Enter or Rename gives it to whoever asked; Escape or Cancel leaves the name as it was.

use std::rc::Rc;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div, px,
};

use crate::modal::{self, Button, Tone};
use crate::theme;

const WIDTH: f32 = 360.;
const GAP: f32 = 12.;

type Done = Rc<dyn Fn(String, &mut App)>;

/// Asks for a name under `title`, the field holding `current` and saying `placeholder` once
/// it's emptied; `done` hears the name, trimmed, once it's given.
pub fn open(
    title: &'static str,
    current: String,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
    done: impl Fn(String, &mut App) + 'static,
) {
    let height =
        2. * modal::PADDING + modal::HEADING_LINE + GAP + modal::FIELD + GAP + modal::FOOTER;
    let done: Done = Rc::new(done);
    modal::open(WIDTH, height, window, cx, move |window, cx| {
        Rename::new(title.into(), current, placeholder, done, window, cx)
    });
}

struct Rename {
    title: SharedString,
    input: Entity<InputState>,
    done: Done,
    _subscription: Subscription,
}

impl Rename {
    fn new(
        title: SharedString,
        current: String,
        placeholder: &'static str,
        done: Done,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        input.update(cx, |input, cx| {
            input.set_value(current, window, cx);
            input.select_all(window, cx);
        });
        input.focus_handle(cx).focus(window, cx);
        let _subscription = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.confirm(window, cx);
            }
        });
        Rename {
            title,
            input,
            done,
            _subscription,
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.input.read(cx).value().trim().to_owned();
        let done = Rc::clone(&self.done);
        modal::dismiss(window, cx);
        done(name, cx);
    }
}

impl Render for Rename {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(modal::heading(self.title.clone()))
            .child(
                div()
                    .mt(px(GAP))
                    .child(modal::field(Input::new(&self.input).small(), p)),
            )
            .child(div().flex_1())
            .child(
                modal::footer(None, p)
                    .child(modal::cancel("rename-cancel"))
                    .child(
                        Button::new("rename-confirm", "Rename", Tone::Primary)
                            .on_click(cx.listener(|this, _, window, cx| this.confirm(window, cx))),
                    ),
            )
    }
}
