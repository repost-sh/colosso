//! A browser pane against the real host of the engine `script/browser-engine` packages in
//! the checkout, in a test window: what the person sees and does reaches the page, and the
//! page paints at the display's rate. Ignored by default, since they need that engine:
//!
//! ```text
//! cargo test -p colosso browser::host_tests -- --ignored
//! ```

use std::io::{Read as _, Write as _};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use colosso_browser_ipc::Go;
use colosso_browser_ipc::client::Launch;
use gpui_kit::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};
use objc2_io_surface::IOSurfaceLockOptions;

use super::frames::Frame;
use super::host::{Browsers, HostStatus, TestLaunch};
use super::view::BrowserView;

mod dialogs;
mod input;
mod keeping;
mod loading;
mod looking;

/// How long a page may take to do what a test waits for.
const PATIENCE: Duration = Duration::from_secs(30);

/// A pane opening `url` in a window of its own, on a host of its own with its own
/// profiles, since two hosts can't share them. The scratch folder goes with `finish`.
fn open<'a>(
    test: &str,
    url: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<BrowserView>, &'a mut VisualTestContext, PathBuf) {
    let scratch = prepare(test, cx);
    let url = url.to_owned();
    let (view, cx) = cx.add_window_view(|window, cx| {
        BrowserView::new(url, Path::new("/work/browser-test"), window, cx)
    });
    (view, cx, scratch)
}

/// Has the test's panes start their host on the packaged engine, with profiles in a
/// scratch folder of the test's own, which it returns.
fn prepare(test: &str, cx: &mut TestAppContext) -> PathBuf {
    cx.executor().allow_parking();
    let executable = super::engine::host()
        .expect("an engine packaged in the checkout: run script/browser-engine");
    let scratch = crate::test_support::scratch(&format!("browser-{test}"));
    let launch = Launch {
        executable,
        profiles: scratch.join("profiles"),
    };
    cx.update(|cx| {
        crate::init(cx);
        cx.set_global(TestLaunch(launch));
    });
    scratch
}

/// Draws and runs the window until `done` holds for the view, or fails the test saying
/// what it waited for.
fn wait_for(
    cx: &mut VisualTestContext,
    view: &Entity<BrowserView>,
    what: &str,
    done: impl Fn(&BrowserView) -> bool,
) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        if view.read_with(cx, |view, _| done(view)) {
            return;
        }
        let (title, url) = view.read_with(cx, |view, _| {
            (view.page.title.clone(), view.page.url.clone())
        });
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; the page is {title:?} at {url}"
        );
        pause(Duration::from_millis(20));
    }
}

/// Lets real time pass, for the engine, which runs outside the test's executor.
#[expect(
    clippy::disallowed_methods,
    reason = "the engine runs in real time, outside the test's executor"
)]
fn pause(duration: Duration) {
    std::thread::sleep(duration);
}

/// Waits until the page's title is `title`.
fn wait_for_title(cx: &mut VisualTestContext, view: &Entity<BrowserView>, title: &str) {
    wait_for(cx, view, &format!("the title {title:?}"), |view| {
        view.page.title == title
    });
}

/// Stops the host, which must quit when asked, and drops the test's scratch.
fn finish(cx: &mut VisualTestContext, scratch: PathBuf) {
    let browsers = cx.update(|_, cx| Browsers::global(cx));
    if let Some(connection) = browsers.update(cx, |browsers, _| browsers.stop()) {
        let started = Instant::now();
        connection.shutdown(Duration::from_secs(5));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the host quits on request"
        );
    }
    let _ = std::fs::remove_dir_all(scratch);
}

/// BGRA at a device pixel of a frame.
fn pixel(frame: &Frame, x: usize, y: usize) -> [u8; 4] {
    let surface = &frame.surface;
    // SAFETY: the surface is locked read-only while its bytes are read, within its rows.
    unsafe {
        surface.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        let base = surface.base_address().as_ptr().cast::<u8>();
        let at = base.add(y * surface.bytes_per_row() + x * 4);
        let pixel = [*at, *at.add(1), *at.add(2), *at.add(3)];
        surface.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
        pixel
    }
}

/// BGRA at the middle of a frame's page.
fn centre(frame: &Frame) -> [u8; 4] {
    let (width, height) = (frame.size.width as usize, frame.size.height as usize);
    pixel(frame, width / 2, height / 2)
}

/// A left click at a point of the window.
fn click(cx: &mut VisualTestContext, x: f32, y: f32) {
    let at = point(px(x), px(y));
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::default());
}

