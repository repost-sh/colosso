//! A picture a message attached, open in Koyo's viewer over the chat (`docs/chat.md` 5.3): filling
//! the tab over all that floats there, zoomed and moved about as a file tab's picture is, with the
//! keys, until Esc or its × closes it and they go back to the composer.

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, Focusable as _,
    InteractiveElement as _, IntoElement as _, KeyBinding, ParentElement as _, StyleRefinement,
    Styled as _, Window, actions, deferred, div,
};

use super::ChatView;
use crate::chat::timeline::{OpenPicture, Timeline};
use crate::shell::icon_button;
use crate::theme;
use crate::viewer::{Kind, Viewer};

actions!(chat, [ClosePicture]);

/// Where a picture open over a chat has the keys.
const KEYS: &str = "ChatPicture";

pub(in crate::chat) fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", ClosePicture, Some(KEYS))]);
}

impl ChatView {
    /// A person clicked a picture a message attached: it opens over the chat, and takes the keys.
    pub(super) fn picture_clicked(
        &mut self,
        _: &Entity<Timeline>,
        OpenPicture(path): &OpenPicture,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let viewer = cx.new(|cx| Viewer::new(path.clone(), Kind::Picture, cx));
        window.focus(&viewer.read(cx).focus_handle(cx), cx);
        self.picture = Some(viewer);
        cx.notify();
    }

    /// The picture closes, and the keys go back to the composer, or the bar in its place.
    fn close_picture(&mut self, _: &ClosePicture, window: &mut Window, cx: &mut Context<Self>) {
        self.picture = None;
        window.focus(&self.focus_handle(cx), cx);
        cx.notify();
    }

    /// The picture open over the chat, if one is: over all that floats there, as the glass of the
    /// composer is drawn under what's deferred.
    pub(super) fn picture_over(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let viewer = self.picture.clone()?;
        let close = icon_button("chat-picture-close", IconName::X, "Close", cx)
            .small()
            .on_click(cx.listener(|this, _, window, cx| {
                this.close_picture(&ClosePicture, window, cx);
            }));
        let over = div()
            .id("chat-picture")
            .debug_selector(|| "chat-picture".into())
            .key_context(KEYS)
            .on_action(cx.listener(Self::close_picture))
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme::palette(cx).floating())
            .child(AnyView::from(viewer).cached(StyleRefinement::default().size_full()))
            .child(div().absolute().top_2().right_2().child(close));
        Some(deferred(over).with_priority(1).into_any_element())
    }
}
