//! Lending on a real Mac (`docs/agent-desktop.md` 5): an app that runs as the person's own
//! is refused with what to ask them, taken in on their word, and given back, never quit:
//! when the agent closes it, when a host that went without a word is followed by the next,
//! and when the host ends. The person's app is played by a `TextEdit` this test starts
//! hidden, by nothing the host knows of, and hides again before it's given back, so nothing
//! of it shows where the person is: the test watches for that while the app is taken in,
//! and ends the app at the first window of it on their screen. It's skipped when a
//! `TextEdit` runs already, which may be theirs.

use std::ffi::c_void;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use koyo_desk_wire::link::{Answer, Ask, Close, RefusalCode};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::NSString;

use crate::support::{Driver, alive, nap};

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative: u32) -> *mut c_void;
}

/// The process of each window on the person's screen: on the desktop that shows.
fn owners_on_screen() -> Vec<i32> {
    /// `kCGWindowListOptionOnScreenOnly`.
    const ON_SCREEN: u32 = 1;
    // SAFETY: the window list is an array of dictionaries that this owns; each answers
    // its owner's pid as a number.
    unsafe {
        let list = CGWindowListCopyWindowInfo(ON_SCREEN, 0).cast::<AnyObject>();
        let Some(list) = Retained::from_raw(list) else {
            return Vec::new();
        };
        let count: usize = msg_send![&*list, count];
        let key = NSString::from_str("kCGWindowOwnerPID");
        (0..count)
            .map(|at| {
                let entry: *mut AnyObject = msg_send![&*list, objectAtIndex: at];
                let owner: *mut AnyObject = msg_send![entry, objectForKey: &*key];
                if owner.is_null() {
                    0
                } else {
                    msg_send![owner, intValue]
                }
            })
            .collect()
    }
}

/// How many windows of the app `pid` are on the person's screen.
fn on_screen(pid: i32) -> usize {
    (owners_on_screen().iter())
        .filter(|owner| **owner == pid)
        .count()
}

/// The watch the lending test keeps on the person's screen sees what is there: windows,
/// each with the process it belongs to. Starts nothing and shows nothing.
#[test]
#[ignore = "needs a session at a Mac"]
fn the_watch_on_the_persons_screen_sees_the_windows_there() {
    let owners = owners_on_screen();
    assert!(owners.len() > 3, "too few windows for a screen: {owners:?}");
    // The window server's own among them are another user's: only the number is checked.
    assert!(owners.iter().all(|pid| *pid > 0), "{owners:?}");
    let first = owners[0];
    assert!(on_screen(first) > 0 && on_screen(-1) == 0);
}

