//! Koyo: what a native surface of a window's composition covers of GPUI's own content, and for
//! how long.

use std::{any::Any, cell::Cell, rc::Rc};

use gpui::{
    AnyWindowHandle, Bounds, Context, DevicePixels, InteractiveElement as _, IntoElement,
    Modifiers, MouseButton, ParentElement as _, PlatformSurfaceAttachment, Point, Render, Result,
    Styled as _, TestAppContext, VisualTestContext, Window, WindowCompositionSurface, canvas, div,
    point, px,
};

/// Where a native view lies: all a test's window has of one.
#[derive(Default)]
struct Place(Cell<Bounds<DevicePixels>>);

impl PlatformSurfaceAttachment for Place {
    fn set_bounds(&self, bounds: Bounds<DevicePixels>) -> Result<()> {
        self.0.set(bounds);
        Ok(())
    }

    fn bounds(&self) -> Bounds<DevicePixels> {
        self.0.get()
    }

    fn set_parent_origin(&self, _: Point<DevicePixels>) -> Result<()> {
        Ok(())
    }

    fn set_visible(&self, _: bool) -> Result<()> {
        Ok(())
    }

    fn platform_handle(&self) -> Result<Box<dyn Any>> {
        Ok(Box::new(()))
    }
}

/// A native view's surface for as long as the view is drawn: it goes once the frame that stopped
/// drawing the view is drawn.
struct Held {
    window: AnyWindowHandle,
    surface: WindowCompositionSurface,
    gone: Rc<Cell<bool>>,
}

impl Held {
    fn new(gone: Rc<Cell<bool>>, window: &Window, cx: &mut Context<Self>) -> Held {
        cx.on_release(|held: &mut Self, cx| {
            let (window, id, gone) = (held.window, held.surface.id(), held.gone.clone());
            cx.defer(move |cx| {
                let removed = window.update(cx, |_, window, _| {
                    window.enable_window_composition()?.remove_surface(id)
                });
                removed.unwrap().unwrap();
                gone.set(true);
            });
        })
        .detach();
        let composition = window.enable_window_composition().unwrap();
        let surface = composition
            .create_external_gpu_surface(Rc::<Place>::default())
            .unwrap();
        Held {
            window: window.window_handle(),
            surface,
            gone,
        }
    }
}

/// A pane that a native view covers, or not, how many presses of its own it heard, and whether
/// the view has gone from the window.
struct Pane {
    covered: bool,
    heard: usize,
    gone: Rc<Cell<bool>>,
}

impl Render for Pane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hear = cx.listener(|pane, _, _, _| pane.heard += 1);
        let pane = div().size_full().on_mouse_down(MouseButton::Left, hear);
        if !self.covered {
            return pane;
        }
        let gone = self.gone.clone();
        let held = window.use_keyed_state("native", cx, |window, cx| Held::new(gone, window, cx));
        let surface = held.read(cx).surface.clone();
        let place = move |bounds: Bounds<_>, window: &mut Window, _: &mut _| {
            let bounds = bounds.to_device_pixels(window.scale_factor());
            let surface = surface.platform_surface().unwrap();
            surface.set_bounds(bounds).unwrap();
        };
        pane.child(canvas(place, |_, (), _, _| ()).size_full())
    }
}

/// A native view that has gone covers nothing: the first press where it was is heard by what was
/// drawn in its place, with no frame drawn since it went.
#[gpui::test]
fn the_first_press_where_a_native_view_was_is_heard(cx: &mut TestAppContext) {
    let gone = Rc::new(Cell::new(false));
    let window = cx.add_window({
        let gone = gone.clone();
        move |_, _| Pane {
            covered: true,
            heard: 0,
            gone,
        }
    });
    let heard_after_a_press = |cx: &mut TestAppContext| {
        let at = point(px(50.), px(50.));
        VisualTestContext::from_window(window.into(), cx).simulate_click(at, Modifiers::default());
        window.read_with(cx, |pane, _| pane.heard).unwrap()
    };
    cx.run_until_parked();
    assert_eq!(heard_after_a_press(cx), 0, "the press is the native view's");

    window
        .update(cx, |pane, _, cx| {
            pane.covered = false;
            cx.notify();
        })
        .unwrap();
    // A test's frame is drawn last in a flush, and what it dropped is let go of in the next.
    cx.update(|_| {});
    assert!(gone.get(), "the view has gone from the window");
    assert_eq!(heard_after_a_press(cx), 1, "the press is the pane's");
}
