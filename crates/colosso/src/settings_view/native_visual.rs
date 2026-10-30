//! The General settings' sliders and switches as macOS draws them, in the real app: the window
//! on screen with `AppKit`'s run loop running, so the native controls are composited, and
//! captured with `screencapture` into `.superpowers/qa/native-controls/`. On the way it moves a
//! slider and flips a switch as `AppKit` does (their actions sent in the process; no input is
//! injected), turns the repeat off, scrolls the page and resizes the window, and counts the
//! window's views before the page shows, while it does, and after it closed, twice.
//!
//! ```sh
//! Q=$PWD/.superpowers/qa/native-controls
//! COLOSSO_CONFIG_DIR=$Q/config COLOSSO_DATA_DIR=$Q/data COLOSSO_LOG_DIR=$Q/logs TMPDIR=$Q/tmp \
//!   cargo run -p colosso --features visual-check -- --visual-check-native-controls
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use gpui_kit::base::Root;
use gpui_kit::{
    Action, AnyWindowHandle, AppContext as _, AsyncApp, Bounds, Entity, MouseMoveEvent,
    PlatformInput, ScrollDelta, ScrollEvent, Styled as _, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, point, px, size, transparent_black,
};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSControl, NSControlStateValueOff, NSControlStateValueOn, NSSlider, NSSwitch,
    NSView, NSWindow,
};

use colosso_lsp::servers::{BASEDPYRIGHT, RUST_ANALYZER, VTSLS};

use crate::key_repeat::KeyRepeat;
use crate::navigation::Route;
use crate::settings;
use crate::settings_view::SettingsPage;
use crate::shell::Shell;
use crate::theme::{self, UseDarkTheme, UseLightTheme};
use crate::visual::Result;

const OUT: &str = ".superpowers/qa/native-controls";
/// Long enough for a change to be drawn, and composited, a few times over.
const SETTLE: Duration = Duration::from_millis(700);

/// Opens the app, runs the check in it and leaves with what it found.
pub fn run() -> ExitCode {
    let out = match crate::visual::out_dir(OUT) {
        Ok(out) => out,
        Err(why) => return finish(Err(why)),
    };
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            crate::init(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(80.), px(80.)),
                    size(px(1100.), px(620.)),
                ))),
                // In front: a window under others gets no frames, and would show none of what
                // the check changes.
                focus: true,
                window_background: WindowBackgroundAppearance::Blurred,
                app_owns_titlebar_drag: true,
                titlebar: Some(TitlebarOptions {
                    title: Some("Colosso native controls check".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                ..WindowOptions::default()
            };
            let mut made = None;
            let opened = cx.open_window(options, |window, cx| {
                theme::follow_window_appearance(window);
                let shell = cx.new(|cx| Shell::new(window, cx));
                made = Some(shell.clone());
                cx.new(|cx| Root::new(shell, window, cx).bg(transparent_black()))
            });
            let (Ok(window), Some(shell)) = (opened, made) else {
                finish(Err("the window didn't open".into()));
                return;
            };
            // In front, app and all: a window under others gets no frames.
            cx.activate(true);
            cx.spawn(async move |cx| {
                let found = check(window.into(), &shell, &out, cx).await;
                finish(found);
            })
            .detach();
        });
    ExitCode::SUCCESS
}

/// Says what the check found and leaves: `AppKit`'s run loop doesn't return.
fn finish(found: Result<Vec<String>>) -> ExitCode {
    let code = i32::from(!crate::visual::report(found));
    #[expect(
        clippy::disallowed_methods,
        reason = "`AppKit`'s run loop never returns to `main`, and its `terminate:` exits with 0: \
                  a failed check says so in its exit code only by leaving itself"
    )]
    std::process::exit(code);
}

