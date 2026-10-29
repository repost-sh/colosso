//! The Files panel drawn as the app draws it: the real shell in an off-screen window, with
//! the real text system and Metal, on this repository's own checkout, saved as PNGs in
//! `.superpowers/qa/files/`. macOS makes windows only on the main thread, so it's a mode of
//! the app rather than a test:
//!
//! ```sh
//! COLOSSO_CONFIG_DIR=$PWD/.superpowers/qa/files/config \
//!   cargo run -p colosso --features visual-check -- --visual-check
//! ```

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use colosso_project::Feed;
use gpui_kit::base::Root;
use gpui_kit::{AnyWindowHandle, AppContext as _, Context, Entity, VisualTestAppContext, px, size};

use super::FilesPanel;
use crate::projects::read_worktrees;
use crate::right_panel::{ShowChanges, ShowFiles};
use crate::shell::Shell;
use crate::theme::UseLightTheme;

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/files";
/// How long the project gets to list what's asked of it.
const PATIENCE: Duration = Duration::from_secs(20);

type Result<T> = std::result::Result<T, String>;

/// Draws the pictures and says where they are, or why it couldn't.
pub fn run() -> ExitCode {
    let mut out = std::io::stdout().lock();
    let written = match draw() {
        Ok(paths) => (paths.iter()).try_for_each(|path| writeln!(out, "{}", path.display())),
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

struct Drawn {
    cx: VisualTestAppContext,
    window: AnyWindowHandle,
    shell: Entity<Shell>,
    out: PathBuf,
}

fn draw() -> Result<Vec<PathBuf>> {
    if std::env::var_os("COLOSSO_CONFIG_DIR").is_none() {
        return Err("set COLOSSO_CONFIG_DIR, so the check's settings stay out of the app's".into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let out = root.join(OUT);
    std::fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    let mut drawn = Drawn::open(out)?;
    // This checkout as a project, its worktrees read as the sidebar reads them, and it
    // selected in the same breath, so the shell never follows another of them.
    let shell = drawn.shell.clone();
    let read = read_worktrees(&root);
    let selected = drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            let project = shell.workspace.add_project(root.clone());
            shell.show_worktrees(&root, read, cx);
            shell.workspace.select_worktree(project, &root)
        })
    });
    if !selected {
        return Err("this checkout isn't among its project's worktrees".into());
    }
    drawn.cx.dispatch_action(drawn.window, ShowFiles);
    drawn.settle(|panel| panel.scanned)?;

    // A file opened from the tree: its folders are revealed, and it's picked.
    let window = drawn.window;
    (drawn.cx)
        .update_window(window, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_file("crates/project/src/lib.rs", window, cx);
            });
        })
        .map_err(|error| error.to_string())?;
    drawn.toggle("script")?;
    drawn.pause()?;
    let tree = drawn.save("files-panel.png")?;

    // An ignored folder opened: read once asked for, and shown dimmed.
    drawn.toggle("crates")?;
    drawn.toggle("target")?;
    drawn.settle(|panel| panel.rows.position("target/debug").is_some())?;
    drawn.panel(|panel, cx| {
        let at = panel.rows.position("target");
        at.map(|at| panel.select(at, cx))
    })?;
    let ignored = drawn.save("files-ignored.png")?;

    // The changes, with the same switch at the left of their header.
    drawn.cx.dispatch_action(drawn.window, ShowChanges);
    drawn.pause()?;
    let changes = drawn.save("changes-switch.png")?;
    Ok(vec![tree, ignored, changes])
}

impl Drawn {
    /// The shell in a laptop-sized window off screen, in the light theme.
    fn open(out: PathBuf) -> Result<Drawn> {
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

    fn draw(&mut self) -> Result<()> {
        self.cx.run_until_parked();
        (self.cx)
            .update_window(self.window, |_, window, cx| window.draw(cx).clear(cx))
            .map_err(|error| error.to_string())?;
        self.cx.run_until_parked();
        Ok(())
    }

    /// Lets the highlighter's debounced parse land, as it would while the user looks.
    fn pause(&mut self) -> Result<()> {
        self.draw()?;
        self.cx.advance_clock(Duration::from_millis(500));
        self.draw()
    }

    fn save(&mut self, name: &str) -> Result<PathBuf> {
        self.draw()?;
        let image = (self.cx.capture_screenshot(self.window)).map_err(|error| error.to_string())?;
        let path = self.out.join(name);
        image.save(&path).map_err(|error| error.to_string())?;
        Ok(path)
    }

    /// Lets the Files panel take what the checkout's project publishes until it's `done`,
    /// waiting for the project between looks.
    fn settle(&mut self, done: impl Fn(&FilesPanel) -> bool) -> Result<()> {
        self.draw()?;
        let project = self.cx.read(|cx| {
            let checkout = self.shell.read(cx).right_panel.checkout();
            checkout.and_then(|checkout| checkout.read(cx).project().cloned())
        });
        // Subscribed before looking, so whatever's published after the look wakes the wait.
        let updates = project
            .ok_or("the checkout didn't open")?
            .subscribe(Feed::Everything);
        let deadline = Instant::now() + PATIENCE;
        loop {
            self.draw()?;
            if self.panel(|panel, _| done(panel))? {
                return Ok(());
            }
            let left = deadline.checked_duration_since(Instant::now());
            updates.next_within(left.ok_or("the Files panel never got there")?);
        }
    }

    fn panel<R>(
        &mut self,
        f: impl FnOnce(&mut FilesPanel, &mut Context<FilesPanel>) -> R,
    ) -> Result<R> {
        let panel = self
            .cx
            .read(|cx| self.shell.read(cx).right_panel.files().cloned());
        let panel = panel.ok_or("the Files panel isn't shown")?;
        Ok(self.cx.update(|cx| panel.update(cx, f)))
    }

    /// Opens or closes the folder at `path`, which is shown.
    fn toggle(&mut self, path: &str) -> Result<()> {
        let toggled = self.panel(|panel, cx| {
            let at = panel.rows.position(path)?;
            panel.toggle(at, cx);
            Some(())
        })?;
        toggled.ok_or_else(|| format!("{path} isn't shown"))?;
        self.draw()
    }
}
