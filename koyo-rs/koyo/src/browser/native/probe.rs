//! A harness for trying native pages out, in a debug build: commands read from a file in the
//! folder `KOYO_NATIVE_PROBE` names, run in the app on its own window, with what each found
//! written back. Pictures are of the app's own window, taken in the process, which macOS
//! allows without the screen-recording permission; clicks and keys are `NSEvent`s made here and
//! handed to the window as `AppKit` would hand it the system's, so none is injected into the
//! system and the person's pointer and keyboard stay theirs.
//!
//! ```text
//! <folder>/cmd     one command a line, taken (and removed) once it's there
//! <folder>/out     what each command found, appended
//!
//! shot <name> [number]     the window (or the app's window of that number) as <folder>/<name>.png
//! front                    the window in front of every other, the app staying behind
//! windows                  the app's windows
//! activate                 the app in front and the window key
//! escape                   an Escape in the app's event queue, which closes a native menu
//! draw                     GPUI lays out and paints now, though nothing shows the window
//! quit                     the app quits, as from its menu
//! pages                    the native pages and where their views are
//! click <x> <y> [number]   a press and release at a point of the window (or the app's window
//!                          of that number), from its top left
//! rclick <x> <y>           a right press and release there
//! rdown <x> <y>            a right press there and no release, as while a menu tracks it
//! move <x> <y>             the pointer moved there
//! hover <x> <y>            the pointer's move handed to the view there and to GPUI's, as their
//!                          tracking areas would be: a window nothing shows gets none of its own
//! scroll <dy>              a wheel turned by dy pixels, where the pointer is (a wheel event
//!                          made here has the pointer's place, not one of its own)
//! type <text>              keys for the text, to the window's first responder
//! key <code> <chars> [cmd|shift|ctrl|alt…]   a key down and up, as a key equivalent first
//! press <code> [cmd|shift|ctrl|alt…]   a key down and up as a keyboard's: an event of the
//!                          system's making, with the flags and characters the system gives
//!                          that key, put in the app's own queue, so `AppKit` handles it as
//!                          it does a key pressed (its pass of key equivalents, the text
//!                          system); nothing is posted to the system
//! seam                     what the native layer keeps (`koyo-seam`): whose the window's last
//!                          press or key was and how long ago, each page's last input through
//!                          its protocol, and the processes known to post inputs
//! menus                    the menu bar, never opened: each menu's title and its items' names,
//!                          `+key` after one with a key of its own
//! key                      the window made key now, the app left behind (the agent desktop)
//! keyboard                 each window's keyboard holder, first responder, key; app in front
//! bounds <name>            where a named part was drawn (`crate::marks`): chat-composer,
//!                          browser-address, page-<pane>, tab-switcher
//! state                    the window's first responder and whether it's key
//! cursor                   the cursor the app has set, and where the window is on screen
//! tick                     windows nothing shows drawn all the same, ten times a second (again: off)
//! views                    the window's views, by class
//! layers                   the layers under GPUI's view, by class, and what each holds
//! frame <x> <y> <w> <h>    the window's frame set, in screen points from the bottom left
//! wait <ms>                nothing for a while
//! clear-data               browsing data cleared, as from Settings
//! eval <pane> <script>     a script evaluated in a page, the developer tools' own too
//! drag <ms> <x> <y> <x> <y> …           a left drag through the points, a pause a move
//! burst <keys|wheels> <n> <gap ms> [x y]   n inputs, each stamped as it's handed to the window
//! ```

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{App, AsyncApp};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{MainThreadMarker, class, msg_send};
use objc2_app_kit::{NSApplication, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

mod keys;
mod look;
use look::{draw, quit, state, views, windows};

use crate::browser::view::sight::UNSEEN_COUNTS;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    static CGRectNull: CGRect;
    fn CGWindowListCreateImage(bounds: CGRect, option: u32, window: u32, image: u32) -> *mut c_void;
    fn CGImageRelease(image: *mut c_void);
    fn CGEventCreateScrollWheelEvent2(
        source: *const c_void,
        units: u32,
        wheels: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> *mut c_void;
    fn CGEventSetIntegerValueField(event: *mut c_void, field: u32, value: i64);
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageDestinationCreateWithURL(
        url: *const c_void,
        kind: *const c_void,
        count: usize,
        options: *const c_void,
    ) -> *mut c_void;
    fn CGImageDestinationAddImage(destination: *mut c_void, image: *mut c_void, properties: *const c_void);
    fn CGImageDestinationFinalize(destination: *mut c_void) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: *const c_void);
}

