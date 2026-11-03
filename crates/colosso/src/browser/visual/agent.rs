//! A tab an agent works in, as its person sees it: the glow while it holds the page, its
//! pointer on its way along an arc to a button, the ring its click sends out where it lands,
//! a second arc that leaves as the first arrived, and the pointer gone six seconds after its
//! last click while the glow stays. The buttons name themselves in the page's title once
//! clicked, so each click is seen to land where the pointer went.

use std::time::{Duration, Instant};

use gpui_kit::Entity;

use super::{Check, PATIENCE, loaded, picture_on, shown_page};
use crate::browser::BrowserView;
use crate::browser::agent::{self, Actor};
use crate::theme::UseLightTheme;
use crate::visual::{Drawn, Result};

/// The page: two buttons, their centres at [`FIRST`] and [`SECOND`].
pub(super) const AGENT: &str = "<!doctype html><title>Agent</title>\
    <body style='margin:0;font:16px -apple-system,sans-serif;color:#1d1d1f;\
    background:linear-gradient(135deg,#f6f7fb,#e3e8f4)'>\
    <h1 style='margin:0;padding:36px 40px;font-size:26px'>An agent at work</h1>\
    <button style='position:absolute;left:620px;top:350px;width:160px;height:60px;\
    font-size:16px' onclick=\"document.title='clicked first'\">First</button>\
    <button style='position:absolute;left:180px;top:150px;width:160px;height:60px;\
    font-size:16px' onclick=\"document.title='clicked second'\">Second</button>";
const FIRST: (f64, f64) = (700., 380.);
const SECOND: (f64, f64) = (260., 180.);

/// What the page shows behind it, where nothing's drawn over it.
const WHITE: [u8; 4] = [255, 255, 255, 255];

impl Check {
    pub(super) fn agent(&mut self, port: u16) -> Result<()> {
        let (shell, window) = (self.drawn.shell.clone(), self.drawn.window);
        self.drawn
            .cx
            .update(|cx| cx.dispatch_action(&UseLightTheme));
        (self.drawn.cx)
            .update_window(window, |_, window, cx| {
                shell.update(cx, |shell, cx| shell.open_browser(None, window, cx));
            })
            .map_err(|error| error.to_string())?;
        let page = (self.drawn.cx.read(|cx| shown_page(&shell, cx))).ok_or("no agent tab")?;
        self.type_text(&format!("127.0.0.1:{port}/agent"));
        self.keys("enter");
        self.until(&page, "the agent's page", |view| loaded(view, "Agent"))?;
        // Typing the address made the page the person's; they give it to the agent.
        self.drawn
            .cx
            .update(|cx| page.update(cx, BrowserView::give_back));

        let actor = Actor {
            id: "visual-check".into(),
            label: "Claude Code".into(),
        };
        self.glide(&page, &actor, FIRST, "agent-on-its-way.png")?;
        self.click(&page, &actor, FIRST, "clicked first", "agent-clicked.png")?;
        self.glide(&page, &actor, SECOND, "agent-second-arc.png")?;
        self.click(
            &page,
            &actor,
            SECOND,
            "clicked second",
            "agent-clicked-again.png",
        )?;
        // Six seconds after its last click, and the fade.
        self.wait(Duration::from_millis(7_200))?;
        self.shoot(&page, "agent-pointer-faded.png")?;
        self.drawn.cx.update(|cx| {
            page.update(cx, |view, cx| view.agent_releases(&actor, cx));
        });
        self.wait(Duration::from_millis(700))?;
        self.shoot(&page, "agent-released.png")
    }

    /// Sends `actor`'s pointer to `to` and pictures it halfway there, then waits for it to
    /// arrive.
    fn glide(
        &mut self,
        page: &Entity<BrowserView>,
        actor: &Actor,
        to: (f64, f64),
        name: &str,
    ) -> Result<()> {
        let travel = self
            .drawn
            .cx
            .update(|cx| page.update(cx, |view, cx| view.agent_points(actor, to, cx)));
        let travel = travel.map_err(|why| format!("the agent was refused: {why:?}"))?;
        if travel.is_zero() {
            return Err("the pointer didn't travel".into());
        }
        let started = Instant::now();
        self.wait(travel / 2)?;
        self.shoot(page, name)?;
        self.wait((started + travel).saturating_duration_since(Instant::now()))
    }

    /// Clicks `at` for `actor` as `browser_click` does once its pointer is there, and
    /// pictures the ring the click sends out once the page says `title`.
    fn click(
        &mut self,
        page: &Entity<BrowserView>,
        actor: &Actor,
        (x, y): (f64, f64),
        title: &str,
        name: &str,
    ) -> Result<()> {
        for kind in ["mousePressed", "mouseReleased"] {
            let event = serde_json::json!({
                "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1,
            });
            let sent = (self.drawn.cx)
                .update(|cx| agent::cdp(page, Some(actor), "Input.dispatchMouseEvent", event, cx));
            // The title says whether it landed; the answer adds nothing.
            let _answer = sent.map_err(|why| format!("the click wasn't sent: {why:?}"))?;
        }
        self.drawn
            .cx
            .update(|cx| page.update(cx, BrowserView::agent_clicked));
        let (wanted, shown) = (title.to_owned(), page.clone());
        (self.drawn)
            .until(PATIENCE, move |drawn| {
                Ok(drawn.cx.read(|cx| shown.read(cx).page.title == wanted))
            })
            .map_err(|why| format!("the click landing on its button: {why}"))?;
        self.shoot(page, name)
    }

    /// Draws for `wait`, in the clock's own time, each frame delivered as the platform would:
    /// the marks ask for one for every step they move.
    fn wait(&mut self, wait: Duration) -> Result<()> {
        let until = Instant::now() + wait;
        (self.drawn).until(PATIENCE, move |drawn| {
            next_frame(drawn)?;
            Ok(Instant::now() >= until)
        })
    }

    /// Saves the window now, the page laid in where it shows white.
    fn shoot(&mut self, page: &Entity<BrowserView>, name: &str) -> Result<()> {
        next_frame(&mut self.drawn)?;
        let path = self.out.join(name);
        (picture_on(&mut self.drawn, &[page], Some(WHITE))?)
            .save(&path)
            .map_err(|error| error.to_string())?;
        self.lines.push(format!("saved {}", path.display()));
        Ok(())
    }
}

/// Delivers the frame the window's views asked for, which the check has no loop to.
fn next_frame(drawn: &mut Drawn) -> Result<()> {
    let window = drawn.window;
    (drawn.cx)
        .update_window(window, |_, window, cx| window.simulate_next_frame(cx))
        .map(drop)
        .map_err(|error| error.to_string())
}
