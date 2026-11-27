//! What the app's tests share: folders of their own under the target folder, git run in them
//! as a user would, with no hooks or signing of theirs, and the shell in a window over them.
//! The visual checks share the git part, to make their fixtures.

use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, CursorStyle, Entity, Modifiers, Pixels, Point,
    Render, Size, TestAppContext, VisualTestContext, Window, WindowBounds, WindowOptions,
    WindowVisibility, point, px, size,
};
#[cfg(test)]
use koyo_term_control::{
    Actor, Control, Event, EventKind, Handle, Status, TerminalRecord, TerminalSummary, WorktreeRef,
};

#[cfg(test)]
use crate::browser::BrowserView;
#[cfg(test)]
use crate::model::PaneKind;
#[cfg(test)]
use crate::projects::read_worktrees;
#[cfg(test)]
use crate::right_panel::{ToggleChanges, View};
#[cfg(test)]
use crate::shell::{KeptWindow, Shell};

#[cfg(test)]
mod keys;
#[cfg(test)]
mod panes;
#[cfg(test)]
mod pinned;
#[cfg(test)]
mod pointer;
mod started;
#[cfg(test)]
pub use keys::{act, frames, press, pressed};
#[cfg(test)]
pub use panes::{iphone, terminals_listed};
#[cfg(test)]
pub use pinned::{loose, pin, pinned, shown};
#[cfg(test)]
pub use pointer::{
    GRAB, carry, drag, fingers, hold, hover, let_go, let_go_on, move_held, take_up,
};
#[cfg(test)]
pub use started::init_settings;
pub use started::init;

/// A folder for one test under the target folder, empty, named after `name` and a number of its
/// own: two tests that pick one name don't empty each other's folder while both run.
#[cfg(test)]
pub fn scratch(name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = Path::new(env!("KOYO_TARGET_DIR"))
        .join("test-repos")
        .join(format!("{name}-{}-{next}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::canonicalize(root).unwrap()
}

/// Runs git with `args` in `root`, or says why it didn't succeed.
pub fn try_git(root: &Path, args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| format!("git {args:?}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let said = String::from_utf8_lossy(&output.stderr);
        Err(format!("git {args:?}: {}", said.trim()))
    }
}

/// Runs git with `args` in `root`, which has to succeed.
#[cfg(test)]
pub fn git(root: &Path, args: &[&str]) {
    try_git(root, args).unwrap();
}

/// Writes `contents` to the file at `path` in `root`, making the folders it's in.
#[cfg(test)]
pub fn write(root: &Path, path: &str, contents: &[u8]) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// A repository of its own, `files` in it committed, on `main`.
#[cfg(test)]
pub fn repo(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = scratch(name);
    for (path, text) in files {
        write(&root, path, text.as_bytes());
    }
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "--allow-empty", "-m", "first"]);
    root
}

/// Draws `window` once what's pending has run, then runs what drawing it started.
#[cfg(test)]
pub fn draw(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.run_until_parked();
    (cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))).unwrap();
    cx.run_until_parked();
}

/// Brings `window` in front, the app's active window, or puts it behind another app's, as a
/// person does. A test's window starts behind.
#[cfg(test)]
pub fn in_front(window: AnyWindowHandle, front: bool, cx: &mut TestAppContext) {
    if front {
        (cx.update_window(window, |_, window, _| window.activate_window())).unwrap();
    } else {
        VisualTestContext::from_window(window, cx).deactivate_window();
    }
    cx.run_until_parked();
}

/// Shows `window` where a person can see it, or hides it from them, as covering it, minimizing it
/// or going to another desktop does. A test's window starts seen.
#[cfg(test)]
pub fn on_screen(window: AnyWindowHandle, seen: bool, cx: &mut TestAppContext) {
    let visibility = if seen {
        WindowVisibility::Visible
    } else {
        WindowVisibility::Hidden
    };
    cx.simulate_window_visibility_change(window, visibility);
    cx.run_until_parked();
}

/// A view that draws nothing: a window to float others over, or what they hold.
#[cfg(test)]
pub struct Blank;

#[cfg(test)]
impl Render for Blank {
    fn render(
        &mut self,
        _: &mut Window,
        _: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        gpui_kit::div()
    }
}

/// What `build` makes, in a window of its own of `size`.
#[cfg(test)]
pub fn in_window<V: Render>(
    size: Size<Pixels>,
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size,
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..crate::window_options::base()
        };
        gpui_kit::open_window(options, cx, build).unwrap()
    })
}

/// The shell in a laptop-sized window, each of `roots` a project in its sidebar.
#[cfg(test)]
pub fn shell_with(roots: &[&Path], cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    cx.executor().allow_parking();
    cx.update(init);
    let (window, shell) = shell_window(cx);
    shell.update(cx, |shell, cx| {
        for root in roots {
            shell.workspace.add_project(root.to_path_buf());
            shell.show_worktrees(root, read_worktrees(root), cx);
        }
        shell.save_projects(cx);
    });
    (window, shell)
}

