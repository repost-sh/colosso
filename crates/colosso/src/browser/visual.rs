//! A browser tab driven as a person drives it, on the real engine, drawn as the app draws it
//! and saved as PNGs in `.superpowers/qa/browser/`: the start page, an address typed and a page
//! loading, a link clicked and history walked back, find, zoom, `DevTools` at each of their
//! places, a second tab typed into, scrolled and split beside the first, and the Browser
//! settings, in the light theme and the dark. A page shows in a native layer of its own, which
//! the window's picture lacks, so each picture has the pages' frames laid in where they show,
//! under what the window draws over them.
//!
//! ```sh
//! COLOSSO_CONFIG_DIR=$PWD/.superpowers/qa/browser/config \
//! COLOSSO_DATA_DIR=$PWD/.superpowers/qa/browser/data \
//!   cargo run -p colosso --features visual-check -- --visual-check-browser
//! ```

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use gpui_kit::{App, Entity, Modifiers, Pixels, Point, point, px};
use image::RgbaImage;
use objc2_io_surface::IOSurfaceLockOptions;

use super::frames::Frame;
use super::host::Browsers;
use super::{BrowserView, Dock};
use crate::navigation::Route;
use crate::settings_view::SettingsPage;
use crate::shell::Shell;
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result};

mod play;

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
    let port = serve()?;
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
    /// Saves the window as `name`, with `pages`' frames laid in.
    fn save(&mut self, name: &str, pages: &[&Entity<BrowserView>]) -> Result<()> {
        let path = self.out.join(name);
        // What arrives just after the step, a late image or the paint for it, shows too.
        let settled = Instant::now() + SETTLE;
        (self.drawn).until(PATIENCE, move |_| Ok(Instant::now() >= settled))?;
        (picture(&mut self.drawn, pages)?)
            .save(&path)
            .map_err(|error| error.to_string())?;
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
        self.save("start-page.png", &[&page])?;
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
        self.save("loading.png", &[page])?;
        self.until(page, "the first page", |view| loaded(view, "First page"))?;
        self.save("loaded.png", &[page])?;

        let window = self.drawn.window;
        let origin = self.drawn.cx.read(|cx| page.read(cx).origin);
        let link = origin + point(px(LINK.0), px(LINK.1));
        (self.drawn.cx).simulate_mouse_move(window, link, None, Modifiers::default());
        (self.drawn.cx).simulate_click(window, link, Modifiers::default());
        self.until(page, "the link followed", |view| {
            loaded(view, "Second page")
        })?;
        self.save("link-followed.png", &[page])?;
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
        self.save("find.png", &[page])?;
        self.keys("escape cmd-=");
        self.until(page, "zoomed in", |view| {
            view.zoom != super::find::ACTUAL_SIZE
        })?;
        self.save("zoom.png", &[page])?;
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
            self.save(&name, &[page, &tools])?;
            if at == 0 {
                self.drag_divider_over(page)?;
            }
        }
        Ok(())
    }

    /// A press on the page, then on the find bar's field over it: the field takes the keys,
    /// though the page's native layer lies under the bar.
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
        self.save("settings-dark.png", &[])?;
        self.drawn
            .cx
            .update(|cx| cx.dispatch_action(&UseLightTheme));
        self.save("settings.png", &[])
    }
}

/// The page of the shown tab's first pane, a browser one.
fn shown_page(shell: &Entity<Shell>, cx: &App) -> Option<Entity<BrowserView>> {
    let unit = shell.read(cx).workspace.branch()?;
    let tab = unit.tabs.get(unit.active_tab)?;
    tab.panes().first()?.browser.clone()
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

/// The window drawn, with each of `pages`' last frames laid where the page shows.
fn picture(drawn: &mut Drawn, pages: &[&Entity<BrowserView>]) -> Result<RgbaImage> {
    drawn.draw()?;
    let window = drawn.window;
    let mut image = (drawn.cx.capture_screenshot(window)).map_err(|error| error.to_string())?;
    let wide = (drawn.cx)
        .update_window(window, |_, window, _| window.viewport_size().width)
        .map_err(|error| error.to_string())?;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a picture's width is far below 2^24"
    )]
    let scale = image.width() as f32 / wide.as_f32();
    for page in pages {
        let shown = drawn.cx.read(|cx| {
            let view = page.read(cx);
            view.frame.clone().map(|frame| (frame, view.origin))
        });
        if let Some((frame, origin)) = shown {
            lay(&mut image, &frame, origin, scale);
        }
    }
    Ok(image)
}

