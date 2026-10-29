//! The app the editor is measured in: a real macOS window, off screen, with the real text
//! system and Metal renderer, so shaping and painting cost what they cost in Colosso. Its
//! scheduler runs nothing until asked, so a sample times exactly the work of one input event
//! and the full frame drawn after it, and the work the event leaves for later runs between
//! samples, outside the timing.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use gpui_kit::base::Root;
use gpui_kit::component::input::{Editor, EditorState, Position};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _,
    Pixels, Render, SharedString, Styled as _, VisualTestAppContext, Window, div, px, size,
};

/// The window: a laptop-sized editor pane.
pub const WIDTH: f32 = 1280.;
pub const HEIGHT: f32 = 800.;

/// How long a pause after setup lasts: past the highlighter's debounce, so the background
/// parse has landed before sampling starts.
const PAUSE: Duration = Duration::from_millis(500);

/// The window's content: one editor, as wide as the window unless a resize narrowed it.
struct Host {
    editor: Entity<EditorState>,
    width: Option<Pixels>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let pane = div().h_full().child(Editor::new(&self.editor).h_full());
        let pane = match self.width {
            Some(width) => pane.w(width),
            None => pane.w_full(),
        };
        div().size_full().child(pane)
    }
}

/// How the editor is set up for a scenario.
#[derive(Clone, Copy)]
pub struct Setup {
    pub language: &'static str,
    pub soft_wrap: bool,
}

fn new_editor(setup: Setup, window: &mut Window, cx: &mut App) -> Entity<EditorState> {
    cx.new(|cx| {
        EditorState::new(window, cx)
            .language(setup.language)
            .soft_wrap(setup.soft_wrap)
            // Wheel scrolling never bounces, but a stretch left over would animate.
            .scroll_bounce(false)
    })
}

pub struct Bench {
    cx: VisualTestAppContext,
    window: AnyWindowHandle,
    host: Entity<Host>,
    setup: Setup,
}

impl Bench {
    /// Opens the window with an empty editor and draws its first frame.
    pub fn open(setup: Setup) -> Result<Self> {
        let platform = gpui_kit::platform::current_platform(false);
        let mut cx = VisualTestAppContext::with_asset_source(
            platform,
            Arc::new(gpui_kit::assets::AllAssets),
        );
        cx.update(gpui_kit::init);
        let mut host = None;
        let window = cx.open_offscreen_window(size(px(WIDTH), px(HEIGHT)), |window, cx| {
            let editor = new_editor(setup, window, cx);
            let view = cx.new(|_| Host {
                editor,
                width: None,
            });
            host = Some(view.clone());
            cx.new(|cx| Root::new(view, window, cx))
        })?;
        let host = host.context("the window opened without its content")?;
        let mut bench = Self {
            cx,
            window: window.into(),
            host,
            setup,
        };
        bench.draw()?;
        Ok(bench)
    }

    /// Times one sample: `input`, delivered as the event loop would deliver an event, then
    /// the full frame drawn after it (update, layout, prepaint, paint).
    pub fn frame(&mut self, input: impl FnOnce(&mut Window, &mut App)) -> Result<Duration> {
        time_frame(&mut self.cx, self.window, input)
    }

    /// Times the editor set to `width`, as a window resize would set it, and the frame after.
    pub fn set_width(&mut self, width: Pixels) -> Result<Duration> {
        let host = &self.host;
        time_frame(&mut self.cx, self.window, |_, cx| {
            host.update(cx, |host, cx| {
                host.width = Some(width);
                cx.notify();
            });
        })
    }

    /// Draws a frame without anything to time.
    pub fn draw(&mut self) -> Result<()> {
        self.frame(|_, _| {}).map(drop)
    }

    /// Lets deferred work finish, as it would while the user pauses: the debounced background
    /// parse lands and its highlights are drawn.
    pub fn pause(&mut self) -> Result<()> {
        self.cx.run_until_parked();
        self.cx.advance_clock(PAUSE);
        self.cx.run_until_parked();
        self.draw()
    }

    /// Changes the window untimed, draws it, and lets it settle.
    pub fn update_and_settle(&mut self, f: impl FnOnce(&mut Window, &mut App)) -> Result<()> {
        self.update(f)?;
        self.draw()?;
        self.pause()
    }

    /// Replaces the editor with a fresh one, as opening a file in a new tab would.
    pub fn new_editor(&mut self) -> Result<()> {
        let (host, setup) = (&self.host, self.setup);
        self.cx.update_window(self.window, |_, window, cx| {
            let editor = new_editor(setup, window, cx);
            host.update(cx, |host, cx| {
                host.editor = editor;
                cx.notify();
            });
        })?;
        self.draw()
    }

    pub fn editor(&self) -> Entity<EditorState> {
        self.cx.read(|cx| self.host.read(cx).editor.clone())
    }

    /// Sets the editor's text, untimed, and lets it settle.
    pub fn load(&mut self, text: SharedString) -> Result<()> {
        let editor = self.editor();
        self.update_and_settle(move |window, cx| {
            editor.update(cx, |state, cx| state.set_value(text, window, cx));
        })
    }

    /// Changes the window untimed.
    pub fn update<R>(&mut self, f: impl FnOnce(&mut Window, &mut App) -> R) -> Result<R> {
        self.cx
            .update_window(self.window, |_, window, cx| f(window, cx))
    }

    pub fn read_editor<R>(&self, f: impl FnOnce(&EditorState, &App) -> R) -> R {
        self.cx.read(|cx| f(self.host.read(cx).editor.read(cx), cx))
    }

    /// Puts the cursor at `position` and focuses the editor, then lets the view scroll there.
    pub fn place_cursor(&mut self, position: Position) -> Result<()> {
        let editor = self.editor();
        self.update_and_settle(move |window, cx| {
            editor.update(cx, |state, cx| {
                state.set_cursor_position(position, window, cx);
            });
        })
    }

    pub fn scale_factor(&mut self) -> Result<f32> {
        self.update(|window, _| window.scale_factor())
    }
}

/// The timing behind [`Bench::frame`], apart from the bench so a frame's input can borrow
/// the rest of it.
fn time_frame(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    input: impl FnOnce(&mut Window, &mut App),
) -> Result<Duration> {
    let started = Instant::now();
    // Two updates, as the event and the frame are two turns of the event loop: what the
    // event notified is flushed before the frame looks for what changed.
    cx.update_window(window, |_, window, cx| input(window, cx))?;
    cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))?;
    let took = started.elapsed();
    // Work the event spawned runs between frames, as it would in the app, untimed. Time
    // stands still, so nothing debounced fires mid-scenario: a typist who never pauses.
    cx.run_until_parked();
    Ok(took)
}