/// Starts taking commands, where `KOYO_NATIVE_PROBE` names a folder for them.
pub fn start(cx: &mut App) {
    let Some(folder) = std::env::var_os("KOYO_NATIVE_PROBE").map(PathBuf::from) else {
        return;
    };
    log::info!("native pages: taking commands from {}", folder.display());
    cx.spawn(async move |cx: &mut AsyncApp| {
        let (cmd, out) = (folder.join("cmd"), folder.join("out"));
        loop {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            if UNSEEN_COUNTS.load(std::sync::atomic::Ordering::Relaxed) {
                look::draw_unseen();
            }
            let Ok(text) = std::fs::read_to_string(&cmd) else {
                continue;
            };
            let _ = std::fs::remove_file(&cmd);
            for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                let mut words = line.splitn(2, ' ');
                let (verb, rest) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
                let found = if verb == "wait" {
                    let ms = rest.parse().unwrap_or(100);
                    cx.background_executor().timer(Duration::from_millis(ms)).await;
                    "waited".to_owned()
                } else if verb == "clear-data" {
                    // What Settings → Browser → Clear browsing data does, once confirmed.
                    cx.update(crate::browser::clear_data);
                    "clearing".to_owned()
                } else if verb == "drag" {
                    look::drag(rest, cx).await
                } else if verb == "eval" {
                    look::eval(rest, cx).await
                } else if verb == "burst" {
                    keys::burst(rest, &folder, cx).await
                } else {
                    run(verb, rest, &folder)
                };
                append(&out, &format!("{line} => {found}\n"));
                cx.background_executor().timer(Duration::from_millis(60)).await;
            }
            append(&out, "done\n");
        }
    })
    .detach();
}

fn append(path: &Path, text: &str) {
    use std::io::Write as _;
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(text.as_bytes());
    }
}

/// Koyo's own window: the first of GPUI's the app has on screen.
fn window() -> Option<Retained<NSWindow>> {
    let main = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(main);
    app.windows().iter().find(|window| {
        let name = class_name(window);
        name.starts_with("GPUI") && window.isVisible()
    })
}

fn class_name(object: &AnyObject) -> String {
    koyo_browser_host::native::class_of(object).name().to_string_lossy().into_owned()
}

fn run(verb: &str, rest: &str, folder: &Path) -> String {
    let Some(window) = window() else {
        return "no window".into();
    };
    let numbers: Vec<f64> = rest.split(' ').filter_map(|word| word.parse().ok()).collect();
    match (verb, numbers.as_slice()) {
        ("shot", _) => {
            // The window named by its number, as a dialog's; else the app's own.
            let (name, number) = rest.split_once(' ').unwrap_or((rest, ""));
            let number = number.parse().ok().or_else(|| u32::try_from(window.windowNumber()).ok());
            match number {
                Some(number) => shot(number, &folder.join(format!("{name}.png"))),
                None => "no window number".into(),
            }
        }
        ("front", _) => {
            window.orderFrontRegardless();
            "in front".into()
        }
        ("windows", _) => windows(),
        ("activate", _) => activate(&window),
        ("escape", _) => escape(&window),
        ("draw", _) => draw(&window),
        ("quit", _) => quit(),
        ("pages", _) => koyo_browser_host::native::describe(),
        // In another window of the app's, by its number: a popover's, a dialog's.
        ("click", [x, y, number]) => match look::numbered(*number as isize) {
            Some(other) => {
                let at = point(&other, *x, *y);
                mouse(&other, 5, at, 0);
                mouse(&other, 1, at, 1);
                mouse(&other, 2, at, 1);
                "clicked there".into()
            }
            None => "no such window".into(),
        },
        // The pointer moved there, then a button down and up: the left's (1, 2), the right's (3, 4).
        ("click" | "rclick", [x, y]) => {
            let (at, down) = (point(&window, *x, *y), if verb == "click" { 1 } else { 3 });
            for (kind, clicks) in [(5, 0), (down, 1), (down + 1, 1)] {
                mouse(&window, kind, at, clicks);
            }
            "clicked".into()
        }
        // Down and not up, as a hand's is while the menu the press opens tracks it.
        ("rdown", [x, y]) => {
            mouse(&window, 3, point(&window, *x, *y), 1);
            "right down".into()
        }
        ("hover", [x, y]) => look::hover(&window, mouse_event(&window, 5, point(&window, *x, *y), 0)),
        ("move", [x, y]) => {
            mouse(&window, 5, point(&window, *x, *y), 0);
            "moved".into()
        }
        ("scroll", [.., dy]) => scroll(&window, *dy as i32),
        ("type", _) => {
            for letter in rest.chars() {
                let text = letter.to_string();
                key(&window, 0, &text, 0, false);
            }
            "typed".into()
        }
        ("press", _) => keys::press(&window, rest),
        ("seam", _) => keys::seam(&window),
        ("menus", _) => keys::menus(),
        ("keyboard", _) => keys::keyboard(),
        ("bounds", _) => keys::bounds(rest),
        ("key", []) if rest.is_empty() => keys::make_key(&window),
        ("key", _) => {
            let mut words = rest.split(' ');
            let code = words.next().and_then(|w| w.parse::<u16>().ok()).unwrap_or(0);
            let chars = words.next().unwrap_or("");
            let mut flags = 0usize;
            for word in words {
                flags |= match word {
                    "shift" => 1 << 17,
                    "ctrl" => 1 << 18,
                    "alt" => 1 << 19,
                    "cmd" => 1 << 20,
                    _ => 0,
                };
            }
            key(&window, code, chars, flags, true)
        }
        ("state", _) => state(&window),
        ("cursor", _) => look::cursor(&window),
        ("layers", _) => look::layers(&window),
        ("tick", _) => {
            let on = !UNSEEN_COUNTS.fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
            format!("ticking {on}")
        }
        ("views", _) => {
            let mut text = String::new();
            if let Some(content) = window.contentView() {
                views(&content, 0, &mut text);
            }
            text
        }
        ("frame", [x, y, w, h]) => {
            window.setFrame_display(NSRect::new(NSPoint::new(*x, *y), NSSize::new(*w, *h)), true);
            "framed".into()
        }
        _ => "unknown".into(),
    }
}

