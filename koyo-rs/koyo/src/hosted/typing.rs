//! What the person types in a device's tab or the agent desktop's, as their keyboard layout and
//! input method make it: text, sent once it's made, which the host types on as characters.

/// Has `$view` take the Mac's typing and pass each text it makes to its own `send` as
/// `$input::Text`. A macro: the input handler is GPUI's trait, so one blanket impl can't serve
/// both views.
macro_rules! typed_as_text {
    ($view:ty, $input:ident) => {
        const _: () = {
            use std::ops::Range;

            use gpui_kit::{
                Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window, px,
            };

            impl EntityInputHandler for $view {
                fn text_for_range(
                    &mut self,
                    _: Range<usize>,
                    _: &mut Option<Range<usize>>,
                    _: &mut Window,
                    _: &mut Context<Self>,
                ) -> Option<String> {
                    None
                }

                fn selected_text_range(
                    &mut self,
                    _: bool,
                    _: &mut Window,
                    _: &mut Context<Self>,
                ) -> Option<UTF16Selection> {
                    Some(UTF16Selection {
                        range: 0..0,
                        reversed: false,
                    })
                }

                fn marked_text_range(
                    &self,
                    _: &mut Window,
                    _: &mut Context<Self>,
                ) -> Option<Range<usize>> {
                    None
                }

                fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

                /// What the keyboard typed, as the person's layout and input method made it.
                fn replace_text_in_range(
                    &mut self,
                    _: Option<Range<usize>>,
                    text: &str,
                    _: &mut Window,
                    cx: &mut Context<Self>,
                ) {
                    if !text.is_empty() {
                        self.send($input::Text { text: text.into() }, cx);
                    }
                }

                /// Text still being composed stays with the input method: it's sent once it's
                /// made.
                fn replace_and_mark_text_in_range(
                    &mut self,
                    _: Option<Range<usize>>,
                    _: &str,
                    _: Option<Range<usize>>,
                    _: &mut Window,
                    _: &mut Context<Self>,
                ) {
                }

                fn bounds_for_range(
                    &mut self,
                    _: Range<usize>,
                    element_bounds: Bounds<Pixels>,
                    _: &mut Window,
                    _: &mut Context<Self>,
                ) -> Option<Bounds<Pixels>> {
                    Some(Bounds::new(
                        gpui_kit::point(
                            element_bounds.left() + px(12.),
                            element_bounds.bottom() - px(28.),
                        ),
                        gpui_kit::size(px(1.), px(20.)),
                    ))
                }

                fn character_index_for_point(
                    &mut self,
                    _: Point<Pixels>,
                    _: &mut Window,
                    _: &mut Context<Self>,
                ) -> Option<usize> {
                    None
                }
            }
        };
    };
}

pub(crate) use typed_as_text;
