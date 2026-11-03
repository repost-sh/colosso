//! A second tab used as a person uses one: a field on its page typed into, the page scrolled
//! with the wheel, its context menu asked for and the address bar pressed after it, then the
//! first tab picked up from the strip and dropped on the right of this tab's page, the two
//! pages side by side.

use gpui_kit::{
    Entity, Focusable as _, Modifiers, MouseButton, ScrollDelta, ScrollEvent, point, px,
};

use super::{Check, PATIENCE, fits, loaded, shown_page};
use crate::browser::BrowserView;
use crate::theme::UseLightTheme;
use crate::visual::Result;

/// The second tab's page: a field fixed at its top left, and room to scroll.
pub(super) const PLAY: &str = "<!doctype html><title>Play</title>\
    <body style='margin:0;height:4000px;font:16px -apple-system,sans-serif;\
    background:linear-gradient(#fff,#9ab)'>\
    <input placeholder='Type here' style='position:fixed;top:40px;left:40px;width:300px;\
    height:32px;font:16px -apple-system,sans-serif' oninput=\"document.title='typed '+this.value\">\
    <script>addEventListener('scroll',()=>document.title='scrolled '+Math.round(scrollY))</script>";

impl Check {
    /// The second tab, after `first`'s `DevTools` are put away in the light theme.
    pub(super) fn play(&mut self, first: &Entity<BrowserView>, port: u16) -> Result<()> {
        let (shell, window) = (self.drawn.shell.clone(), self.drawn.window);
        self.drawn.cx.update(|cx| {
            first.update(cx, BrowserView::close_devtools);
            cx.dispatch_action(&UseLightTheme);
        });
        (self.drawn.cx)
            .update_window(window, |_, window, cx| {
                shell.update(cx, |shell, cx| shell.open_browser(None, window, cx));
            })
            .map_err(|error| error.to_string())?;
        let play = (self.drawn.cx.read(|cx| shown_page(&shell, cx))).ok_or("no second tab")?;
        self.type_text(&format!("127.0.0.1:{port}/play"));
        self.keys("enter");
        self.until(&play, "the play page", |view| loaded(view, "Play"))?;

        let none = Modifiers::default();
        let origin = self.drawn.cx.read(|cx| play.read(cx).origin);
        (self.drawn.cx).simulate_click(window, origin + point(px(120.), px(56.)), none);
        self.type_text("hello");
        self.until(&play, "the field typed into", |view| {
            view.page.title == "typed hello"
        })?;
        let wheel = ScrollEvent {
            position: origin + point(px(500.), px(400.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-600.))),
            ..ScrollEvent::default()
        };
        (self.drawn.cx).simulate_event(window, wheel);
        self.until(&play, "the page scrolled", |view| {
            view.page.title.starts_with("scrolled ") && view.page.title != "scrolled 0"
        })?;
        self.save("typed-and-scrolled.png", &[&play])?;
        self.press_after_menu(&play)?;
        self.split(first, &play)
    }

    /// A right press on `play`'s page held while the page's menu shows, as the menu takes
    /// its release, then a press on the address bar: the bar takes the keys, so the app's own
    /// buttons still answer once a page's menu has been up.
    fn press_after_menu(&mut self, play: &Entity<BrowserView>) -> Result<()> {
        let (window, none) = (self.drawn.window, Modifiers::default());
        let origin = self.drawn.cx.read(|cx| play.read(cx).origin);
        let on_page = origin + point(px(500.), px(300.));
        (self.drawn.cx).simulate_mouse_down(window, on_page, MouseButton::Right, none);
        self.until(play, "the page's context menu", |view| {
            view.open_menu.is_some()
        })?;
        // The toolbar is 40 px tall, right over the page; its address field fills its middle.
        (self.drawn.cx).simulate_click(window, origin + point(px(300.), px(-20.)), none);
        let page = play.clone();
        (self.drawn)
            .until(PATIENCE, |drawn| {
                let window = drawn.window;
                (drawn.cx)
                    .update_window(window, |_, window, cx| {
                        let address = &page.read(cx).address;
                        address.read(cx).focus_handle(cx).is_focused(window)
                    })
                    .map_err(|error| error.to_string())
            })
            .map_err(|why| format!("the address bar pressed after the page's menu: {why}"))
    }

    /// `first`'s tab, picked up from the strip's start over the page's toolbar and dropped on
    /// the right of `play`'s page, where the page's native layer lies under the drop target.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a page's size in pixels is far below 2^24"
    )]
    fn split(&mut self, first: &Entity<BrowserView>, play: &Entity<BrowserView>) -> Result<()> {
        let (window, none) = (self.drawn.window, Modifiers::default());
        let held = Some(MouseButton::Left);
        let (origin, wide, tall) = self.drawn.cx.read(|cx| {
            let view = play.read(cx);
            let room = view.viewport.as_ref();
            let room = room.map_or((0., 0.), |room| (room.width as f32, room.height as f32));
            (view.origin, room.0, room.1)
        });
        let tab = origin + point(px(70.), px(-59.));
        let drop = origin + point(px(wide - 20.), px(tall / 2.));
        let cx = &mut self.drawn.cx;
        cx.simulate_mouse_down(window, tab, MouseButton::Left, none);
        cx.simulate_mouse_move(window, tab + point(px(30.), px(30.)), held, none);
        // The drop targets show once something's in hand.
        self.drawn.draw()?;
        self.drawn.cx.simulate_mouse_move(window, drop, held, none);
        self.drawn.draw()?;
        (self.drawn.cx).simulate_mouse_up(window, drop, MouseButton::Left, none);
        let (dropped, onto) = (first.clone(), play.clone());
        (self.drawn)
            .until(PATIENCE, |drawn| {
                Ok(drawn.cx.read(|cx| {
                    let (dropped, onto) = (dropped.read(cx), onto.read(cx));
                    fits(dropped) && fits(onto) && dropped.origin.x > onto.origin.x + px(100.)
                }))
            })
            .map_err(|why| format!("the first tab dropped on the right of the page: {why}"))?;
        self.save("split.png", &[play, first])
    }
}
