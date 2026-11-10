//! The host on a real Mac (`docs/agent-desktop.md` 12): started as Koyo starts it, it makes
//! the agent desktop, opens an app there, shows, reads and drives its window, and removes
//! everything again. Needs a session at the Mac and a loader app that holds the two grants:
//!
//! ```sh
//! KOYO_DESK_HOST="$PWD/koyo-rs/target/desk-host/Koyo Agent Desktop.app" \
//!   script/cargo test -p koyo-desk-host --test real_mac -- --ignored --nocapture
//! ```

#![cfg(target_os = "macos")]
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test fails loudly at the step that broke"
)]

mod lending;
mod steps;
mod support;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use koyo_desk_wire::link::{
    Act, Answer, Ask, Button, Close, DesktopState, Input, RefusalCode, Spot, ToHost, Window,
};
use koyo_desk_wire::{Point, Size};
use support::Driver;

/// What the steps share: the desktop, and the document's window in the agent's `TextEdit`.
struct Scene {
    size: Size,
    scale: f64,
    document: PathBuf,
    pid: i32,
    window: Window,
}

fn png_size(png: &[u8]) -> (u32, u32) {
    let reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    (reader.info().width, reader.info().height)
}

/// The value of the first text box in a snapshot's tree.
fn textbox_value(tree: &str) -> Option<String> {
    let line = tree
        .lines()
        .find(|line| line.trim_start().starts_with("- textbox"))?;
    let from = line.find("value=\"")? + 7;
    let to = line[from..]
        .rfind("\" [")
        .or_else(|| line[from..].rfind('"'))?;
    Some(line[from..from + to].to_owned())
}

fn reference(tree: &str, role: &str) -> Option<String> {
    let line = tree
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("- {role}")))?;
    let from = line.find("[ref=")? + 5;
    let to = line[from..].find(']')?;
    Some(line[from..from + to].to_owned())
}

fn snapshot(driver: &mut Driver, window: u32) -> String {
    let asked = Ask::Snapshot {
        window: Some(window),
        interactive: false,
        menus: false,
    };
    match driver.ask(asked).unwrap() {
        Answer::Snapshot { tree, .. } => tree,
        other => panic!("{other:?}"),
    }
}

fn act(driver: &mut Driver, act: Act) {
    assert_eq!(driver.ask(Ask::Act(act)), Ok(Answer::Done));
}

fn keys(driver: &mut Driver, combo: &str, window: Option<u32>) {
    let combo = combo.to_owned();
    act(driver, Act::Key { combo, window });
}

fn typed(driver: &mut Driver, text: &str, into: Option<String>) {
    let text = text.to_owned();
    act(driver, Act::Type { text, into });
}

/// An app that isn't there is said so; a document opens in an instance of the agent's own, on
/// the agent desktop.
fn opens(driver: &mut Driver, size: Size, scale: f64) -> Scene {
    let missing = Ask::Open {
        app: "No Such App 4711".into(),
        document: None,
        args: Vec::new(),
        theirs: false,
        consent: None,
    };
    assert_eq!(driver.ask(missing).unwrap_err().code, RefusalCode::NotFound);
    let document = driver.dir.join("probe.txt");
    std::fs::write(&document, "first line\n").unwrap();
    let opened = driver.ask(Ask::Open {
        app: "TextEdit".into(),
        document: Some(document.to_string_lossy().into_owned()),
        args: Vec::new(),
        theirs: false,
        consent: None,
    });
    let Ok(Answer::Opened { pid, windows, .. }) = opened else {
        panic!("{opened:?} {:?}", driver.logs);
    };
    let window = (windows.iter())
        .find(|window| window.title.contains("probe"))
        .expect("TextEdit showed no window for the document")
        .clone();
    assert_eq!(window.pid, pid);
    let Ok(Answer::Windows { windows: listed }) = driver.ask(Ask::Windows) else {
        panic!("no windows");
    };
    assert!(listed.iter().any(|listed| listed.id == window.id));
    Scene {
        size,
        scale,
        document,
        pid,
        window,
    }
}

