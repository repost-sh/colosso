//! What the visual checks share: the real shell in an off-screen window, with the real text
//! system and Metal, on this repository's own checkout, drawn and saved as PNGs under
//! `.superpowers/qa/`. macOS makes windows only on the main thread, so each check is a mode of
//! the app rather than a test, run with `--features visual-check`.

use std::future::Future;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::pin::{Pin, pin};
use std::process::ExitCode;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use colosso_project::Feed;
use gpui_kit::base::Root;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, KeyUpEvent, Keystroke, PlatformInput,
    VisualTestAppContext, px, size,
};

use crate::projects::read_worktrees;
use crate::shell::Shell;
use crate::theme::UseLightTheme;

pub type Result<T> = std::result::Result<T, String>;

/// The longest a wait parks before it looks again, well under the scheduler's limit.
const PARK: Duration = Duration::from_secs(5);
/// How often a wait draws the window, so what changes meanwhile is laid out.
const FRAME: Duration = Duration::from_millis(100);

/// Runs a check: says each line `draw` returns, the pictures it saved and what it measured,
/// or why it couldn't.
pub fn run(draw: impl FnOnce() -> Result<Vec<String>>) -> ExitCode {
    let mut out = std::io::stdout().lock();
    let written = match draw() {
        Ok(lines) => (lines.iter()).try_for_each(|line| writeln!(out, "{line}")),
        Err(why) => {
            let _ = writeln!(std::io::stderr().lock(), "visual check failed: {why}");
            return ExitCode::FAILURE;
        }
    };
    if written.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// This repository's checkout.
pub fn checkout() -> Result<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.canonicalize().map_err(|error| error.to_string())
}

/// The shell drawn off screen, and where its pictures go.
pub struct Drawn {
    pub cx: VisualTestAppContext,
    pub window: AnyWindowHandle,
    pub shell: Entity<Shell>,
    out: PathBuf,
}

impl Drawn {
    /// The shell in a laptop-sized window off screen, in the light theme.
    pub fn open(out: PathBuf) -> Result<Drawn> {
        let platform = gpui_kit::platform::current_platform(false);
        let assets = Arc::new(gpui_kit::assets::AllAssets);
        let mut cx = VisualTestAppContext::with_asset_source(platform, assets);
        // The project lists and watches the checkout on a thread of its own, which wakes the
        // panels' tasks from outside the scheduler.
        cx.executor().allow_parking();
        cx.update(|cx| {
            crate::init(cx);
            cx.dispatch_action(&UseLightTheme);
        });
        let mut made = None;
        let window = cx
            .open_offscreen_window(size(px(1600.), px(1000.)), |window, cx| {
                let shell = cx.new(|cx| Shell::new(window, cx));
                made = Some(shell.clone());
                cx.new(|cx| Root::new(shell, window, cx))
            })
            .map_err(|error| error.to_string())?;
        let shell = made.ok_or("the window opened without the shell")?;
        Ok(Drawn {
            cx,
            window: window.into(),
            shell,
            out,
        })
    }

    /// The shell off screen, its pictures going to `out` under the checkout, and its
    /// settings, which the check may change, to `COLOSSO_CONFIG_DIR` rather than the app's.
    pub fn open_in(out: &str) -> Result<Drawn> {
        if std::env::var_os("COLOSSO_CONFIG_DIR").is_none() {
            return Err(
                "set COLOSSO_CONFIG_DIR, so the check's settings stay out of the app's".into(),
            );
        }
        let out = checkout()?.join(out);
        std::fs::create_dir_all(&out).map_err(|error| error.to_string())?;
        Drawn::open(out)
    }

