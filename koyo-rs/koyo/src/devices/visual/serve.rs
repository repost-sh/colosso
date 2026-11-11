//! The device tools through the real MCP, with nothing on the screen (`docs/devices.md` 13): the
//! shell off screen with app control started as the app starts it, its MCP server on the
//! settings' port with the QA clients allowed, drawing as a window would until the stop file
//! appears or its time is up. Files in the pictures' folder stand in for the person: `snap`
//! saves the window; `person-tap` clicks the middle of the shown device's picture through the
//! window's own events; `take` and `give` do what the bar's Take control and Give back do.
//!
//! ```sh
//! KOYO_CONFIG_DIR=$PWD/.superpowers/vc KOYO_DATA_DIR=$PWD/.superpowers/vd TMPDIR=$PWD/.superpowers/vt/ \
//! KOYO_DEVICE_SET=$PWD/.superpowers/ds koyo --visual-check-devices-serve
//! .superpowers/devices/qa/mcp device_list            # the QA client, port 6301
//! touch .superpowers/devices/qa/visual/serve/snap      # a picture of the window
//! touch .superpowers/devices/qa/visual/serve/stop      # quits, its simulators shut down
//! ```

use std::process::ExitCode;
use std::time::Duration;

use koyo_app_control::Scope;

use crate::control::ApprovedClient;
use crate::visual::{Drawn, Result};

const OUT: &str = ".superpowers/devices/qa/visual/serve";
const PORT: u16 = 6301;
/// How long it serves at most, unless `KOYO_VISUAL_SERVE_MINUTES` says.
const MINUTES: u64 = 60;

pub fn run() -> ExitCode {
    crate::visual::run(serve)
}

fn client(name: &str, scopes: Vec<Scope>) -> ApprovedClient {
    ApprovedClient {
        name: name.into(),
        scopes,
        approved_at: 0,
        last_used: None,
    }
}

fn serve() -> Result<Vec<String>> {
    let mut drawn = Drawn::open_on_checkout(OUT, &[])?;
    drawn.cx.update(|cx| {
        crate::settings::update(cx, |settings| {
            let policy = &mut settings.external_control;
            policy.enabled = true;
            policy.port = PORT;
            let agent: Vec<Scope> = (Scope::ALL.into_iter()).filter(|scope| *scope != Scope::Destructive).collect();
            let mut without = agent.clone();
            without.retain(|scope| *scope != Scope::Devices);
            policy.clients = vec![
                client("qa-devices", agent),
                client("qa-readonly", vec![Scope::Read]),
                client("qa-nodevices", without),
            ];
        });
        crate::control::start(cx);
    });
    let out = crate::visual::out_dir(OUT)?;
    let _ = std::fs::remove_file(out.join("stop"));
    let minutes = (std::env::var("KOYO_VISUAL_SERVE_MINUTES").ok())
        .and_then(|minutes| minutes.parse().ok())
        .unwrap_or(MINUTES);
    let mut lines = vec![format!("serving MCP on 127.0.0.1:{PORT} for {minutes} min")];
    let mut shots = 0;
    let _ = drawn.until(Duration::from_secs(minutes * 60), |drawn| {
        if std::fs::remove_file(out.join("snap")).is_ok() {
            shots += 1;
            drawn.save(&format!("serve-{shots}.png"))?;
        }
        if std::fs::remove_file(out.join("person-tap")).is_ok() {
            person_taps(drawn)?;
        }
        for (file, take) in [("take", true), ("give", false)] {
            if std::fs::remove_file(out.join(file)).is_ok() {
                person_holds(drawn, take);
            }
        }
        Ok(out.join("stop").exists())
    });
    // As the app quits: every host told to go, each shutting its simulator down.
    drawn.cx.update(|cx| super::super::devices(cx).update(cx, super::super::Devices::stop_all));
    let _ = drawn.until(Duration::from_secs(8), |_| Ok(false));
    lines.push(format!("stopped after {shots} pictures"));
    Ok(lines)
}

/// The shown device's link, if the shown tab is a device's.
fn shown_link(drawn: &mut Drawn) -> Option<gpui_kit::Entity<super::super::Link>> {
    let shell = drawn.shell.clone();
    drawn.cx.read(|cx| shell.read(cx).shown_device_view(cx)?.read(cx).link().cloned())
}

/// The person's click in the middle of the shown device's picture, as the window hears one.
fn person_taps(drawn: &mut Drawn) -> Result<()> {
    use gpui_kit::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput, point, px};
    let shell = drawn.shell.clone();
    let placed = drawn.cx.read(|cx| {
        let view = shell.read(cx).shown_device_view(cx)?;
        let view = view.read(cx);
        let (origin, fit) = view.placed?;
        let screen = view.link()?.read(cx).screen()?;
        Some(point(
            origin.x + px(screen.size.width as f32 * fit / 2.),
            origin.y + px(screen.size.height as f32 * fit / 2.),
        ))
    });
    let Some(at) = placed else {
        return Ok(());
    };
    let window = drawn.window;
    let events = [
        PlatformInput::MouseDown(MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        }),
        PlatformInput::MouseUp(MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
        }),
    ];
    for event in events {
        (drawn.cx.update_window(window, |_, window, cx| {
            window.dispatch_event(event, cx);
        }))
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// What the bar's Take control (`take`) or Give back does.
fn person_holds(drawn: &mut Drawn, take: bool) {
    let Some(link) = shown_link(drawn) else {
        return;
    };
    drawn.cx.update(|cx| {
        link.update(cx, |link, cx| {
            if take {
                link.person_acts(crate::browser::control::Mode::Explicit, cx);
            } else {
                link.give_back(cx);
            }
        });
    });
}