/// The shell over a checkout of its own with a README, selected and drawn: the window, the shell
/// and the checkout.
#[cfg(test)]
pub fn shell_on(name: &str, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>, PathBuf) {
    let root = repo(name, &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    draw(window, cx);
    (window, shell, root)
}

/// The shell over a checkout of its own whose one tab, a chat, is shown, motion reduced so what
/// moves is where it goes at once: the window and the shell.
#[cfg(test)]
pub fn shell_on_chat(name: &str, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    let (window, shell, _) = shell_on(name, cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    shell.update(cx, |shell, cx| {
        shell.workspace.new_tab(PaneKind::Chat);
        cx.notify();
    });
    draw(window, cx);
    (window, shell)
}

/// The shell in a laptop-sized window of its own, as the app opens it.
#[cfg(test)]
pub fn shell_window(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    in_window(size(px(1400.), px(900.)), cx, |window, cx| {
        cx.new(|cx| Shell::new(window, KeptWindow::default(), cx))
    })
}

/// Selects the checkout at `root` in the sidebar.
#[cfg(test)]
pub fn select(shell: &Entity<Shell>, root: &Path, cx: &mut TestAppContext) {
    shell.update(cx, |shell, cx| {
        let project = shell.workspace.project_index(root).unwrap();
        assert!(shell.workspace.select_worktree(project, root));
        cx.notify();
    });
    cx.run_until_parked();
}

/// Closes the tab shown, as the close button does, and lets what it starts run.
#[cfg(test)]
pub fn close_shown(window: AnyWindowHandle, shell: &Entity<Shell>, cx: &mut TestAppContext) {
    window
        .update(cx, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                let at = shell.workspace.shown_tab().unwrap();
                shell.close_tab_at(at, window, cx);
            });
        })
        .unwrap();
    cx.run_until_parked();
}

/// Opens a browser tab at `url` on the selected checkout, or on the start page with none: its
/// pane.
#[cfg(test)]
pub fn open_page(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    url: Option<&str>,
    cx: &mut TestAppContext,
) -> u64 {
    let url = url.map(str::to_owned);
    let opened = cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_browser_tab(url, (false, false), None, window, cx)
        })
    });
    opened.unwrap().expect("a checkout is selected")
}

/// Opens a browser tab at `url` on the selected checkout and draws it: its page.
#[cfg(test)]
pub fn open_page_view(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    url: &str,
    cx: &mut TestAppContext,
) -> Entity<BrowserView> {
    let pane = open_page(window, shell, Some(url), cx);
    draw(window, cx);
    shell.read_with(cx, |shell, _| {
        let pane = shell.workspace.pane(pane).expect("the tab's pane");
        pane.view().expect("its page").clone()
    })
}

/// Opens the right panel on the changes of the selected checkout and draws it.
#[cfg(test)]
pub fn open_changes(window: AnyWindowHandle, shell: &Entity<Shell>, cx: &mut TestAppContext) {
    cx.dispatch_action(window, ToggleChanges);
    draw(window, cx);
    let open = shell.read_with(cx, |shell, _| shell.right_panel.shows(View::Changes));
    assert!(open, "the right panel is open on the changes");
}

/// Clicks in the middle of pane `pane` of the tab shown, as a person does to be in it.
#[cfg(test)]
pub fn click_pane(window: AnyWindowHandle, pane: u64, cx: &mut TestAppContext) {
    draw(window, cx);
    click(
        window,
        Box::leak(format!("pane-{pane}").into_boxed_str()),
        cx,
    );
}