/// The window's picture comes as surfaces of its own size once the tab shows; a screenshot of
/// it, and of the whole desktop, is a PNG of the size it says.
fn shows(driver: &mut Driver, scene: &Scene) {
    let id = scene.window.id;
    driver.tell(&ToHost::Visible { visible: true });
    // A window opens a point or two larger than it settles at: its frames are of the size
    // it has once that has stood a moment.
    let wide = (scene.window.frame.width * scene.scale).round() as usize & !1;
    driver.until(Duration::from_secs(5), |driver| {
        driver.frames_of(id).size.0 == wide
    });
    let frames = driver.frames_of(id);
    assert!(frames.count > 0, "no frame came");
    assert_eq!(frames.size.0, wide);
    // A window that doesn't change sends next to nothing more: its text cursor blinking,
    // not the sixty frames a second the capture delivers.
    driver.rest(Duration::from_millis(600));
    let still = driver.frames_of(id).count;
    driver.rest(Duration::from_secs(1));
    let more = driver.frames_of(id).count - still;
    assert!(more < 12, "{more} frames in a second of a still window");

    let shot = driver.ask(Ask::Screenshot {
        window: Some(id),
        scale: 1,
    });
    let Ok(Answer::Screenshot { png, size, .. }) = shot else {
        panic!("{shot:?}");
    };
    std::fs::write(driver.dir.join("window.png"), &png).unwrap();
    assert_eq!(png_size(&png), (size.width as u32, size.height as u32));
    let whole = Ask::Screenshot {
        window: None,
        scale: 1,
    };
    let Ok(Answer::Screenshot { png, .. }) = driver.ask(whole) else {
        panic!("no desktop screenshot");
    };
    std::fs::write(driver.dir.join("desktop.png"), &png).unwrap();
    assert_eq!(
        png_size(&png),
        (scene.size.width as u32, scene.size.height as u32)
    );
}

/// The window's tree reads, and what is typed lands in it: a click that names its window
/// (which comes to the front if the app opened another over it), a shortcut, keys.
fn types(driver: &mut Driver, scene: &Scene) {
    let id = scene.window.id;
    assert_eq!(
        textbox_value(&snapshot(driver, id)).as_deref(),
        Some("first line")
    );
    let middle = scene.window.frame.center();
    act(
        driver,
        Act::Click {
            spot: Spot::Point(middle),
            window: Some(id),
            button: Button::Left,
            count: 1,
            flags: 0,
        },
    );
    assert_eq!(driver.pointers.last(), Some(&(middle.x, middle.y, true)));
    let Ok(Answer::Windows { windows }) = driver.ask(Ask::Windows) else {
        panic!("no windows");
    };
    assert_eq!(
        windows.first().map(|front| front.id),
        Some(id),
        "{windows:?}"
    );
    keys(driver, "cmd+a", None);
    typed(driver, "Agent desktop probe\nSecond line", None);
    let waited = driver.ask(Ask::Wait {
        quiet_ms: 400,
        timeout_ms: 5000,
    });
    assert!(
        matches!(waited, Ok(Answer::Waited { still: true, .. })),
        "{waited:?}"
    );
    let tree = snapshot(driver, id);
    let value = textbox_value(&tree);
    assert_eq!(
        value.as_deref(),
        Some("Agent desktop probe Second line"),
        "{tree}"
    );
}

/// A copy pressed in the agent's app answers with what was copied, and the person's
/// clipboard holds what it held.
fn copies(driver: &mut Driver, scene: &Scene) {
    let clipboard = || {
        std::process::Command::new("/usr/bin/pbpaste")
            .output()
            .unwrap()
            .stdout
    };
    let before = clipboard();
    keys(driver, "cmd+a", Some(scene.window.id));
    let combo = "cmd+c".to_owned();
    let window = Some(scene.window.id);
    let copied = driver.ask(Ask::Act(Act::Key { combo, window }));
    let Ok(Answer::Copied { text }) = copied else {
        panic!("{copied:?}");
    };
    assert_eq!(text, "Agent desktop probe\nSecond line");
    assert_eq!(clipboard(), before, "the person's clipboard changed");
}

/// Typing goes on a few characters at a time, so the person acting in the tab stops it at
/// once: the agent is told, and only part of the text is in.
fn the_person_stops_typing(driver: &mut Driver, scene: &Scene) {
    keys(driver, "cmd+a", Some(scene.window.id));
    let text = "x".repeat(900);
    let typing = driver.begin(Ask::Act(Act::Type { text, into: None }));
    driver.rest(Duration::from_millis(900));
    let at = scene.window.frame.center();
    driver.tell(&ToHost::Input {
        input: Input::Move { at },
    });
    let acted = Instant::now();
    let answer = driver
        .answered(typing, Duration::from_secs(3))
        .expect("typing went on after the person acted");
    assert_eq!(answer.unwrap_err().code, RefusalCode::PersonActing);
    assert!(
        acted.elapsed() < Duration::from_secs(1),
        "{:?}",
        acted.elapsed()
    );
    driver.rest(Duration::from_millis(300));
    let typed = textbox_value(&snapshot(driver, scene.window.id)).unwrap_or_default();
    let count = typed.chars().filter(|typed| *typed == 'x').count();
    assert!(count > 10 && count < 900, "{count} of 900 characters typed");
}