fn text_edits() -> Vec<i32> {
    let found = Command::new("/usr/bin/pgrep")
        .args(["-x", "TextEdit"])
        .output()
        .unwrap();
    (String::from_utf8_lossy(&found.stdout).lines())
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// The app that stands in for the person's: ended as the test ends, however it ends.
struct StandIn(i32);

impl Drop for StandIn {
    fn drop(&mut self) {
        // SAFETY: a signal to the one process this test started.
        unsafe { libc::kill(self.0, libc::SIGKILL) };
    }
}

fn stand_in() -> StandIn {
    let started = Command::new("/usr/bin/open")
        .args(["-n", "-g", "-j", "-a", "TextEdit"])
        .status()
        .unwrap();
    assert!(started.success(), "TextEdit didn't start");
    for _ in 0..100 {
        if let [pid] = text_edits()[..] {
            let app = StandIn(pid);
            // An app of the person's has long been running when an agent asks for it:
            // this one is given the time to finish starting, hidden as it started.
            nap(Duration::from_secs(2));
            assert_eq!(
                on_screen(pid),
                0,
                "the stand-in shows on the person's screen"
            );
            return app;
        }
        nap(Duration::from_millis(50));
    }
    panic!("no TextEdit came up");
}

/// Brings the app `pid` to the front, as a click on its Dock icon does.
fn activate(pid: i32) {
    // SAFETY: a class method answering the running app of a pid, which takes
    // `activateWithOptions:`.
    unsafe {
        let app: Option<Retained<AnyObject>> = msg_send![
            class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid
        ];
        let app = app.expect("the stand-in went away");
        let _: bool = msg_send![&*app, activateWithOptions: 0usize];
    }
}

/// Hides the app `pid`, as the person's ⌘H would, and waits until it is.
fn hide(pid: i32) {
    for _ in 0..50 {
        // SAFETY: a class method answering the running app of a pid, which answers whether
        // it's hidden and takes `hide`.
        let hidden = unsafe {
            let app: Option<Retained<AnyObject>> = msg_send![
                class!(NSRunningApplication),
                runningApplicationWithProcessIdentifier: pid
            ];
            let app = app.expect("the stand-in went away");
            let hidden: bool = msg_send![&*app, isHidden];
            if !hidden {
                let _: bool = msg_send![&*app, hide];
            }
            hidden
        };
        if hidden {
            return;
        }
        nap(Duration::from_millis(100));
    }
    panic!("the stand-in stayed shown");
}

fn open(consent: Option<&str>) -> Ask {
    Ask::Open {
        app: "TextEdit".into(),
        document: None,
        args: Vec::new(),
        theirs: true,
        consent: consent.map(str::to_owned),
    }
}

/// Lends the stand-in as an agent comes by it: asked for, it's refused with what to ask the
/// person; asked for again with their word, it's on the agent desktop. Says its windows
/// there. While it's taken in, nothing of it may show where the person is.
fn lends(driver: &mut Driver, pid: i32, said: &str) -> usize {
    let refused = driver.ask(open(None)).unwrap_err();
    assert_eq!(refused.code, RefusalCode::NeedsConsent, "{refused:?}");
    let request = driver.begin(open(Some(said)));
    let opened = loop {
        let answer = driver.answered(request, Duration::from_millis(20));
        assert_eq!(
            on_screen(pid),
            0,
            "the lent app shows on the person's screen"
        );
        if let Some(answer) = answer {
            break answer;
        }
    };
    let Ok(Answer::Opened {
        pid: lent,
        windows,
        lent: true,
        ..
    }) = opened
    else {
        panic!("{opened:?} {:?}", driver.logs);
    };
    assert_eq!(lent, pid, "another process than the person's was taken");
    driver.until(Duration::from_secs(3), |driver| !driver.lent.is_empty());
    let told: Vec<i32> = driver.lent.iter().map(|lent| lent.pid).collect();
    assert_eq!(told, [pid], "the app wasn't told what is lent");
    windows.len()
}

fn windows_of(driver: &mut Driver, pid: i32) -> usize {
    let Ok(Answer::Windows { windows }) = driver.ask(Ask::Windows) else {
        panic!("no windows");
    };
    windows.iter().filter(|window| window.pid == pid).count()
}

#[test]
#[ignore = "needs a session at a Mac and a loader app with the grants (KOYO_DESK_HOST)"]
fn an_app_of_the_persons_is_lent_on_their_word_and_given_back_not_quit() {
    if !text_edits().is_empty() {
        println!("skipped: a TextEdit runs, which may be the person's");
        return;
    }
    let Some(mut driver) = Driver::start() else {
        println!("skipped: KOYO_DESK_HOST names no loader app");
        return;
    };
    driver.ready();
    let theirs = stand_in();
    let pid = theirs.0;

    // Words that come before any refusal said what to ask count for nothing; without their
    // word it's refused, with what to tell them and ask; nothing is taken.
    let early = driver
        .ask(open(Some("yes, before I was asked")))
        .unwrap_err();
    assert_eq!(early.code, RefusalCode::NeedsConsent, "{early:?}");
    assert!(early.message.contains("explicit consent"), "{early:?}");
    let empty = driver.ask(open(Some("  "))).unwrap_err();
    assert_eq!(empty.code, RefusalCode::NeedsConsent, "{empty:?}");
    assert!(driver.lent.is_empty() && windows_of(&mut driver, pid) == 0);

    // On their word, their own process is on the agent desktop, and on record as lent.
    let shown = lends(&mut driver, pid, "yes, bring my TextEdit over");
    assert!(shown > 0, "the lent app shows no window: {:?}", driver.logs);
    assert_eq!(windows_of(&mut driver, pid), shown);
    let record = Path::new(env!("KOYO_TARGET_DIR")).join("dt/state/apps");
    let recorded = std::fs::read_to_string(&record).unwrap();
    // The record names it, when it started, that it's lent, and the desktop it came from
    // (none: hidden, it was on no desktop).
    let lent = |line: &str| {
        let mut parts = line.split(' ');
        parts.next() == Some(pid.to_string().as_str()) && parts.nth(1) == Some("lent")
    };
    assert!(recorded.lines().any(lent), "{recorded:?}");
    assert!(
        driver
            .logs
            .iter()
            .any(|log| log.contains("yes, bring my TextEdit over")),
        "their words aren't in the log: {:?}",
        driver.logs
    );

    // Brought forward from outside, as macOS and the app itself do it, it stays lent, and
    // the front is the person's again before macOS can follow it to its desktop.
    let said = driver.logs.len();
    activate(pid);
    driver.rest(Duration::from_millis(700));
    let took = (driver.logs[said..].iter())
        .find_map(|log| {
            log.split_once("), in ")?
                .1
                .strip_suffix(" ms")?
                .parse::<u64>()
                .ok()
        })
        .unwrap_or_else(|| panic!("the front wasn't given back: {:?}", &driver.logs[said..]));
    assert!(took < 150, "the front came back after {took} ms");
    assert_eq!(
        on_screen(pid),
        0,
        "macOS followed the lent app to its desktop"
    );
    assert_eq!(driver.lent.len(), 1, "coming to the front gave it back");

    // Closed by the agent, it goes back as it is: off the agent desktop, and running long
    // after an app of the agent's would have been stopped.
    hide(pid);
    assert_eq!(driver.ask(Ask::Close(Close::App(pid))), Ok(Answer::Done));
    assert!(driver.until(Duration::from_secs(3), |driver| driver.lent.is_empty()));
    driver.rest(Duration::from_secs(4));
    assert!(alive(pid), "the person's app was quit");
    assert_eq!(windows_of(&mut driver, pid), 0);
    // The words that lent it are used up: with them alone it's refused, and said why.
    let stale = driver
        .ask(open(Some("yes, bring my TextEdit over")))
        .unwrap_err();
    assert_eq!(stale.code, RefusalCode::NeedsConsent, "{stale:?}");
    assert!(stale.message.contains("used up"), "{stale:?}");
    assert!(driver.lent.is_empty() && windows_of(&mut driver, pid) == 0);

    // A host that goes without a word leaves it lent on record: the next gives it back
    // before all else, and ends nothing of the person's.
    lends(&mut driver, pid, "yes, once more");
    hide(pid);
    driver.kill();
    let mut driver = Driver::start().unwrap();
    driver.ready();
    assert!(alive(pid), "the next host ended the person's app");
    assert!(driver.lent.is_empty() && windows_of(&mut driver, pid) == 0);

    // And the host's own end gives it back the same.
    lends(&mut driver, pid, "yes, a last time");
    hide(pid);
    driver.quit();
    assert!(alive(pid), "the host's end quit the person's app");
}