/// Where the page starts in the window, under the toolbar.
fn page_origin(cx: &mut VisualTestContext, view: &Entity<BrowserView>) -> (f32, f32) {
    view.read_with(cx, |view, _| {
        (view.origin.x.as_f32(), view.origin.y.as_f32())
    })
}

/// A local server answering `GET /<name>` with what `page` gives for the name, `None` being
/// a 404.
fn serve(page: fn(&str) -> Option<String>) -> u16 {
    listen(move |head| {
        let path = head.split_whitespace().nth(1).unwrap_or("/");
        match page(path.trim_start_matches('/')) {
            Some(body) => format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
            None => {
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
            }
        }
    })
}

/// A local server sending what `respond` makes of each request's head. Each connection is
/// answered on a thread of its own, then closed.
fn listen(respond: impl Fn(&str) -> String + Copy + Send + 'static) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let port = listener.local_addr().expect("its address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            std::thread::spawn(move || answer(stream, respond));
        }
    });
    port
}

fn answer(mut stream: TcpStream, respond: impl Fn(&str) -> String) {
    // The whole head, so closing never resets the connection.
    let mut head = Vec::new();
    let mut chunk = [0u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&chunk[..n]),
        }
    }
    let response = respond(&String::from_utf8_lossy(&head));
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Write);
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn a_pane_shows_its_page_and_follows_navigation(cx: &mut TestAppContext) {
    let red = "data:text/html,<title>Red page</title><body style='margin:0;background:%23f00'>";
    let blue = "data:text/html,<title>Blue page</title><body style='margin:0;background:%2300f'>";
    let opened = Instant::now();
    let (view, cx, scratch) = open("nav", red, cx);
    wait_for(cx, &view, "the red page", |view| {
        view.page.title == "Red page" && view.frame.as_ref().is_some_and(|f| centre(f)[2] > 240)
    });
    eprintln!(
        "first page painted {:?} after the pane opened",
        opened.elapsed()
    );
    view.read_with(cx, |view, cx| {
        assert_eq!(view.status(cx), HostStatus::Running);
        let [b, g, r, _] = centre(view.frame.as_ref().unwrap());
        assert!(r > 240 && g < 16 && b < 16, "red: {r} {g} {b}");
    });

    view.update(cx, |view, cx| view.navigate(blue.into(), cx));
    wait_for(cx, &view, "the blue page", |view| {
        view.page.title == "Blue page" && view.frame.as_ref().is_some_and(|f| centre(f)[0] > 240)
    });
    view.read_with(cx, |view, _| {
        assert!(view.page.can_go_back, "the red page is behind");
        assert_eq!(view.title(), "Blue page");
    });

    view.update(cx, |view, cx| view.go(Go::Back, cx));
    wait_for_title(cx, &view, "Red page");
    view.update(cx, |view, cx| view.go(Go::Forward, cx));
    wait_for_title(cx, &view, "Blue page");
    finish(cx, scratch);
}

/// The fastest refresh rate among the displays.
fn display_rate() -> f64 {
    // SAFETY: only reads the screen list; nothing here changes AppKit's state.
    let main = unsafe { objc2::MainThreadMarker::new_unchecked() };
    objc2_app_kit::NSScreen::screens(main)
        .iter()
        .map(|screen| screen.maximumFramesPerSecond())
        .max()
        .map_or(60., |fps| fps as f64)
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn an_animating_page_paints_at_the_displays_rate(cx: &mut TestAppContext) {
    // A new colour every animation frame, so every frame paints.
    let busy = "data:text/html,<title>busy</title><body style='margin:0'><script>\
        let n=0;function f(){document.body.style.background=n++%2?'%23f00':'%2300f';\
        requestAnimationFrame(f)}requestAnimationFrame(f)</script>";
    let (view, cx, scratch) = open("rate", busy, cx);
    wait_for(cx, &view, "the busy page", |view| {
        view.page.title == "busy" && view.frame.is_some()
    });
    let count = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.frames_shown);
    let before = count(cx);
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(2000) {
        cx.run_until_parked();
        pause(Duration::from_millis(1));
    }
    let fps = (count(cx) - before) as f64 / started.elapsed().as_secs_f64();
    let rate = display_rate();
    eprintln!("{fps:.0} frames a second; the fastest display runs at {rate}");
    assert!(fps >= rate * 0.9, "{fps:.0} frames a second, wanted {rate}");
    finish(cx, scratch);
}
