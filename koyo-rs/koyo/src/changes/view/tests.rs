//! The changes panel: its file list grouped and acted on, and a file picked in it glided to in
//! the diffs.

use koyo_git::{Change, Content, FileDiff, Hunk, Lines};
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Point, TestAppContext, WindowBounds,
    WindowOptions, size,
};

use super::*;

/// The panel's list beside its diffs, as the right panel and the Changes tab show them.
struct Both {
    panel: Entity<ChangesPanel>,
    diffs: Entity<ChangesTab>,
}

impl Render for Both {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let list = div().w(px(320.)).h_full().child(self.panel.clone());
        let diffs = div().flex_1().min_w_0().h_full().child(self.diffs.clone());
        div().size_full().flex().child(list).child(diffs)
    }
}

/// A Rust file of `len` lines with every seventh replaced, as an agent's edit might leave it.
fn edited(path: &str, len: u32) -> FileDiff {
    let line = |n: u32, new: bool| {
        let name = if new { "updated" } else { "original" };
        format!("    let {name}_{n} = compute(input, {n}); // keeps the total in range\n")
    };
    let old: String = (0..len).map(|n| line(n, false)).collect();
    let new: String = (0..len).map(|n| line(n, n % 7 == 3)).collect();
    let hunks = (0..len)
        .filter(|n| n % 7 == 3)
        .map(|n| Hunk {
            old: n..n + 1,
            new: n..n + 1,
        })
        .collect();
    FileDiff {
        change: Change {
            path: path.into(),
            status: Status::Modified,
            staged: false,
        },
        content: Content::Text {
            old: Lines::new(old),
            new: Lines::new(new),
            hunks,
        },
    }
}

#[test]
fn staged_and_unstaged_files_are_grouped_apart() {
    let file = |path: &str, staged| {
        let mut diff = edited(path, 10);
        diff.change.staged = staged;
        Arc::new(FileView::new(diff))
    };
    let files = [
        file("src/a.rs", true),
        file("src/a.rs", false),
        file("b.rs", false),
    ];
    // Closing a folder in one group leaves the same folder in the other open.
    let closed = HashSet::from([(false, "src".to_owned())]);
    let rows: Vec<String> = (file_entries(&files, true, &closed).iter())
        .map(|entry| match entry {
            Entry::Folder { name, depth, .. } => format!("{depth} {name}/"),
            Entry::File { index, depth } => format!("{depth} #{index}"),
        })
        .collect();
    assert_eq!(
        rows,
        [
            "0 Staged/",
            "1 src/",
            "2 #0",
            "0 Unstaged/",
            "1 src/",
            "1 #2"
        ]
    );
}

/// A panel in a window of its own, showing `count` edited files.
fn open(count: usize, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<ChangesPanel>) {
    let files = (0..count)
        .map(|n| edited(&format!("crates/app/src/module_{n:03}.rs"), 400))
        .collect();
    open_with(files, cx)
}

/// A panel in a window of its own, beside its diffs, showing `files`.
fn open_with(
    files: Vec<FileDiff>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<ChangesPanel>) {
    cx.update(gpui_kit::init);
    cx.update(settings::init);
    cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(960.), px(1100.)),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..WindowOptions::default()
        };
        let mut panel = None;
        let (window, _) = gpui_kit::open_window(options, cx, |window, cx| {
            let changes = cx.new(|cx| Checkout::read_as(files, window, cx));
            let made = cx.new(|cx| ChangesPanel::new(changes, cx));
            panel = Some(made.clone());
            let diffs = cx.new(|cx| ChangesTab::new(made.clone(), cx));
            cx.new(|_| Both { panel: made, diffs })
        })
        .unwrap();
        (window, panel.unwrap())
    })
}

#[gpui_kit::test]
fn a_folder_or_group_acts_on_the_files_under_it(cx: &mut TestAppContext) {
    let file = |path: &str, staged| {
        let mut diff = edited(path, 10);
        diff.change.staged = staged;
        diff
    };
    let files = vec![
        file("src/a.rs", true),
        file("src/a.rs", false),
        file("src/b.rs", false),
        file("c.rs", false),
    ];
    let (_, panel) = open_with(files, cx);
    panel.update(cx, |panel, _| {
        // Staged/, src/, a.rs, Unstaged/, src/, a.rs, b.rs, c.rs
        assert_eq!(panel.entry_files(0), [0]);
        assert_eq!(panel.entry_files(1), [0]);
        assert_eq!(panel.entry_files(3), [1, 2, 3]);
        assert_eq!(panel.entry_files(4), [1, 2]);
        assert_eq!(panel.entry_files(7), [3]);
        assert!(panel.entry_is_staged(1) && !panel.entry_is_staged(4));
    });
}

