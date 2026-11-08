//! A browser tab driven as a person drives it, on the real engine, drawn as the app draws it
//! and saved as PNGs in `.superpowers/qa/browser/`: the start page, an address typed and a page
//! loading, a link clicked and history walked back, find, zoom, `DevTools` at each of their
//! places, a second tab typed into, scrolled and split beside the first, an agent at work, the
//! sidebar folded over a dark page and back (no strip of anything else beside the page as it
//! moves), a page an agent opens at the size the person set for those (`opened`), and the
//! Browser and MCP server settings, in the light theme and the dark. A page's frame is drawn
//! in the window's own frame, so each picture is the window as it is on screen.
//!
//! ```sh
//! KOYO_CONFIG_DIR=$PWD/.superpowers/qa/browser/config \
//! KOYO_DATA_DIR=$PWD/.superpowers/qa/browser/data \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-browser
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use gpui_kit::{App, Entity, Modifiers, point, px};
use image::RgbaImage;

use super::host::Browsers;
use super::{BrowserView, Dock};
use crate::navigation::Route;
use crate::settings_view::SettingsPage;
use crate::shell::Shell;
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result};

mod agent;
mod fold;
mod opened;
mod play;
mod serve;

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/browser";
/// How long the engine may take to do what the check waits for.
const PATIENCE: Duration = Duration::from_secs(30);
/// How long a step's pictures wait for what comes just after it.
const SETTLE: Duration = Duration::from_millis(400);
/// Where the first page's link is, in the page.
const LINK: (f32, f32) = (60., 231.);

/// Draws the pictures and says where they are, or why it couldn't.
pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

fn draw() -> Result<Vec<String>> {
    let port = serve::serve()?;
    let out = crate::visual::out_dir(OUT)?;
    let drawn = Drawn::open_on_checkout(OUT, &[])?;
    let mut check = Check {
        drawn,
        out,
        lines: Vec::new(),
    };
    // Runs save where DevTools are left; each starts where a new install has them.
    (check.drawn.cx).update(|cx| {
        crate::settings::update(cx, |settings| {
            settings.devtools = super::DevtoolsPlace::default();
        });
    });
    let page = check.open_tab()?;
    check.browse(&page, port)?;
    check.find_and_zoom(&page)?;
    check.devtools(&page)?;
    check.play(&page, port)?;
    check.agent(port)?;
    check.fold(port)?;
    check.opened(port)?;
    check.settings()?;
    let browsers = check.drawn.cx.update(Browsers::global);
    let stopped = (check.drawn.cx).update(|cx| browsers.update(cx, |browsers, _| browsers.stop()));
    if let Some(connection) = stopped {
        connection.shutdown(Duration::from_secs(5));
    }
    Ok(check.lines)
}

/// The shell drawn off screen, where its pictures go, and what was saved.
struct Check {
    drawn: Drawn,
    out: PathBuf,
    lines: Vec<String>,
}

impl Check {
    /// Saves the window as `name`.
    fn save(&mut self, name: &str) -> Result<()> {
        // What arrives just after the step, a late image or the paint for it, shows too.
        let settled = Instant::now() + SETTLE;
        (self.drawn).until(PATIENCE, move |_| Ok(Instant::now() >= settled))?;
        let picture = picture(&mut self.drawn)?;
        self.keep(name, &picture)
    }

    /// Writes `picture` out as `name`, and says so.
    fn keep(&mut self, name: &str, picture: &RgbaImage) -> Result<()> {
        let path = self.out.join(name);
        picture.save(&path).map_err(|error| error.to_string())?;
        self.lines.push(format!("saved {}", path.display()));
        Ok(())
    }

    /// Draws until `test` holds for `view`, or says what it waited for and where the page
    /// was.
    fn until(
        &mut self,
        view: &Entity<BrowserView>,
        what: &str,
        test: fn(&BrowserView) -> bool,
    ) -> Result<()> {
        let page = view.clone();
        let done = move |drawn: &mut Drawn| Ok(drawn.cx.read(|cx| test(page.read(cx))));
        let Err(why) = self.drawn.until(PATIENCE, done) else {
            return Ok(());
        };
        let window = self.drawn.window;
        let focused = (self.drawn.cx)
            .update_window(window, |_, window, cx| {
                view.read(cx).focus.is_focused(window)
            })
            .ok();
        let state = self.drawn.cx.read(|cx| {
            let view = view.read(cx);
            format!(
                "the page is {:?} at {}, loading {}, focused {focused:?}",
                view.page.title, view.page.url, view.page.loading
            )
        });
        Err(format!("{what}: {why}; {state}"))
    }

    fn keys(&mut self, keys: &str) {
        let window = self.drawn.window;
        self.drawn.cx.simulate_keystrokes(window, keys);
    }

