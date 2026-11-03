//! What keeps a native control for as long as its page shows it: the `AppKit` control in its
//! surface, the actions it sends, what it was last told to show and where on the page it goes.

use std::sync::Once;

use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedReceiver};
use gpui_kit::{
    App, Context, Div, ElementId, HitboxBehavior, ParentElement as _, Pixels, Size, Styled as _,
    Task, Window, div, px, size,
};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::{NSControl, NSView};

use super::surface::Surface;
use super::target::Target;

/// A setting's `AppKit` control: how it's made, told what to show and heard when it's used.
pub trait Control: 'static {
    /// The control `AppKit` draws.
    type View: Message + AsRef<NSControl> + AsRef<NSView>;
    /// What the page tells the control to show.
    type Shown: Copy + PartialEq + 'static;

    /// The control, set up; it's sized to fit and put in the window after.
    fn make(&self, main: MainThreadMarker) -> Retained<Self::View>;

    /// Has `view` show `shown`, leaving alone what it shows already: a change of its own
    /// it's reporting.
    fn show(view: &Self::View, shown: Self::Shown);

    /// Takes up a change `view` reports, the control as it was last `shown`.
    fn acted(&self, view: &Self::View, shown: Option<Self::Shown>, cx: &mut App);
}

/// `control`'s `AppKit` view, the one `id` names, showing `shown`: how big `AppKit` draws it,
/// and a div that size, full width, it's put over. None where the window can't host a native
/// view, which falls back to the GPUI control.
pub fn host<C: Control>(
    id: &'static str,
    control: C,
    shown: C::Shown,
    window: &mut Window,
    cx: &mut App,
) -> Option<(Size<Pixels>, Div)> {
    let key = ElementId::NamedInteger(id.into(), 0);
    let hosted = window.use_keyed_state(key, cx, |window, cx| Hosted::new(control, window, cx));
    let size = hosted.update(cx, |hosted, _| {
        let native = hosted.native.as_ref()?;
        if hosted.shown != Some(shown) {
            C::show(&native.view, shown);
            hosted.shown = Some(shown);
        }
        Some(native.size)
    })?;
    // As a hitbox there is clipped to what of a scroll layer shows rather than all it paints,
    // the control is put over its bounds and shown as far as the page shows them. Listening
    // for its child keeps a scrolling page off a scroll layer too, which would move the div
    // without prepainting it.
    let track = div()
        .w_full()
        .h(size.height)
        .on_children_prepainted(move |children, window, cx| {
            if let (Some(&bounds), Some(native)) = (children.first(), &hosted.read(cx).native) {
                let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                native
                    .surface
                    .place(bounds, hitbox.content_mask.bounds, window);
            }
        })
        .child(div().size_full());
    Some((size, track))
}

/// A control's `AppKit` half and what it was last told to show. Dropped with the page, which
/// takes the control out of the window.
struct Hosted<C: Control> {
    control: C,
    /// What `AppKit` was last told, so it's told only what changes.
    shown: Option<C::Shown>,
    native: Option<Native<C::View>>,
    _acts: Task<()>,
}

/// The control in its surface, sending its actions to its target.
struct Native<V: Message> {
    surface: Surface,
    view: Retained<V>,
    _target: Retained<Target>,
    size: Size<Pixels>,
}

impl<C: Control> Hosted<C> {
    fn new(control: C, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (tell, told) = mpsc::unbounded();
        let native = match Native::new(&control, window, tell) {
            Ok(native) => Some(native),
            Err(why) => {
                static SAID: Once = Once::new();
                SAID.call_once(|| log::warn!("GPUI's controls stand in for AppKit's: {why}"));
                None
            }
        };
        cx.on_release(|hosted: &mut Self, cx| {
            if let Some(native) = hosted.native.take() {
                native.remove(cx);
            }
        })
        .detach();
        Hosted {
            control,
            shown: None,
            native,
            _acts: listen(told, cx),
        }
    }
}

impl<V: Message + AsRef<NSControl> + AsRef<NSView>> Native<V> {
    fn new<C: Control<View = V>>(
        control: &C,
        window: &Window,
        tell: mpsc::UnboundedSender<()>,
    ) -> Result<Self, String> {
        let main = MainThreadMarker::new().ok_or("not on the main thread")?;
        let view = control.make(main);
        let as_control: &NSControl = (*view).as_ref();
        as_control.sizeToFit();
        let fitted = as_control.frame().size;
        let surface = Surface::new(window, (*view).as_ref())?;
        Ok(Native {
            surface,
            _target: Target::attach(main, as_control, tell),
            size: size(px(fitted.width as f32), px(fitted.height as f32)),
            view,
        })
    }

    /// Takes the control out of the window, having it stop sending to its target first.
    fn remove(self, cx: &mut App) {
        Target::detach((*self.view).as_ref());
        self.surface.remove(cx);
    }
}

/// Takes up the actions the control sends as long as the page holds it: once for all that are
/// waiting, before the page is drawn again, as each says only that the control changed and
/// the control is read as it is now.
fn listen<C: Control>(mut acts: UnboundedReceiver<()>, cx: &mut Context<Hosted<C>>) -> Task<()> {
    cx.spawn(async move |hosted, cx| {
        while acts.next().await.is_some() {
            while acts.try_recv().is_ok() {}
            let heard = hosted.update(cx, |hosted, cx| {
                if let Some(native) = &hosted.native {
                    hosted.control.acted(&native.view, hosted.shown, cx);
                }
            });
            if heard.is_err() {
                break;
            }
        }
    })
}