async fn check(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    out: &std::path::Path,
    cx: &mut AsyncApp,
) -> Result<Vec<String>> {
    let main = MainThreadMarker::new().ok_or("not on the main thread")?;
    let mut lines = Vec::new();
    settle(cx).await;
    let ns_window = ns_window(main)?;
    lines.push(views(&ns_window, "workspace").0);

    go(shell, Route::Settings(SettingsPage::General), cx);
    cx.update(|cx| {
        cx.dispatch_action(&UseLightTheme);
        settings::update(cx, |settings| settings.key_repeat = KeyRepeat::default());
    });
    settle(cx).await;
    lines.push(views(&ns_window, "General shown").0);
    lines.extend(flip(&ns_window, cx).await?);
    lines.push(shoot(&ns_window, out, "top-light.png", cx).await?);
    scroll(window, 4000., cx)?;
    settle(cx).await;
    lines.push(shoot(&ns_window, out, "bottom-light.png", cx).await?);

    // As AppKit does once the thumb is dragged to the eighth stop.
    let speed = slider(&ns_window, 10)?;
    speed.setDoubleValue(7.);
    act(&speed);
    settle(cx).await;
    let per_second = cx.update(|cx| settings::get(cx).key_repeat.per_second);
    lines.push(format!("speed moved to stop 7: per_second = {per_second}"));
    if (per_second - 150.).abs() > f32::EPSILON {
        return Err(format!("the move wrote {per_second}, not 150"));
    }

    let themes: [(&dyn Action, &str); 2] = [(&UseLightTheme, "light"), (&UseDarkTheme, "dark")];
    for (theme, name) in themes {
        cx.update(|cx| {
            cx.dispatch_action(theme);
            settings::update(cx, |settings| settings.key_repeat.enabled = false);
        });
        settle(cx).await;
        let enabled = [10, 8].map(|stops| slider(&ns_window, stops).map(|s| s.isEnabled()));
        lines.push(format!("repeat off ({name}): sliders enabled {enabled:?}"));
        if enabled.iter().any(|enabled| *enabled != Ok(false)) {
            return fail("a slider stayed enabled with the repeat off", &lines);
        }
        lines.push(shoot(&ns_window, out, &format!("off-{name}.png"), cx).await?);
        cx.update(|cx| settings::update(cx, |settings| settings.key_repeat.enabled = true));
        settle(cx).await;
        lines.push(shoot(&ns_window, out, &format!("bottom-{name}.png"), cx).await?);
    }

    // Up a little at a time, until the page's edge cuts the delay slider.
    let delay = slider(&ns_window, 8)?;
    for _ in 0..80 {
        scroll(window, -12., cx)?;
        cx.background_executor()
            .timer(Duration::from_millis(40))
            .await;
        let cut = |delay: &NSSlider| (visible_height(delay), delay.frame().size.height);
        let (shown, whole) = cut(&delay);
        if shown > 1. && shown < whole - 1. {
            settle(cx).await;
            let (shown, whole) = cut(&delay);
            lines.push(format!(
                "delay slider cut by the page: {shown}pt of {whole}pt shown"
            ));
            lines.push(shoot(&ns_window, out, "clipped-dark.png", cx).await?);
            break;
        }
    }
    cx.update(|cx| window.update(cx, |_, window, _| window.resize(size(px(1400.), px(820.)))))
        .map_err(|error| error.to_string())?;
    settle(cx).await;
    lines.push(shoot(&ns_window, out, "resized-dark.png", cx).await?);

    // Closed, opened again and closed again, it leaves nothing behind.
    for (route, label) in [
        (Route::Workspace, "closed"),
        (Route::Settings(SettingsPage::General), "General again"),
        (Route::Workspace, "closed again"),
    ] {
        go(shell, route, cx);
        settle(cx).await;
        let (said, controls) = views(&ns_window, label);
        lines.push(said);
        if route == Route::Workspace && controls != 0 {
            return fail("a native control outlived its page", &lines);
        }
    }
    Ok(lines)
}

async fn settle(cx: &mut AsyncApp) {
    cx.background_executor().timer(SETTLE).await;
}

fn go(shell: &Entity<Shell>, route: Route, cx: &mut AsyncApp) {
    cx.update(|cx| shell.update(cx, |shell, cx| shell.navigate(route, cx)));
}

/// Scrolls the page by `down` points, the pointer over its title rather than a slider.
fn scroll(window: AnyWindowHandle, down: f32, cx: &mut AsyncApp) -> Result<()> {
    cx.update(|cx| {
        window.update(cx, |_, window, cx| {
            let at = point(window.viewport_size().width * 0.6, px(90.));
            // Scrolls go where the pointer was last seen, not where they say.
            window.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: at,
                    ..MouseMoveEvent::default()
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::Scroll(ScrollEvent {
                    position: at,
                    delta: ScrollDelta::Pixels(point(px(0.), px(-down))),
                    ..ScrollEvent::default()
                }),
                cx,
            );
        })
    })
    .map_err(|error| error.to_string())
}

/// The check's window: the app's only one.
fn ns_window(main: MainThreadMarker) -> Result<Retained<NSWindow>> {
    let windows = NSApplication::sharedApplication(main).windows();
    (0..windows.count())
        .map(|index| windows.objectAtIndex(index))
        .find(|window| window.isVisible())
        .ok_or_else(|| "no window on screen".into())
}

/// How many views the window holds and how many are native controls, said after `label`; and
/// the controls' count.
fn views(window: &NSWindow, label: &str) -> (String, usize) {
    let mut views = 0;
    each_view(window, &mut |_| views += 1);
    let controls = controls::<NSControl>(window).len();
    let said = format!("{label}: {views} views, {controls} native controls");
    (said, controls)
}