    /// The shell off screen with this repository's checkout selected and `more` beside it
    /// in the sidebar, its pictures going to `out` under the checkout. The checkout's
    /// worktrees are read as the sidebar reads them, and it's selected in the same breath,
    /// so the shell never follows another of them.
    pub fn open_on_checkout(out: &str, more: &[&Path]) -> Result<Drawn> {
        let root = checkout()?;
        let mut drawn = Drawn::open_in(out)?;
        let shell = drawn.shell.clone();
        let read: Vec<_> = (std::iter::once(root.as_path()).chain(more.iter().copied()))
            .map(|root| (root.to_owned(), read_worktrees(root)))
            .collect();
        let selected = drawn.cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                let project = shell.workspace.add_project(root.clone());
                for (root, worktrees) in read {
                    shell.workspace.add_project(root.clone());
                    shell.show_worktrees(&root, worktrees, cx);
                }
                shell.workspace.select_worktree(project, &root)
            })
        });
        if !selected {
            return Err("this checkout isn't among its project's worktrees".into());
        }
        Ok(drawn)
    }

    /// Draws a frame, letting what it set off run.
    pub fn draw(&mut self) -> Result<()> {
        self.cx.run_until_parked();
        (self.cx)
            .update_window(self.window, |_, window, cx| window.draw(cx).clear(cx))
            .map_err(|error| error.to_string())?;
        self.cx.run_until_parked();
        Ok(())
    }

    /// Lets the highlighter's debounced parse land, as it would while the user looks.
    pub fn pause(&mut self) -> Result<()> {
        self.draw()?;
        self.cx.advance_clock(Duration::from_millis(500));
        self.draw()
    }

    /// Draws a frame and saves it as `name`, saying where.
    pub fn save(&mut self, name: &str) -> Result<PathBuf> {
        self.draw()?;
        let image = (self.cx.capture_screenshot(self.window)).map_err(|error| error.to_string())?;
        let path = self.out.join(name);
        image.save(&path).map_err(|error| error.to_string())?;
        Ok(path)
    }

    /// Draws until `done`, for up to `patience`, waiting between looks for what the selected
    /// checkout's project publishes.
    pub fn settle(
        &mut self,
        patience: Duration,
        mut done: impl FnMut(&mut Drawn) -> Result<bool>,
    ) -> Result<()> {
        self.draw()?;
        let project = self.cx.read(|cx| {
            let checkout = self.shell.read(cx).right_panel.checkout();
            checkout.and_then(|checkout| checkout.read(cx).project().cloned())
        });
        // Subscribed before looking, so whatever's published after the look wakes the wait.
        let updates = project
            .ok_or("the checkout didn't open")?
            .subscribe(Feed::Everything);
        let deadline = Instant::now() + patience;
        loop {
            self.draw()?;
            if done(self)? {
                return Ok(());
            }
            let left = deadline.checked_duration_since(Instant::now());
            updates.next_within(left.ok_or("it never got there")?);
        }
    }

    /// Draws until `done`, for up to `patience`, looking again each time the scheduler has
    /// something to run: what other threads hand it, a language server's answers say, wakes
    /// it.
    pub fn until(
        &mut self,
        patience: Duration,
        mut done: impl FnMut(&mut Drawn) -> Result<bool>,
    ) -> Result<()> {
        let deadline = Instant::now() + patience;
        self.draw()?;
        let mut drawn_at = Instant::now();
        loop {
            self.cx.run_until_parked();
            if done(self)? {
                return Ok(());
            }
            if drawn_at.elapsed() >= FRAME {
                self.draw()?;
                drawn_at = Instant::now();
            }
            if Instant::now() >= deadline {
                return Err(format!("nothing got there in {} s", patience.as_secs()));
            }
            self.park(Woken::default());
        }
    }

    /// Runs the scheduler until `future` is done, for up to `patience`.
    pub fn finish(&mut self, patience: Duration, future: impl Future<Output = ()>) -> Result<()> {
        let deadline = Instant::now() + patience;
        let mut future = pin!(future);
        let done = std::cell::Cell::new(false);
        while !done.get() {
            if Instant::now() >= deadline {
                return Err(format!("it didn't finish in {} s", patience.as_secs()));
            }
            self.cx.run_until_parked();
            self.park(async {
                future.as_mut().await;
                done.set(true);
            });
        }
        Ok(())
    }

    /// Holds `key` down for `held` at 120 frames a second, the app's repeater driving it,
    /// then lets it up: how many steps it took, as `count` counts them, how many it took in a
    /// frame at most, and what each frame cost, the steps it took included.
    pub fn hold(
        &mut self,
        what: &str,
        key: &'static str,
        held: Duration,
        mut count: impl FnMut(&mut Drawn) -> Result<usize>,
    ) -> Result<String> {
        let keystroke = Keystroke::parse(key).map_err(|error| error.to_string())?;
        let window = self.window;
        self.draw()?;
        let first = count(self)?;
        (self.cx)
            .update_window(window, |_, window, cx| {
                window.dispatch_keystroke(keystroke.clone(), cx);
                window.draw(cx).clear(cx);
            })
            .map_err(|error| error.to_string())?;
        let frame = Duration::from_secs(1) / 120;
        let (mut took, mut fastest, mut last) = (Vec::new(), 0, count(self)?);
        for _ in 0..held.as_millis() / frame.as_millis().max(1) {
            self.cx.advance_clock(frame);
            let started = Instant::now();
            (self.cx)
                .update_window(window, |_, window, cx| {
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                })
                .map_err(|error| error.to_string())?;
            took.push(started.elapsed());
            self.cx.run_until_parked();
            let now = count(self)?;
            fastest = fastest.max(now.abs_diff(last));
            last = now;
        }
        (self.cx)
            .update_window(window, |_, window, cx| {
                window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
            })
            .map_err(|error| error.to_string())?;
        took.sort();
        let at = |share: f64| took[((took.len() - 1) as f64 * share).round() as usize];
        let ms = |duration: Duration| duration.as_secs_f64() * 1000.;
        Ok(format!(
            "{what}: {key} held {:.1} s moved {} steps, at most {fastest} a frame; a frame {:.2} ms p50, {:.2} p95, {:.2} max",
            held.as_secs_f64(),
            last.abs_diff(first),
            ms(at(0.5)),
            ms(at(0.95)),
            ms(at(1.)),
        ))
    }

    /// Blocks on `future` until it's done or [`PARK`] has passed, running the background work
    /// meanwhile. The deadline is a timer thread's, not `block_with_timeout`'s: the test
    /// scheduler takes its locks for that in the opposite order to a thread handing it work,
    /// and the two deadlock.
    fn park(&self, future: impl Future<Output = ()>) {
        let _: Option<()> =
            (self.cx.foreground_executor()).block_on(colosso_lsp::within(PARK, future));
    }
}

/// Not done when first looked at, done when looked at again: blocked on, it returns once the
/// scheduler has been woken to run something, or a timer is due.
#[derive(Default)]
struct Woken(bool);

impl Future for Woken {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            Poll::Pending
        }
    }
}
