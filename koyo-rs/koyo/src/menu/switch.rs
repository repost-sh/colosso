//! The switch on a menu's row: the one `AppKit` draws, as in Settings, or gpui-kit's where the window
//! can't host a native view. A flip of it is a pick of its row.

use gpui_kit::component::switch::Switch;
use gpui_kit::{AnyElement, Context, ElementId, IntoElement as _, Window};

use super::view::Rows;
use crate::theme;

fn id_of(index: usize) -> ElementId {
    ElementId::NamedInteger("menu-row-switch".into(), index as u64)
}

pub(super) fn row_switch(
    index: usize,
    on: bool,
    window: &mut Window,
    cx: &mut Context<Rows>,
) -> AnyElement {
    #[cfg(target_os = "macos")]
    if let Some(switch) = appkit::switch(index, on, window, cx) {
        return switch;
    }
    let _ = window;
    Switch::new(id_of(index))
        .checked(on)
        .color(theme::palette(cx).accent)
        .into_any_element()
}

#[cfg(target_os = "macos")]
mod appkit {
    use gpui_kit::{
        AnyElement, App, Context, IntoElement as _, ParentElement as _, Styled as _, WeakEntity,
        Window, div,
    };
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSControlSize, NSSwitch};

    use super::{Rows, id_of};
    use crate::native::{self, Control, switch};

    struct RowSwitch {
        rows: WeakEntity<Rows>,
        index: usize,
    }

    impl Control for RowSwitch {
        type View = NSSwitch;
        type Shown = bool;

        fn make(&self, main: MainThreadMarker) -> Retained<NSSwitch> {
            let view = NSSwitch::new(main);
            view.setControlSize(NSControlSize::Small);
            // What's typed keeps the keys, flipped or not.
            view.setRefusesFirstResponder(true);
            view
        }

        fn show(view: &NSSwitch, on: bool) {
            switch::show(view, on);
        }

        fn acted(&self, view: &NSSwitch, shown: Option<bool>, cx: &mut App) {
            if shown.is_some_and(|shown| shown != switch::is_on(view)) {
                let index = self.index;
                self.rows.update(cx, |rows, cx| rows.pick(index, cx)).ok();
            }
        }
    }

    pub(super) fn switch(
        index: usize,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Rows>,
    ) -> Option<AnyElement> {
        let control = RowSwitch {
            rows: cx.weak_entity(),
            index,
        };
        let (size, track) = native::host(id_of(index), control, on, window, cx)?;
        Some(div().w(size.width).child(track).into_any_element())
    }
}