/// The views in `window` that are `T`s, in the order the window holds them.
fn controls<T: objc2::DowncastTarget + objc2::Message>(window: &NSWindow) -> Vec<Retained<T>> {
    let mut found = Vec::new();
    each_view(window, &mut |view| {
        if let Some(control) = view.downcast_ref::<T>() {
            found.push(control.retain());
        }
    });
    found
}

/// Sends `control`'s action to its target, as `AppKit` does once it's used.
fn act(control: &NSControl) {
    // SAFETY: the control's own action to its own target.
    unsafe { control.sendAction_to(control.action(), control.target().as_deref()) };
}

/// The check failed with `why`, after what it saw so far.
fn fail<T>(why: &str, lines: &[String]) -> Result<T> {
    Err(format!("{why}:\n{}", lines.join("\n")))
}

/// The settings the page's switches turn on and off, in the switches' order.
fn switched(cx: &mut AsyncApp) -> Vec<bool> {
    cx.update(|cx| {
        let settings = settings::get(cx);
        vec![
            settings.sidebar_open_on_launch,
            settings.runs_server(RUST_ANALYZER.name, None),
            settings.runs_server(VTSLS.name, None),
            settings.runs_server(BASEDPYRIGHT.name, None),
            settings.stop_idle_language_servers,
            settings.key_repeat.enabled,
        ]
    })
}

/// Flips a switch as `AppKit` does once it's clicked, and has exactly one setting follow; then
/// sets the settings back, and has the switch follow.
async fn flip(window: &NSWindow, cx: &mut AsyncApp) -> Result<Vec<String>> {
    let switches = controls::<NSSwitch>(window);
    let saved = cx.update(|cx| settings::get(cx).clone());
    let before = switched(cx);
    let Some(switch) = switches.first().filter(|_| switches.len() == before.len()) else {
        return Err(format!(
            "{} switches on the General page, not {}",
            switches.len(),
            before.len()
        ));
    };
    let on = switch.state() != NSControlStateValueOn;
    switch.setState(if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    act(switch);
    settle(cx).await;
    let after = switched(cx);
    let changed: Vec<bool> = (before.iter().zip(&after))
        .filter(|(before, after)| before != after)
        .map(|(_, after)| *after)
        .collect();
    if changed != [on] {
        return Err(format!(
            "flipping a switch to {on} changed {before:?} to {after:?}"
        ));
    }
    let mut lines = vec![format!(
        "switch flipped to {on}: settings {before:?} -> {after:?}"
    )];
    cx.update(|cx| settings::update(cx, |settings| *settings = saved));
    settle(cx).await;
    let shows = switch.state() == NSControlStateValueOn;
    lines.push(format!("settings set back: switch on = {shows}"));
    if shows == on {
        return Err("the switch didn't follow its setting".into());
    }
    Ok(lines)
}

/// Visits each view in `window`, its content view first.
fn each_view(window: &NSWindow, visit: &mut dyn FnMut(&NSView)) {
    fn under(view: &NSView, visit: &mut dyn FnMut(&NSView)) {
        visit(view);
        let subviews = view.subviews();
        for index in 0..subviews.count() {
            under(&subviews.objectAtIndex(index), visit);
        }
    }
    if let Some(content) = window.contentView() {
        under(&content, visit);
    }
}

/// The slider over `stops` stops.
fn slider(window: &NSWindow, stops: isize) -> Result<Retained<NSSlider>> {
    (controls::<NSSlider>(window).into_iter())
        .find(|slider| slider.numberOfTickMarks() == stops)
        .ok_or_else(|| format!("no slider over {stops} stops"))
}

/// How much of the slider its container, clipped to the page, shows.
fn visible_height(slider: &NSSlider) -> f64 {
    // SAFETY: the superview of a view the window holds, read on the main thread.
    unsafe { slider.superview() }
        .filter(|container| !container.isHidden())
        .map_or(0., |c| c.frame().size.height)
}

/// Captures the window as macOS composites it, its native views included, into `out`.
async fn shoot(
    window: &NSWindow,
    out: &std::path::Path,
    name: &str,
    cx: &mut AsyncApp,
) -> Result<String> {
    let path: PathBuf = out.join(name);
    let number = window.windowNumber().to_string();
    let target = path.clone();
    let captured = cx
        .background_executor()
        .spawn(async move {
            std::process::Command::new("screencapture")
                .args(["-x", "-o", "-l", &number])
                .arg(&target)
                .status()
        })
        .await
        .map_err(|error| error.to_string())?;
    if !captured.success() {
        return Err(format!("screencapture failed for {name}: {captured}"));
    }
    Ok(format!("saved {}", path.display()))
}
