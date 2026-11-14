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
//! shot <name>              the window as <folder>/<name>.png, a point a pixel
//! front                    the window in front of every other, the app staying behind
//! windows                  the app's windows
//! activate                 the app in front and the window key
//! escape                   an Escape in the app's event queue, which closes a native menu
//! draw                     GPUI lays out and paints now, though nothing shows the window
//! quit                     the app quits, as from its menu
//! pages                    the native pages and where their views are
//! click <x> <y>            a press and release at a point of the window, from its top left
//! rclick <x> <y>           a right press and release there
//! move <x> <y>             the pointer moved there
//! scroll <dy>              a wheel turned by dy pixels, where the pointer is (a wheel event
//!                          made here has the pointer's place, not one of its own)
//! type <text>              keys for the text, to the window's first responder
//! key <code> <chars> [cmd|shift|ctrl|alt…]   a key down and up, as a key equivalent first
//! state                    the window's first responder and whether it's key
//! views                    the window's views, by class
//! frame <x> <y> <w> <h>    the window's frame set, in screen points from the bottom left
//! wait <ms>                nothing for a while
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

mod look;
use look::{state, views, windows};

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
                } else if verb == "burst" {
                    burst(rest, &folder, cx).await
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

/// `burst <keys|wheels> <count> <gap ms> [x y]`: inputs one after another, each stamped as
/// it's handed to the window, the stamps in `<kind>.json`.
async fn burst(rest: &str, folder: &Path, cx: &AsyncApp) -> String {
    let words: Vec<&str> = rest.split(' ').collect();
    let kind = words.first().copied().unwrap_or("keys");
    let number = |at: usize, or: f64| words.get(at).and_then(|w| w.parse::<f64>().ok()).unwrap_or(or);
    let (count, gap) = (number(1, 50.) as usize, number(2, 100.) as u64);
    let mut sent = Vec::with_capacity(count);
    for _ in 0..count {
        if let Some(window) = window() {
            sent.push(wall_ms());
            if kind == "wheels" {
                scroll(&window, -3);
            } else {
                key(&window, 0, "a", 0, false);
            }
        }
        cx.background_executor().timer(Duration::from_millis(gap)).await;
    }
    let stamps: Vec<String> = sent.iter().map(|at| format!("{at:.3}")).collect();
    let _ = std::fs::write(folder.join(format!("{kind}.json")), format!("[{}]", stamps.join(",")));
    format!("{} {kind} sent", sent.len())
}

/// The time by the wall clock, in milliseconds, as a page reads it too.
fn wall_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0., |since| since.as_secs_f64() * 1000.)
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
        ("shot", _) => match u32::try_from(window.windowNumber()) {
            Ok(number) => shot(number, &folder.join(format!("{rest}.png"))),
            Err(_) => "no window number".into(),
        },
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
        ("click", [x, y]) => {
            let at = point(&window, *x, *y);
            mouse(&window, 5, at, 0); // moved
            mouse(&window, 1, at, 1); // left down
            mouse(&window, 2, at, 1); // left up
            "clicked".into()
        }
        ("rclick", [x, y]) => {
            let at = point(&window, *x, *y);
            mouse(&window, 5, at, 0); // moved
            mouse(&window, 3, at, 1); // right down
            mouse(&window, 4, at, 1); // right up
            "right-clicked".into()
        }
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
            let _: () = msg_send![
                &*NSApplication::sharedApplication(main),
                postEvent: event,
                atStart: Bool::YES
            ];
        }
    }
    "escape posted".into()
}

/// Has GPUI lay out and paint though nothing shows the window, as with the display asleep:
/// what a frame does (a page placed, the keyboard moved) is done.
fn draw(window: &NSWindow) -> String {
    let layer = (window.contentView())
        .and_then(|content| koyo_browser_host::native::find_view(&content, c"GPUIView"))
        .and_then(|view| view.layer());
    match layer {
        Some(layer) => {
            layer.display();
            "drawn".into()
        }
        None => "no GPUI view".into(),
    }
}

/// Quits the app, as its menu's Quit does.
fn quit() -> String {
    if let Some(main) = MainThreadMarker::new() {
        // SAFETY: `NSApplication`'s own, as the Quit menu item sends it.
        unsafe {
            let _: () = msg_send![
                &*NSApplication::sharedApplication(main),
                terminate: std::ptr::null::<AnyObject>()
            ];
        }
    }
    "quitting".into()
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
    // SAFETY: NSEvent's own constructor, its arguments as it declares them.
    let event: *mut AnyObject = unsafe {
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
    };
    if !event.is_null() {
        // SAFETY: an event of this window's, handed to it as the app would.
        unsafe {
            let _: () = msg_send![window, sendEvent: event];
        }
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
