//! Steps of the real-Mac test (`real_mac.rs`) for what macOS does away from the app itself:
//! a window moved by its bar, a panel another process draws, a sheet with nobody watching.

use std::time::Duration;

use koyo_desk_wire::Point;
use koyo_desk_wire::link::{Act, Button, Input, Spot, ToHost};

use crate::support::Driver;
use crate::{Scene, act, keys, snapshot, typed};

/// A window is moved by its title bar, for an agent and for the person in the tab alike, to
/// where the pointer takes it. macOS moves a dragged window after the real pointer, so the
/// events alone move nothing; and the desktop's own picture is among what the tab is told.
pub fn moves_a_window_by_its_bar(driver: &mut Driver, scene: &Scene) {
    let id = scene.window.id;
    let place = |driver: &Driver| {
        let frame = (driver.windows.iter())
            .find(|window| window.id == id)
            .unwrap()
            .frame;
        (frame.x, frame.y)
    };
    let (x, y) = place(driver);
    let bar = Point {
        x: x + scene.window.frame.width / 2.,
        y: y + 14.,
    };
    let by = |point: Point, dx: f64, dy: f64| Point {
        x: point.x + dx,
        y: point.y + dy,
    };
    let near =
        |(ax, ay): (f64, f64), (bx, by): (f64, f64)| (ax - bx).abs() <= 1. && (ay - by).abs() <= 1.;

    // An agent's drag.
    act(
        driver,
        Act::Drag {
            from: Spot::Point(bar),
            to: Spot::Point(by(bar, 60., 40.)),
            window: Some(id),
        },
    );
    driver.until(Duration::from_secs(3), |driver| {
        near(place(driver), (x + 60., y + 40.))
    });
    assert!(
        near(place(driver), (x + 60., y + 40.)),
        "{:?}",
        place(driver)
    );

    // The person's, in the tab: back to where it was.
    let held = by(bar, 60., 40.);
    let down = Input::Down {
        at: held,
        button: Button::Left,
        clicks: 1,
        flags: 0,
    };
    driver.tell(&ToHost::Input { input: down });
    for step in 1..=6 {
        let at = by(held, -10. * f64::from(step), -40. * f64::from(step) / 6.);
        let input = Input::Drag {
            at,
            button: Button::Left,
        };
        driver.tell(&ToHost::Input { input });
        driver.rest(Duration::from_millis(20));
    }
    let up = Input::Up {
        at: bar,
        button: Button::Left,
        clicks: 1,
        flags: 0,
    };
    driver.tell(&ToHost::Input { input: up });
    driver.until(Duration::from_secs(3), |driver| near(place(driver), (x, y)));
    assert!(near(place(driver), (x, y)), "{:?}", place(driver));
    // The agent is held off for a while after the person's hand: wait it out.
    driver.rest(Duration::from_millis(300));
    assert!(
        driver.backdrop.is_some(),
        "the desktop's own picture isn't among what the tab is told"
    );
}

/// An Open panel is drawn by a process of macOS's own for an app in a sandbox, as `TextEdit`
/// is: what's clicked and typed in it goes to that process, and keys that name no window go
/// to the panel while it's up.
pub fn drives_a_panel_another_process_draws(driver: &mut Driver, scene: &Scene) {
    keys(driver, "cmd+o", Some(scene.window.id));
    let panel = |driver: &Driver| {
        (driver.windows.iter())
            .find(|window| window.pid == scene.pid && window.id != scene.window.id)
            .cloned()
    };
    driver.until(Duration::from_secs(5), |driver| panel(driver).is_some());
    let opened = panel(driver).expect("no Open panel came up");
    driver.rest(Duration::from_millis(600));
    // The document's own file is in the panel's list: typing its name selects it there.
    typed(driver, "probe", None);
    driver.rest(Duration::from_millis(600));
    let tree = snapshot(driver, opened.id);
    let chosen = tree
        .lines()
        .any(|line| line.contains("probe.txt") && line.contains("[selected]"));
    assert!(chosen, "typing reached no file in the panel: {tree}");
    keys(driver, "escape", None);
    driver.until(Duration::from_secs(4), |driver| panel(driver).is_none());
    assert!(
        panel(driver).is_none(),
        "the panel stayed: {:?}",
        driver.windows
    );
}

/// A new document saved through its save sheet while no tab draws the desktop. The keys go
/// where the app moves them (the document it made, the sheet on it, the Go to Folder sheet on
/// that), and the sheet's Save button is on: for a window nothing captures it stays off.
pub fn saves_through_a_sheet_with_nobody_watching(driver: &mut Driver, scene: &Scene) {
    let wait = Duration::from_secs(5);
    driver.tell(&ToHost::Visible { visible: false });
    driver.rest(Duration::from_secs(1));
    let before = driver.windows.len();
    keys(driver, "cmd+n", Some(scene.window.id));
    driver.until(wait, |driver| driver.windows.len() == before + 1);
    typed(driver, "A new document", None);
    keys(driver, "cmd+s", None);
    let sheet = |driver: &Driver| driver.windows.iter().find(|w| w.title == "Save").cloned();
    driver.until(wait, |driver| sheet(driver).is_some());
    let save = sheet(driver).expect("no save sheet came down");
    let on = |tree: &str| {
        (tree.lines()).any(|line| line.contains("button \"Save\"") && !line.contains("[disabled]"))
    };
    let mut tree = String::new();
    for _ in 0..20 {
        tree = snapshot(driver, save.id);
        if on(&tree) {
            break;
        }
        driver.rest(Duration::from_millis(200));
    }
    assert!(on(&tree), "the sheet's Save button stayed off: {tree}");

    keys(driver, "shift+cmd+g", None);
    driver.until(wait, |driver| driver.windows.len() == before + 3);
    driver.rest(Duration::from_millis(500));
    keys(driver, "cmd+a", None);
    let folder = driver.dir.to_string_lossy().into_owned();
    typed(driver, &folder, None);
    driver.rest(Duration::from_millis(800));
    keys(driver, "return", None);
    driver.until(wait, |driver| driver.windows.len() == before + 2);
    driver.rest(Duration::from_millis(600));
    keys(driver, "cmd+a", None);
    typed(driver, "saved-by-the-test", None);
    driver.rest(Duration::from_millis(400));
    keys(driver, "return", None);
    let saved = driver.dir.join("saved-by-the-test.rtf");
    driver.until(wait, |_| saved.exists());
    let text = std::fs::read_to_string(&saved).unwrap_or_default();
    assert!(text.contains("A new document"), "saved: {text:?}");

    // The document is closed again, saved as it is: the scene is as it was.
    keys(driver, "cmd+w", None);
    driver.until(wait, |driver| driver.windows.len() == before);
    assert_eq!(driver.windows.len(), before, "{:?}", driver.windows);
    driver.tell(&ToHost::Visible { visible: true });
}
