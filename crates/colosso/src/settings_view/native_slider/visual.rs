//! The Keyboard settings' sliders as macOS draws them, in the real app: the window on screen
//! with `AppKit`'s run loop running, so the native sliders are composited, and captured with
//! `screencapture` into `.superpowers/qa/native-slider/`. On the way it moves a slider as
//! `AppKit` does (its action sent in the process; no input is injected), turns the repeat off,
//! scrolls the page and resizes the window, and counts the window's views before the page
//! shows, while it does, and after it closed, twice.
//!
//! ```sh
//! Q=$PWD/.superpowers/qa/native-slider
//! COLOSSO_CONFIG_DIR=$Q/config COLOSSO_DATA_DIR=$Q/data COLOSSO_LOG_DIR=$Q/logs TMPDIR=$Q/tmp \
//!   cargo run -p colosso --features visual-check -- --visual-check-native-slider
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use gpui_kit::base::Root;
use gpui_kit::{
    Action, AnyWindowHandle, AppContext as _, AsyncApp, Bounds, Entity, MouseMoveEvent, PlatformInput, ScrollDelta,
    ScrollEvent, Styled as _, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, point, px, size, transparent_black,
};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, Message as _};
use objc2_app_kit::{NSApplication, NSSlider, NSView, NSWindow};

use crate::key_repeat::KeyRepeat;
use crate::navigation::Route;
use crate::settings;
use crate::settings_view::SettingsPage;
use crate::shell::Shell;
use crate::theme::{self, UseDarkTheme, UseLightTheme};
use crate::visual::Result;

const OUT: &str = ".superpowers/qa/native-slider";
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
                    title: Some("Colosso native slider check".into()),
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
    lines.push(shoot(&ns_window, out, "top-light.png", cx).await?);
    scroll(window, 4000., cx)?;
    settle(cx).await;
    lines.push(shoot(&ns_window, out, "bottom-light.png", cx).await?);

    // As AppKit does once the thumb is dragged to the eighth stop.
    let speed = slider(&ns_window, 10)?;
    speed.setDoubleValue(7.);
    // SAFETY: the slider's own action to its own target, as a drag sends it.
    unsafe { speed.sendAction_to(speed.action(), speed.target().as_deref()) };
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
            return Err("a slider stayed enabled with the repeat off".into());
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
        let (said, sliders) = views(&ns_window, label);
        lines.push(said);
        if route == Route::Workspace && sliders != 0 {
            return Err("a slider outlived its page".into());
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

/// How many views the window holds and how many are sliders, said after `label`; and the
/// sliders' count.
fn views(window: &NSWindow, label: &str) -> (String, usize) {
    let (mut views, mut sliders) = (0, 0);
    if let Some(content) = window.contentView() {
        each_view(&content, &mut |view| {
            views += 1;
            sliders += usize::from(view.downcast_ref::<NSSlider>().is_some());
        });
    }
    (format!("{label}: {views} views, {sliders} sliders"), sliders)
}

fn each_view(view: &NSView, visit: &mut dyn FnMut(&NSView)) {
    visit(view);
    let subviews = view.subviews();
    for index in 0..subviews.count() {
        each_view(&subviews.objectAtIndex(index), visit);
    }
}

/// The slider over `stops` stops.
fn slider(window: &NSWindow, stops: isize) -> Result<Retained<NSSlider>> {
    let mut found = None;
    if let Some(content) = window.contentView() {
        each_view(&content, &mut |view| {
            if let Some(slider) = view.downcast_ref::<NSSlider>()
                && slider.numberOfTickMarks() == stops
            {
                found = Some(slider.retain());
            }
        });
    }
    found.ok_or_else(|| format!("no slider over {stops} stops"))
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
