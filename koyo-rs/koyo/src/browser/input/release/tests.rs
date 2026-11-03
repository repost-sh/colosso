//! Presses the window never sees the end of, in a window of its own: a move with no button
//! held, while the window holds a press on the page, has it let the press go.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use koyo_browser_ipc as ipc;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Context, DispatchPhase, Entity, IntoElement, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection,
    ParentElement as _, Pixels, PlatformInput, Point, Render, Styled as _, TestAppContext, Window,
    canvas, div, point, px, size,
};

use crate::browser::view::BrowserView;
use crate::test_support::{draw, in_window};

/// The releases the window dispatched, in order.
type Released = Rc<RefCell<Vec<MouseButton>>>;

/// A page in a window, and every release the window dispatches, noted.
struct Noting {
    page: Entity<BrowserView>,
    released: Released,
}

impl Render for Noting {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let released = self.released.clone();
        let noting = canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, _| {
                    if phase == DispatchPhase::Capture {
                        released.borrow_mut().push(event.button);
                    }
                });
            },
        )
        .absolute()
        .size_full();
        div().size_full().child(self.page.clone()).child(noting)
    }
}

/// A start page drawn in a window of its own, so it's placed, a point inside it, and the
/// releases the window dispatches from then on.
fn page_in_window(
    cx: &mut TestAppContext,
) -> (
    AnyWindowHandle,
    Entity<BrowserView>,
    Point<Pixels>,
    Released,
) {
    cx.update(crate::init);
    let released: Released = Rc::new(RefCell::new(Vec::new()));
    let noted = released.clone();
    let (window, noting) = in_window(size(px(800.), px(600.)), cx, |window, cx| {
        let page = cx.new(|cx| BrowserView::new(String::new(), Path::new("/work/app"), window, cx));
        cx.new(|_| Noting {
            page,
            released: noted,
        })
    });
    draw(window, cx);
    let (page, inside) = noting.read_with(cx, |noting, cx| {
        let page = noting.page.clone();
        let inside = page.read(cx).origin + point(px(10.), px(10.));
        (page, inside)
    });
    (window, page, inside, released)
}

/// `event`, as the page's native view would hand it on; the deferred releases run with the
/// draw after.
fn hand_on(
    window: AnyWindowHandle,
    page: &Entity<BrowserView>,
    event: &PlatformInput,
    cx: &mut TestAppContext,
) {
    cx.update_window(window, |_, window, cx| {
        page.update(cx, |page, cx| page.native_pointer(event, window, cx));
    })
    .unwrap();
    draw(window, cx);
}

fn pressed(button: MouseButton, at: Point<Pixels>) -> PlatformInput {
    PlatformInput::MouseDown(MouseDownEvent {
        button,
        position: at,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    })
}

fn released(button: MouseButton, at: Point<Pixels>) -> PlatformInput {
    PlatformInput::MouseUp(MouseUpEvent {
        button,
        position: at,
        modifiers: Modifiers::default(),
        click_count: 1,
    })
}

fn moved(at: Point<Pixels>, pressed_button: Option<MouseButton>) -> PlatformInput {
    PlatformInput::MouseMove(MouseMoveEvent {
        position: at,
        pressed_button,
        modifiers: Modifiers::default(),
    })
}

#[gpui_kit::test]
fn a_move_with_no_button_held_is_the_pages_while_the_window_holds_no_press(
    cx: &mut TestAppContext,
) {
    let (window, page, inside, released) = page_in_window(cx);
    hand_on(window, &page, &moved(inside, None), cx);
    assert!(released.borrow().is_empty());
    page.read_with(cx, |page, _| {
        assert!(page.hovered, "the pointer is the page's");
    });
}

#[gpui_kit::test]
fn a_press_the_window_holds_on_the_page_is_let_go_once_the_pointer_belies_it(
    cx: &mut TestAppContext,
) {
    let (window, page, inside, noted) = page_in_window(cx);
    hand_on(window, &page, &pressed(MouseButton::Left, inside), cx);
    page.read_with(cx, |page, _| {
        assert_eq!(page.pressed, Some(ipc::MouseButton::Left));
        assert_eq!(page.window_presses, [MouseButton::Left]);
    });
    // Held, the button is the page's wherever the pointer goes: a drag.
    let further = inside + point(px(30.), px(0.));
    hand_on(window, &page, &moved(further, Some(MouseButton::Left)), cx);
    assert!(noted.borrow().is_empty());
    // The window never saw the release; the pointer moving with no button held says so.
    hand_on(window, &page, &moved(inside, None), cx);
    assert_eq!(
        *noted.borrow(),
        [MouseButton::Left],
        "the window hears the release it never got"
    );
    page.read_with(cx, |page, _| {
        assert!(page.pressed.is_none(), "the page's own press ends");
        assert!(page.window_presses.is_empty());
        assert!(!page.hovered, "and the page is left");
    });
}

#[gpui_kit::test]
fn a_press_the_window_saw_the_end_of_is_nothing_to_let_go(cx: &mut TestAppContext) {
    let (window, page, inside, noted) = page_in_window(cx);
    hand_on(window, &page, &pressed(MouseButton::Left, inside), cx);
    hand_on(window, &page, &released(MouseButton::Left, inside), cx);
    hand_on(window, &page, &moved(inside, None), cx);
    assert!(noted.borrow().is_empty());
    page.read_with(cx, |page, _| assert!(page.hovered));
}

#[gpui_kit::test]
fn a_navigate_press_whose_release_never_comes_is_let_go_the_same_way(cx: &mut TestAppContext) {
    let (window, page, inside, noted) = page_in_window(cx);
    let back = MouseButton::Navigate(NavigationDirection::Back);
    hand_on(window, &page, &pressed(back, inside), cx);
    assert!(noted.borrow().is_empty(), "a release may yet come");
    page.read_with(cx, |page, _| {
        assert!(page.pressed.is_none(), "not a press of the page's");
    });
    hand_on(window, &page, &moved(inside, None), cx);
    assert_eq!(*noted.borrow(), [back]);
}