/// The app in front and the window key, as when the person works in it.
fn activate(window: &NSWindow) -> String {
    if let Some(main) = MainThreadMarker::new() {
        #[expect(deprecated, reason = "the one call that brings an app in front of another")]
        NSApplication::sharedApplication(main).activateIgnoringOtherApps(true);
    }
    window.makeKeyAndOrderFront(None);
    "active".into()
}

/// An Escape put in the app's own event queue, which a menu being tracked reads: it closes a
/// native menu, which takes no event handed to the window.
fn escape(window: &NSWindow) -> String {
    let text = NSString::from_str("\u{1b}");
    // SAFETY: NSEvent's own constructor and NSApplication's own queue.
    unsafe {
        let event: *mut AnyObject = msg_send![
            class!(NSEvent),
            keyEventWithType: 10usize,
            location: NSPoint::new(0., 0.),
            modifierFlags: 0usize,
            timestamp: uptime(),
            windowNumber: window.windowNumber(),
            context: std::ptr::null::<AnyObject>(),
            characters: &*text,
            charactersIgnoringModifiers: &*text,
            isARepeat: Bool::NO,
            keyCode: 53u16
        ];
        if let (Some(main), false) = (MainThreadMarker::new(), event.is_null()) {
            koyo_seam::heard::probe_queued(1);
            let _: () = msg_send![
                &*NSApplication::sharedApplication(main),
                postEvent: event,
                atStart: Bool::YES
            ];
        }
    }
    "escape posted".into()
}

/// A point of the window given from its top left, as `AppKit` has it: from the bottom left.
fn point(window: &NSWindow, x: f64, y: f64) -> NSPoint {
    let height = window.contentView().map_or(0., |view| view.frame().size.height);
    NSPoint::new(x, height - y)
}

fn uptime() -> f64 {
    // SAFETY: NSProcessInfo's own.
    unsafe {
        let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        msg_send![info, systemUptime]
    }
}

fn mouse(window: &NSWindow, kind: usize, at: NSPoint, clicks: isize) {
    let event = mouse_event(window, kind, at, clicks);
    if !event.is_null() {
        // A press of the left or right button, going down: the harness's own.
        if matches!(kind, 1 | 3) {
            koyo_seam::heard::probe_sends(window.windowNumber(), koyo_seam::input::Kind::Press);
        }
        // SAFETY: an event of this window's, handed to it as the app would.
        unsafe {
            let _: () = msg_send![window, sendEvent: event];
        }
    }
}

fn mouse_event(window: &NSWindow, kind: usize, at: NSPoint, clicks: isize) -> *mut AnyObject {
    // SAFETY: NSEvent's own constructor, its arguments as it declares them.
    unsafe {
        msg_send![
            class!(NSEvent),
            mouseEventWithType: kind,
            location: at,
            modifierFlags: 0usize,
            timestamp: uptime(),
            windowNumber: window.windowNumber(),
            context: std::ptr::null::<AnyObject>(),
            eventNumber: 0isize,
            clickCount: clicks,
            pressure: if clicks > 0 { 1.0f32 } else { 0.0f32 }
        ]
    }
}