    fn type_text(&mut self, text: &str) {
        let window = self.drawn.window;
        self.drawn.cx.simulate_input(window, text);
    }

    /// A new tab on the served page at `path`, once `ready` holds for it, as the page it
    /// waits for says `what` is.
    fn open_served(
        &mut self,
        port: u16,
        path: &str,
        what: &str,
        ready: fn(&BrowserView) -> bool,
    ) -> Result<Entity<BrowserView>> {
        let (shell, window) = (self.drawn.shell.clone(), self.drawn.window);
        (self.drawn.cx)
            .update_window(window, |_, window, cx| {
                shell.update(cx, |shell, cx| shell.open_browser(None, window, cx));
            })
            .map_err(|error| error.to_string())?;
        let page = (self.drawn.cx.read(|cx| shown_page(&shell, cx)))
            .ok_or_else(|| format!("no tab for {what}"))?;
        self.type_text(&format!("127.0.0.1:{port}{path}"));
        self.keys("enter");
        self.until(&page, what, ready)?;
        Ok(page)
    }

    /// A new browser tab, in the light theme, on its start page.
    fn open_tab(&mut self) -> Result<Entity<BrowserView>> {
        let (shell, window) = (self.drawn.shell.clone(), self.drawn.window);
        self.drawn
            .cx
            .update(|cx| cx.dispatch_action(&UseLightTheme));
        (self.drawn.cx)
            .update_window(window, |_, window, cx| {
                shell.update(cx, |shell, cx| shell.open_browser(None, window, cx));
            })
            .map_err(|error| error.to_string())?;
        let page =
            (self.drawn.cx.read(|cx| shown_page(&shell, cx))).ok_or("no browser tab opened")?;
        self.save("start-page.png")?;
        Ok(page)
    }

    /// An address typed into the bar the new tab focused, a page whose picture comes late,
    /// a click on its link, then back with the keys.
    fn browse(&mut self, page: &Entity<BrowserView>, port: u16) -> Result<()> {
        self.type_text(&format!("127.0.0.1:{port}/first"));
        self.keys("enter");
        self.until(page, "the page loading", |view| {
            view.page.loading && view.page.progress > 0
        })?;
        self.save("loading.png")?;
        self.until(page, "the first page", |view| loaded(view, "First page"))?;
        self.save("loaded.png")?;

        let window = self.drawn.window;
        let origin = self.drawn.cx.read(|cx| page.read(cx).origin);
        let link = origin + point(px(LINK.0), px(LINK.1));
        (self.drawn.cx).simulate_mouse_move(window, link, None, Modifiers::default());
        (self.drawn.cx).simulate_click(window, link, Modifiers::default());
        self.until(page, "the link followed", |view| {
            loaded(view, "Second page")
        })?;
        self.save("link-followed.png")?;
        self.keys("cmd-[");
        self.until(page, "back to the first page", |view| {
            loaded(view, "First page")
        })
    }

    /// Find, then zoom, with the keys.
    fn find_and_zoom(&mut self, page: &Entity<BrowserView>) -> Result<()> {
        self.keys("cmd-f");
        self.type_text("browser");
        self.until(page, "matches found", |view| {
            view.find_tally().is_some_and(|(n, _)| n > 1)
        })?;
        self.keys("enter");
        self.until(page, "the next match", |view| {
            view.find_tally().is_some_and(|(_, at)| at == 2)
        })?;
        self.press_find_field(page)?;
        self.save("find.png")?;
        self.keys("escape cmd-=");
        self.until(page, "zoomed in", |view| {
            view.zoom != super::find::ACTUAL_SIZE
        })?;
        self.save("zoom.png")?;
        self.keys("cmd-0");
        self.until(page, "actual size", |view| {
            view.zoom == super::find::ACTUAL_SIZE
        })
    }

    /// `DevTools` at each of their places, and at the bottom in the dark theme.
    fn devtools(&mut self, page: &Entity<BrowserView>) -> Result<()> {
        let cx = &mut self.drawn.cx;
        cx.update(|cx| page.update(cx, |page, cx| page.inspect(None, cx)));
        let tools =
            (cx.read(|cx| page.read(cx).devtools().cloned())).ok_or("no DevTools opened")?;
        self.until(&tools, "DevTools painted", |view| view.frame.is_some())?;
        let docks = [
            Dock::Bottom,
            Dock::Left,
            Dock::Right,
            Dock::Detached,
            Dock::Bottom,
        ];
        for (at, dock) in docks.into_iter().enumerate() {
            let dark = at == docks.len() - 1;
            self.drawn.cx.update(|cx| {
                page.update(cx, |page, cx| page.dock_devtools(dock, cx));
                if dark {
                    cx.dispatch_action(&UseDarkTheme);
                }
            });
            // Both laid out afresh in their new room.
            let both = [page.clone(), tools.clone()];
            (self.drawn).until(PATIENCE, |drawn| {
                drawn.draw()?;
                Ok(drawn
                    .cx
                    .read(|cx| both.iter().all(|view| fits(view.read(cx)))))
            })?;
            let name = if dark {
                "devtools-dark.png".to_owned()
            } else {
                format!("devtools-{dock:?}.png").to_lowercase()
            };
            self.save(&name)?;
            if at == 0 {
                self.drag_divider_over(page)?;
            }
        }
        Ok(())
    }