#[gpui_kit::test]
fn hovering_the_file_list_shows_its_actions_without_breaking_a_frame(cx: &mut TestAppContext) {
    let (handle, _) = open(8, cx);
    let draw = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
    };
    draw(cx);
    // Down the file list, row by row, onto each row and off it again.
    for y in (40..400).step_by(9) {
        cx.update_window(handle, |_, window, cx| {
            window.simulate_mouse_move(gpui_kit::point(px(120.), px(y as f32)), cx);
        })
        .unwrap();
        draw(cx);
    }
}

#[gpui_kit::test]
fn a_file_picked_in_the_list_is_glided_to_and_stays_picked(cx: &mut TestAppContext) {
    let (handle, panel) = open(20, cx);
    let draw = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
    };
    draw(cx);
    panel.update(cx, |panel, cx| panel.show(12, cx));
    // Partway there, the file it's going to stays the one picked.
    draw(cx);
    panel.update(cx, |panel, _| {
        assert!(panel.glide.is_some());
        assert_ne!(panel.list.logical_scroll_top().item_ix, panel.headers[12]);
        assert_eq!(panel.current, Some(12));
    });
    panel.update(cx, |panel, _| {
        let glide = panel.glide.as_mut().unwrap();
        glide.start -= GLIDE;
    });
    draw(cx);
    panel.update(cx, |panel, _| {
        assert!(panel.glide.is_none());
        let top = panel.list.logical_scroll_top();
        assert_eq!(top.item_ix, panel.headers[12]);
        assert_eq!(top.offset_in_item, px(0.));
        assert_eq!(panel.current, Some(12));
    });
}

#[gpui_kit::test]
fn the_file_list_takes_the_keys_the_file_tree_takes(cx: &mut TestAppContext) {
    let files = (0..80)
        .map(|n| edited(&format!("src/module_{n:03}.rs"), 4))
        .collect();
    let (handle, panel) = open_with(files, cx);
    cx.update(crate::right_panel::init);
    let draw = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
    };
    cx.update_window(handle, |_, window, cx| {
        let focus = panel.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
    })
    .unwrap();
    draw(cx);
    let press = |keys: &str, cx: &mut TestAppContext| {
        cx.simulate_keystrokes(handle, keys);
        draw(cx);
        cx.read(|cx| panel.read(cx).cursor())
    };
    let last = cx.read(|cx| panel.read(cx).entries.len() - 1);
    assert_eq!(press("end", cx), Some(last));
    assert_eq!(press("home", cx), Some(0));
    let paged = press("pagedown", cx).unwrap();
    assert!(
        1 < paged && paged < last,
        "a page is the rows in sight: {paged}"
    );
    assert_eq!(press("pageup", cx), Some(0));
}

/// How long a frame takes to draw the diffs after jumping from file to file of a large
/// diff, by arrow key, one way and back. Run with
/// `script/cargo test --release -p koyo jumping -- --ignored --nocapture`.
#[gpui_kit::test]
#[ignore = "a measurement, not a check"]
fn jumping_between_files(cx: &mut TestAppContext) {
    let (handle, panel) = open(200, cx);
    let mut frame = |step: &mut dyn FnMut(&mut ChangesPanel, &mut Context<ChangesPanel>)| {
        cx.update_window(handle, |_, window, cx| {
            panel.update(cx, |panel, cx| step(panel, cx));
            let started = Instant::now();
            window.draw(cx).clear(cx);
            started.elapsed()
        })
        .unwrap()
    };
    frame(&mut |_, _| {});
    let mut times: Vec<Duration> = Vec::new();
    for step in [1, -1] {
        for _ in 0..199 {
            times.push(frame(&mut |panel, cx| panel.step(step, true, cx)));
        }
    }
    times.sort();
    let at = |share: f64| times[((times.len() - 1) as f64 * share) as usize];
    eprintln!(
        "jump frames: median {:?}, 90th {:?}, worst {:?}, over {} jumps",
        at(0.5),
        at(0.9),
        at(1.),
        times.len()
    );
}