fn scroll(window: &NSWindow, dy: i32) -> String {
    // SAFETY: Core Graphics makes the event; the fields set are the window it's over, which
    // `AppKit` reads back as the event's window.
    unsafe {
        let made = CGEventCreateScrollWheelEvent2(std::ptr::null(), 0, 1, dy, 0, 0);
        if made.is_null() {
            return "no scroll event".into();
        }
        let number = window.windowNumber() as i64;
        CGEventSetIntegerValueField(made, 91, number);
        CGEventSetIntegerValueField(made, 92, number);
        let event: *mut AnyObject = msg_send![class!(NSEvent), eventWithCGEvent: made];
        CFRelease(made);
        if event.is_null() {
            return "no NSEvent for the scroll".into();
        }
        let _: () = msg_send![window, sendEvent: event];
    }
    "scrolled".into()
}

/// A key down and up for `chars`. As a key equivalent first when `equivalent`, the window's
/// then the menu's, as `NSApplication` tries them before the key goes to the first responder.
fn key(window: &NSWindow, code: u16, chars: &str, flags: usize, equivalent: bool) -> String {
    let text = NSString::from_str(chars);
    let make = |kind: usize| -> *mut AnyObject {
        // SAFETY: NSEvent's own constructor, its arguments as it declares them.
        unsafe {
            msg_send![
                class!(NSEvent),
                keyEventWithType: kind,
                location: NSPoint::new(0., 0.),
                modifierFlags: flags,
                timestamp: uptime(),
                windowNumber: window.windowNumber(),
                context: std::ptr::null::<AnyObject>(),
                characters: &*text,
                charactersIgnoringModifiers: &*text,
                isARepeat: Bool::NO,
                keyCode: code
            ]
        }
    };
    let down = make(10);
    if down.is_null() {
        return "no key event".into();
    }
    let mut how = "to the first responder";
    // SAFETY: the window's and the menu's own handling of a key equivalent, then the event
    // handed to the window as the app would.
    unsafe {
        let mut taken = false;
        if equivalent {
            let by_window: Bool = msg_send![window, performKeyEquivalent: down];
            taken = by_window.as_bool();
            if taken {
                how = "taken by the window as a key equivalent";
            } else if let Some(main) = MainThreadMarker::new() {
                let app = NSApplication::sharedApplication(main);
                let menu: *mut AnyObject = msg_send![&*app, mainMenu];
                if !menu.is_null() {
                    let by_menu: Bool = msg_send![menu, performKeyEquivalent: down];
                    taken = by_menu.as_bool();
                    if taken {
                        how = "taken by the menu as a key equivalent";
                    }
                }
            }
        }
        if !taken {
            koyo_seam::heard::probe_sends(window.windowNumber(), koyo_seam::input::Kind::Key);
            let _: () = msg_send![window, sendEvent: down];
            let up = make(11);
            if !up.is_null() {
                let _: () = msg_send![window, sendEvent: up];
            }
        }
    }
    how.into()
}

/// The window as a PNG, a point a pixel, taken by the window server.
fn shot(number: u32, path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt as _;
    // SAFETY: Core Graphics' and Image I/O's own calls; each object made is released.
    unsafe {
        // Only this window (8), its frame without its shadow (1), at a point a pixel (16).
        let image = CGWindowListCreateImage(CGRectNull, 8, number, 1 | 16);
        if image.is_null() {
            return "no picture of the window".into();
        }
        let bytes = path.as_os_str().as_bytes();
        let url: *mut AnyObject = msg_send![
            class!(NSURL),
            fileURLWithPath: &*NSString::from_str(&String::from_utf8_lossy(bytes))
        ];
        let kind = NSString::from_str("public.png");
        let destination = CGImageDestinationCreateWithURL(
            url.cast(),
            Retained::as_ptr(&kind).cast(),
            1,
            std::ptr::null(),
        );
        if destination.is_null() {
            CGImageRelease(image);
            return "no file for the picture".into();
        }
        CGImageDestinationAddImage(destination, image, std::ptr::null());
        let written = CGImageDestinationFinalize(destination);
        CFRelease(destination);
        CGImageRelease(image);
        if written {
            format!("saved {}", path.display())
        } else {
            "couldn't write the picture".into()
        }
    }
}