    /// A press on the page, then on the find bar's field over it: the field takes the keys,
    /// though the page lies under the bar.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a page's size in pixels is far below 2^24"
    )]
    fn press_find_field(&mut self, page: &Entity<BrowserView>) -> Result<()> {
        let (window, none) = (self.drawn.window, Modifiers::default());
        let (origin, wide) = self.drawn.cx.read(|cx| {
            let view = page.read(cx);
            (
                view.origin,
                view.viewport.as_ref().map_or(0., |room| room.width as f32),
            )
        });
        (self.drawn.cx).simulate_click(window, origin + point(px(600.), px(500.)), none);
        let at = origin + point(px(wide - 12. - 340. + 80.), px(26.));
        (self.drawn.cx).simulate_click(window, at, none);
        let page = page.clone();
        (self.drawn)
            .until(PATIENCE, |drawn| {
                let window = drawn.window;
                (drawn.cx)
                    .update_window(window, |_, window, cx| {
                        page.read(cx).find_focused(window, cx)
                    })
                    .map_err(|error| error.to_string())
            })
            .map_err(|why| format!("the find field pressed over the page: {why}"))
    }

    /// The divider between the page and the `DevTools` under it, picked up just over the
    /// page and dragged up: the `DevTools` grow by as much.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a page's size in pixels is far below 2^24"
    )]
    fn drag_divider_over(&mut self, page: &Entity<BrowserView>) -> Result<()> {
        let (window, none) = (self.drawn.window, Modifiers::default());
        let tall = |cx: &App| crate::settings::get(cx).devtools.height;
        let before = self.drawn.cx.read(tall);
        let from = self.drawn.cx.read(|cx| {
            let view = page.read(cx);
            let room = view
                .viewport
                .as_ref()
                .map_or((0., 0.), |room| (room.width as f32, room.height as f32));
            view.origin + point(px(room.0 / 2.), px(room.1 - 2.))
        });
        let left = Some(gpui_kit::MouseButton::Left);
        let cx = &mut self.drawn.cx;
        cx.simulate_mouse_down(window, from, gpui_kit::MouseButton::Left, none);
        cx.simulate_mouse_move(window, from - point(px(0.), px(5.)), left, none);
        cx.simulate_mouse_move(window, from - point(px(0.), px(60.)), left, none);
        let to = from - point(px(0.), px(60.));
        cx.simulate_mouse_up(window, to, gpui_kit::MouseButton::Left, none);
        (self.drawn)
            .until(PATIENCE, |drawn| {
                Ok(drawn.cx.read(|cx| (tall(cx) - before - 60.).abs() < 4.))
            })
            .map_err(|why| {
                let after = self.drawn.cx.read(tall);
                format!(
                    "the divider dragged over the page: {why}; DevTools {before} then {after} high"
                )
            })
    }

    /// The Browser settings page, in the dark theme and the light.
    fn settings(&mut self) -> Result<()> {
        let shell = self.drawn.shell.clone();
        self.drawn.cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.navigate(Route::Settings(SettingsPage::Browser), cx);
            });
        });
        self.save("settings-dark.png")?;
        self.drawn
            .cx
            .update(|cx| cx.dispatch_action(&UseLightTheme));
        self.save("settings.png")?;
        self.mcp_settings()
    }
}

/// The page of the shown tab's first pane, a browser one.
fn shown_page(shell: &Entity<Shell>, cx: &App) -> Option<Entity<BrowserView>> {
    let unit = shell.read(cx).workspace.branch()?;
    let tab = unit.tabs.get(unit.active_tab)?;
    tab.panes().first()?.view().cloned()
}

/// Whether `view` shows the page titled `title`, loaded and painted.
fn loaded(view: &BrowserView, title: &str) -> bool {
    view.page.title == title && !view.page.loading && view.frame.is_some()
}

/// Whether `view`'s last frame was painted for the room it has now.
fn fits(view: &BrowserView) -> bool {
    let (Some(frame), Some(room)) = (&view.frame, view.viewport.as_ref()) else {
        return false;
    };
    frame.size == room.pixels()
}

/// The window as it's drawn now.
fn picture(drawn: &mut Drawn) -> Result<RgbaImage> {
    drawn.draw()?;
    let window = drawn.window;
    (drawn.cx.capture_screenshot(window)).map_err(|error| error.to_string())
}