/// Copies `frame`'s page into `image` at `origin`, in the image's pixels, wherever the
/// window shows the page's backdrop: what the window draws over the page, among its overlays,
/// stays over it, as on screen.
fn lay(image: &mut RgbaImage, frame: &Frame, origin: Point<Pixels>, scale: f32) {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a page's place in the window, which is never left of or above it"
    )]
    let (left, top) = (
        (origin.x.as_f32() * scale).round().max(0.) as u32,
        (origin.y.as_f32() * scale).round().max(0.) as u32,
    );
    let (wide, tall) = (frame.size.width, frame.size.height);
    let (Some(last_x), Some(last_y)) = (
        (left + wide.min(image.width().saturating_sub(left))).checked_sub(1),
        (top + tall.min(image.height().saturating_sub(top))).checked_sub(1),
    ) else {
        return;
    };
    // The page's bottom left corner, where nothing's drawn over it.
    let backdrop = image.get_pixel(left.min(last_x), last_y).0;
    let surface = &frame.surface;
    let row = surface.bytes_per_row();
    // SAFETY: the surface is locked read-only while its rows are read, within the page's
    // part of it.
    unsafe {
        surface.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        let base = surface.base_address().as_ptr().cast::<u8>().cast_const();
        for y in 0..frame.size.height.min(image.height().saturating_sub(top)) {
            for x in 0..frame.size.width.min(image.width().saturating_sub(left)) {
                let at = base.add(y as usize * row + x as usize * 4);
                let [b, g, r, a] = [*at, *at.add(1), *at.add(2), *at.add(3)];
                let shown = image.get_pixel_mut(left + x, top + y);
                if shown
                    .0
                    .iter()
                    .zip(backdrop)
                    .all(|(&was, of)| was.abs_diff(of) <= 2)
                {
                    shown.0 = [r, g, b, a];
                }
            }
        }
        surface.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
    }
}

/// Serves the check's pages on a local port: a first one whose picture comes late, with a
/// link to a second.
fn serve() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || answer(stream));
        }
    });
    Ok(port)
}

const FIRST: &str = "<!doctype html><title>First page</title><style>\
    body{font:16px -apple-system,sans-serif;margin:0;padding:32px 40px;color:#1d1d1f}\
    h1{font-size:28px;margin:0 0 12px}p{max-width:620px;line-height:1.5}\
    a{position:absolute;left:40px;top:220px;font-size:20px}\
    input{position:absolute;left:40px;top:270px;font-size:16px;padding:6px 10px;width:280px}\
    img{position:absolute;left:40px;top:330px}</style>\
    <h1>A page served for the browser check</h1>\
    <p>This pane runs Chromium in an engine of its own. Find has a few matches here: \
    browser, browser and browser.</p><a href=/second>Go to the second page</a>\
    <input placeholder='Type here'><img src=/late.svg width=320 height=120>";

const SECOND: &str = "<!doctype html><title>Second page</title>\
    <body style='font:16px -apple-system,sans-serif;padding:32px 40px'>\
    <h1>The second page</h1><p>Reached by a click on the first page's link.</p>";

const LATE: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='320' height='120'>\
    <rect width='320' height='120' rx='14' fill='#5e5ce6'/></svg>";

fn answer(mut stream: TcpStream) {
    let mut head = Vec::new();
    let mut chunk = [0u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&head);
    let (kind, body) = match head.split_whitespace().nth(1).unwrap_or("/") {
        "/first" => ("text/html", FIRST),
        "/second" => ("text/html", SECOND),
        "/play" => ("text/html", play::PLAY),
        "/late.svg" => {
            late(Duration::from_millis(2500));
            ("image/svg+xml", LATE)
        }
        _ => ("text/plain", ""),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Holds the answer back, so the page is caught loading.
#[expect(
    clippy::disallowed_methods,
    reason = "a server thread of the check's own, late on purpose"
)]
fn late(by: Duration) {
    std::thread::sleep(by);
}