/// By ref: the text box's whole value set through accessibility, then typed into; an act
/// where no window is, or on an element that isn't, is refused.
fn acts_by_ref(driver: &mut Driver, scene: &Scene) {
    let id = scene.window.id;
    let text = reference(&snapshot(driver, id), "textbox").expect("no ref on the text box");
    act(
        driver,
        Act::SetValue {
            element: text.clone(),
            text: "Set by ref".into(),
        },
    );
    typed(driver, " and typed", Some(text));
    driver.rest(Duration::from_millis(300));
    let tree = snapshot(driver, id);
    let value = textbox_value(&tree).unwrap_or_default();
    assert!(
        value.contains("Set by ref") && value.contains("and typed"),
        "{tree}"
    );

    let nowhere = Act::Click {
        spot: Spot::Point(Point { x: 3.0, y: 3.0 }),
        window: None,
        button: Button::Left,
        count: 1,
        flags: 0,
    };
    let refused = driver.ask(Ask::Act(nowhere)).unwrap_err();
    assert_eq!(refused.code, RefusalCode::NotFound, "{refused:?}");
    let gone = Act::SetValue {
        element: "e9999".into(),
        text: String::new(),
    };
    assert_eq!(
        driver.ask(Ask::Act(gone)).unwrap_err().code,
        RefusalCode::NotFound
    );
}

/// Saved with the app's own shortcut, the document on disk has what was typed; the app quits
/// when asked, and its windows go.
fn saves_and_quits(driver: &mut Driver, scene: &Scene) {
    keys(driver, "cmd+s", Some(scene.window.id));
    driver.rest(Duration::from_millis(800));
    let saved = std::fs::read_to_string(&scene.document).unwrap();
    assert!(saved.contains("Set by ref"), "saved: {saved:?}");
    let quit = Ask::Close(Close::App(scene.pid));
    assert_eq!(driver.ask(quit), Ok(Answer::Done));
    // What stays is the desktop's own picture, no window of an app's.
    let gone = |driver: &Driver| driver.windows.is_empty();
    driver.until(Duration::from_secs(6), gone);
    assert!(gone(driver), "windows stayed: {:?}", driver.windows);
}

#[test]
#[ignore = "needs a session at a Mac and a loader app with the grants (KOYO_DESK_HOST)"]
fn the_host_makes_a_desktop_and_drives_an_app_on_it() {
    let Some(mut driver) = Driver::start() else {
        println!("skipped: KOYO_DESK_HOST names no loader app");
        return;
    };
    assert_eq!(driver.grants, (true, true), "the loader lacks a grant");
    driver.ready();
    let Some(DesktopState::Ready { size, scale }) = driver.desktop.clone() else {
        panic!("no desktop: {:?} {:?}", driver.desktop, driver.logs);
    };
    let scene = opens(&mut driver, size, scale);
    shows(&mut driver, &scene);
    types(&mut driver, &scene);
    copies(&mut driver, &scene);
    the_person_stops_typing(&mut driver, &scene);
    acts_by_ref(&mut driver, &scene);
    steps::moves_a_window_by_its_bar(&mut driver, &scene);
    steps::drives_a_panel_another_process_draws(&mut driver, &scene);
    steps::saves_through_a_sheet_with_nobody_watching(&mut driver, &scene);
    saves_and_quits(&mut driver, &scene);
    println!("host said: {:?}", driver.logs);
    driver.quit();
}

/// A loader the person hasn't granted anything says so, makes no desktop and refuses what
/// needs one. It asks macOS for nothing on its own: no prompt comes up.
#[test]
#[ignore = "needs a session at a Mac and a loader app without the grants (KOYO_DESK_HOST)"]
fn a_host_without_its_grants_says_so_and_refuses() {
    let Some(mut driver) = Driver::start() else {
        println!("skipped: KOYO_DESK_HOST names no loader app");
        return;
    };
    assert_eq!(driver.grants, (false, false), "this loader holds a grant");
    driver.until(Duration::from_secs(10), |driver| driver.desktop.is_some());
    let Some(DesktopState::Missing { why }) = driver.desktop.clone() else {
        panic!("a desktop without Accessibility: {:?}", driver.desktop);
    };
    assert!(why.contains("Accessibility"), "{why}");
    let refused = driver.ask(Ask::Windows).unwrap_err();
    assert_eq!(refused.code, RefusalCode::NoDesktop, "{refused:?}");
    let open = Ask::Open {
        app: "TextEdit".into(),
        document: None,
        args: Vec::new(),
        theirs: false,
        consent: None,
    };
    assert_eq!(driver.ask(open).unwrap_err().code, RefusalCode::NoDesktop);
    driver.quit();
}