/// The middle of what `selector` names in `window`, as it was last drawn.
#[cfg(test)]
pub fn center_of(
    window: AnyWindowHandle,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Point<Pixels> {
    let bounds = VisualTestContext::from_window(window, cx).debug_bounds(selector);
    bounds.expect(selector).center()
}

/// Where `selector` names something in `window`, once it's drawn again; none while it names
/// nothing drawn.
#[cfg(test)]
pub fn drawn(
    window: AnyWindowHandle,
    selector: impl Into<String>,
    cx: &mut TestAppContext,
) -> Option<Bounds<Pixels>> {
    draw(window, cx);
    let selector: &'static str = Box::leak(selector.into().into_boxed_str());
    VisualTestContext::from_window(window, cx).debug_bounds(selector)
}

/// Clicks the middle of what `selector` names in `window`, and draws what that changed.
#[cfg(test)]
pub fn click(window: AnyWindowHandle, selector: &'static str, cx: &mut TestAppContext) {
    let at = center_of(window, selector, cx);
    VisualTestContext::from_window(window, cx).simulate_click(at, Modifiers::default());
    draw(window, cx);
}

/// The cursor `window` shows with the pointer moved to `at`, no button down, as it was last
/// drawn: GPUI says it only in its account of the app's state.
#[cfg(test)]
pub fn cursor_at(
    window: AnyWindowHandle,
    at: Point<Pixels>,
    cx: &mut TestAppContext,
) -> CursorStyle {
    hover(window, at, cx);
    let state = cx.update(|cx| cx.debug_state());
    let id = window.window_id().as_u64();
    let windows = state["windows"].as_array().expect("the app's windows");
    let shown = windows
        .iter()
        .find(|shown| shown["id"] == id)
        .expect("the window");
    serde_json::from_value::<Option<CursorStyle>>(shown["cursor_style"].clone())
        .expect("a cursor style")
        .unwrap_or_default()
}

/// Whether the keyboard is in key context `context` after a few frames, where what would take
/// it or give it back has had its chance: [`PROMPT`] for a chat's prompt.
#[cfg(test)]
pub fn keyboard_in(window: AnyWindowHandle, context: &str, cx: &mut TestAppContext) -> bool {
    for _ in 0..3 {
        draw(window, cx);
    }
    let stack = cx.update_window(window, |_, window, _| window.context_stack());
    stack.unwrap().iter().any(|at| at.contains(context))
}

/// The key context of a chat's prompt.
#[cfg(test)]
pub const PROMPT: &str = "ChatComposer";

/// Whether the window itself has the keyboard after a few frames, and so no pane of it.
#[cfg(test)]
pub fn keyboard_at_window(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    cx: &mut TestAppContext,
) -> bool {
    for _ in 0..3 {
        draw(window, cx);
    }
    let at = cx.update_window(window, |_, window, cx| {
        shell.read(cx).focus.is_focused(window)
    });
    at.unwrap()
}

/// Presses the chat shown, over its empty background high above the prompt.
#[cfg(test)]
pub fn press_chat_background(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    cx: &mut TestAppContext,
) {
    draw(window, cx);
    let pane = shell.read_with(cx, |shell, _| shell.shown_tab_key().expect("a tab shown"));
    let selector: &'static str = Box::leak(format!("pane-{pane}").into_boxed_str());
    let mut vcx = VisualTestContext::from_window(window, cx);
    let bounds = vcx.debug_bounds(selector).expect("the chat's pane");
    vcx.simulate_click(
        point(bounds.center().x, bounds.top() + px(80.)),
        Modifiers::default(),
    );
    draw(window, cx);
}

/// Presses the prompt of the chat shown, in its first line of text, as a person does to type.
#[cfg(test)]
pub fn press_prompt(window: AnyWindowHandle, cx: &mut TestAppContext) {
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    let glass = vcx.debug_bounds("chat-composer").expect("the prompt's box");
    vcx.simulate_click(
        point(glass.center().x, glass.top() + px(22.)),
        Modifiers::default(),
    );
    draw(window, cx);
}

/// Terminal `handle` as the daemon lists it: called `name`, at its prompt in the checkout at
/// `worktree`.
#[cfg(test)]
pub fn terminal_listed(handle: &str, name: &str, worktree: &Path) -> TerminalSummary {
    let at = worktree.to_str().expect("a path that's text");
    TerminalSummary {
        record: TerminalRecord {
            name: name.into(),
            ..terminal_record(handle, at, Status::Idle, "prompt")
        },
        preview: String::new(),
    }
}

/// What the daemon says of terminal `handle` going to `status` for `reason`.
#[cfg(test)]
pub fn status_event(handle: &str, status: Status, reason: &str) -> Event {
    Event {
        handle: Handle(handle.into()),
        kind: EventKind::Status {
            status,
            reason: reason.into(),
            progress: None,
        },
    }
}

/// A terminal as the daemon tells of it: `handle`, named after it, in the checkout at
/// `worktree`, at `status` for `reason`.
#[cfg(test)]
pub fn terminal_record(
    handle: &str,
    worktree: &str,
    status: Status,
    reason: &str,
) -> TerminalRecord {
    TerminalRecord {
        handle: Handle(handle.into()),
        worktree: Some(WorktreeRef {
            path: worktree.into(),
            repo_main: worktree.into(),
            branch: None,
        }),
        name: handle.into(),
        title: None,
        cwd: worktree.into(),
        command: vec![],
        env: std::collections::BTreeMap::new(),
        size: koyo_term_control::Size::default(),
        labels: std::collections::BTreeSet::new(),
        created_ms: 1,
        created_by: Actor::system(),
        incarnation: 1,
        pid: Some(42),
        exit: None,
        status,
        status_reason: reason.into(),
        last_exit: None,
        progress: None,
        control: Control::default(),
        attached_clients: 1,
        last_output_ms: 1,
        pinned: None,
    }
}
