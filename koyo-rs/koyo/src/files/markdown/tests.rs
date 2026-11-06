use std::path::Path;

use gpui_kit::{AnyWindowHandle, App, AppContext as _, Entity, SharedString, TestAppContext};

use super::diagram::{self, Card, Diagram, Look};
use super::mermaid::{PADDING, is_mermaid};
use super::{MarkdownPreview, Mode, SetMarkdownOpensAs, is_markdown};
use crate::files::tests::{checkout, editing, press, shell_showing, typed, with_shell};
use crate::files::{At, Place};
use crate::settings;
use crate::shell::Shell;
use crate::test_support::{draw, write};
use crate::theme;

const FLOW: &str = "flowchart LR\n    Code --> Preview\n";

#[test]
fn markdown_is_told_by_the_file_name() {
    for name in ["README.md", "notes.markdown", "page.mdx", "UPPER.MD"] {
        assert!(is_markdown(Path::new(name)), "{name}");
    }
    for name in ["lib.rs", "md", "README", "notes.md.bak"] {
        assert!(!is_markdown(Path::new(name)), "{name}");
    }
}

#[test]
fn a_fenced_block_is_mermaid_by_its_language() {
    assert!(is_mermaid(Some("mermaid")));
    assert!(is_mermaid(Some(" Mermaid ")));
    assert!(!is_mermaid(Some("rust")));
    assert!(!is_mermaid(None));
}

/// The app's settings and theme, as a window has them.
fn init(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(settings::init);
}

/// The looks a diagram's drawn in, in the app's light theme and its dark.
fn looks(cx: &mut TestAppContext) -> [Look; 2] {
    init(cx);
    [gpui_kit::component::ThemeMode::Light, gpui_kit::component::ThemeMode::Dark].map(|mode| {
        cx.update(|cx| {
            gpui_kit::component::Theme::change(mode, None, cx);
            look(cx)
        })
    })
}

/// The look a diagram's drawn in, in the theme now.
fn look(cx: &App) -> Look {
    let dark = gpui_kit::component::ActiveTheme::theme(cx).mode.is_dark();
    Look::of(theme::palette(cx), Card::of(cx).paper, dark)
}

/// Shows the checkout's file at `path` as it reads.
fn previewed(window: AnyWindowHandle, shell: &Entity<Shell>, path: &str, cx: &mut TestAppContext) {
    with_shell(window, shell, cx, |shell, window, cx| {
        shell.view_file(Place::file(path), window, cx);
        let file = shell.viewing().unwrap();
        file.update(cx, |file, cx| file.show_as(Mode::Preview, window, cx));
    });
    draw(window, cx);
}

/// Shows the checkout's file at `path` as it reads in a window 1600 wide and `height` tall,
/// laid out at that size.
fn laid_out(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    path: &str,
    height: f32,
    cx: &mut TestAppContext,
) -> gpui_kit::VisualTestContext {
    previewed(window, shell, path, cx);
    let mut visual = gpui_kit::VisualTestContext::from_window(window, cx);
    visual.simulate_resize(gpui_kit::size(gpui_kit::px(1600.), gpui_kit::px(height)));
    for _ in 0..3 {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
    }
    visual
}

/// What `read` makes of the preview the shown tab shows.
fn preview<R>(
    shell: &Entity<Shell>,
    cx: &TestAppContext,
    read: impl FnOnce(&MarkdownPreview, &App) -> R,
) -> R {
    cx.read(|cx| {
        let file = shell.read(cx).viewing().unwrap();
        read(file.read(cx).shown_preview().expect("a preview shown").read(cx), cx)
    })
}

/// How the shown tab shows its file.
fn mode(shell: &Entity<Shell>, cx: &TestAppContext) -> Mode {
    cx.read(|cx| shell.read(cx).viewing().unwrap().read(cx).mode())
}

#[gpui_kit::test]
fn a_diagram_is_drawn_in_the_palette_s_colours(cx: &mut TestAppContext) {
    let [light, dark] = looks(cx);
    assert_ne!(light, dark);
    for look in [light, dark] {
        let drawn = diagram::draw(FLOW, &look).expect("drawn");
        assert!(drawn.width > 0. && drawn.height > 0.);
        // Its labels as SVG text GPUI paints, not HTML it can't.
        assert!(!drawn.text.contains("<foreignObject"), "no HTML labels");
        assert!(drawn.text.contains("Preview"));
        // The nodes' fill is the theme's, not Mermaid's lavender.
        assert!(!drawn.text.to_ascii_lowercase().contains("#ececff"), "Mermaid's own colours");
        // The same source in the same look draws the same picture.
        assert_eq!(diagram::draw(FLOW, &look).unwrap().text, drawn.text);
    }
}

#[gpui_kit::test]
#[expect(clippy::float_cmp, reason = "solid is a whole 1, not near it")]
fn a_diagram_s_card_is_a_code_block_s(cx: &mut TestAppContext) {
    init(cx);
    let card = cx.update(|cx| Card::of(cx));
    // Solid, as its labels' boxes are painted in it.
    assert_eq!(card.paper.a, 1.);
    assert!(card.corners.top_left.is_some(), "rounded as a code block is");
}

#[gpui_kit::test]
fn a_diagram_mermaid_cant_draw_says_why(cx: &mut TestAppContext) {
    let [look, _] = looks(cx);
    let why = diagram::draw("flowchart LR\n    A --> \n    B -->>>> {{", &look).expect_err("not drawn");
    assert!(!why.is_empty());
    assert!(diagram::draw("not a diagram at all", &look).is_err());
}

/// Whether what the shown tab shows has the keyboard.
fn has_focus(window: AnyWindowHandle, shell: &Entity<Shell>, cx: &mut TestAppContext) -> bool {
    cx.update_window(window, |_, window, cx| {
        let file = shell.read(cx).viewing().unwrap();
        file.read(cx).has_focus(window, cx)
    })
    .unwrap()
}

#[gpui_kit::test]
fn a_markdown_file_flips_between_its_code_and_how_it_reads(cx: &mut TestAppContext) {
    let root = checkout("markdown-flip");
    let (window, shell, _) = shell_showing(&root, cx);
    draw(window, cx);
    // README.md opens as it reads, with the keyboard.
    assert_eq!(mode(&shell, cx), Mode::Preview);
    assert_eq!(preview(&shell, cx, MarkdownPreview::text).trim(), "Test");
    assert!(has_focus(window, &shell, cx), "the keyboard goes with what's shown");
    // ⌘⇧V shows its code, and back.
    for shown in [Mode::Code, Mode::Preview] {
        press(window, "cmd-shift-v", cx);
        draw(window, cx);
        assert_eq!(mode(&shell, cx), shown);
        assert!(has_focus(window, &shell, cx), "the keyboard goes with what's shown");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_markdown_file_opens_as_set_and_as_code_at_a_place(cx: &mut TestAppContext) {
    let root = checkout("markdown-opens");
    for name in ["one", "two", "three"] {
        write(&root, &format!("docs/{name}.md"), b"# A page\n\nWords.\n");
    }
    let (window, shell, _) = shell_showing(&root, cx);
    let open = |place: Place, cx: &mut TestAppContext| {
        with_shell(window, &shell, cx, |shell, window, cx| shell.view_file(place, window, cx));
        draw(window, cx);
        mode(&shell, cx)
    };
    // Set to open as code, from the Editor settings' menu.
    cx.dispatch_action(window, SetMarkdownOpensAs(Mode::Code));
    assert_eq!(cx.read(|cx| settings::get(cx).markdown_opens_as), Mode::Code);
    assert_eq!(open(Place::file("docs/one.md"), cx), Mode::Code);
    // A file open already stays as it's shown.
    assert_eq!(open(Place::file("README.md"), cx), Mode::Preview);
    // As it reads again; opened at a place in its text, as a search result is, as code.
    cx.dispatch_action(window, SetMarkdownOpensAs(Mode::Preview));
    assert_eq!(open(Place::file("docs/two.md"), cx), Mode::Preview);
    let at_a_line = Place { path: "docs/three.md".into(), at: Some(At::Line(2)) };
    assert_eq!(open(at_a_line, cx), Mode::Code);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_place_in_a_file_shown_as_it_reads_shows_its_code(cx: &mut TestAppContext) {
    let root = checkout("markdown-reveal");
    let (window, shell, _) = shell_showing(&root, cx);
    previewed(window, &shell, "README.md", cx);
    assert_eq!(mode(&shell, cx), Mode::Preview);
    // Gone to, as a definition or a search result is.
    with_shell(window, &shell, cx, |shell, window, cx| {
        shell.view_file(Place { path: "README.md".into(), at: Some(At::Line(0)) }, window, cx);
    });
    draw(window, cx);
    assert_eq!(mode(&shell, cx), Mode::Code);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_preview_shows_the_text_as_it_s_edited(cx: &mut TestAppContext) {
    let root = checkout("markdown-edits");
    let (window, shell, _) = shell_showing(&root, cx);
    let (_, editor, _) = editing(window, &shell, "README.md", At::Bytes(7..7), cx);
    typed(window, &editor, "\nUnsaved words.\n", cx);
    previewed(window, &shell, "README.md", cx);
    let text = |cx: &TestAppContext| preview(&shell, cx, MarkdownPreview::text);
    // What's in the editor, saved or not.
    assert!(text(cx).contains("Unsaved words."), "{}", text(cx));
    // Typed into while it's shown as it reads, by an agent say, it reads again.
    typed(window, &editor, "More words.", cx);
    assert!(text(cx).contains("More words."), "{}", text(cx));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn only_a_markdown_file_flips(cx: &mut TestAppContext) {
    let root = checkout("markdown-only");
    let (window, shell, _) = shell_showing(&root, cx);
    let _ = editing(window, &shell, "src/lib.rs", At::Line(0), cx);
    press(window, "cmd-shift-v", cx);
    draw(window, cx);
    assert_eq!(mode(&shell, cx), Mode::Code);
    let previewed = cx.read(|cx| shell.read(cx).viewing().unwrap().read(cx).shown_preview().is_some());
    assert!(!previewed);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_preview_keeps_the_diagrams_it_shows_and_lets_go_of_the_rest(cx: &mut TestAppContext) {
    let root = checkout("markdown-mermaid");
    write(&root, "docs/flow.md", format!("# Flow\n\n```mermaid\n{FLOW}```\n").as_bytes());
    let (window, shell, _) = shell_showing(&root, cx);
    previewed(window, &shell, "docs/flow.md", cx);
    // Asked for as it's drawn, drawn off the main thread, and drawn again once it is.
    draw(window, cx);
    // A fenced block's source, as markdown has it, ends without its last newline.
    let key = (SharedString::from(FLOW.trim_end()), cx.update(|cx| look(cx)));
    let drawn = cx.update(|cx| cx.fetch_asset::<Diagram>(&key));
    assert!(matches!(drawn, Some(Ok(_))), "{drawn:?}");
    // Its source is what it reads as, to copy.
    let text = preview(&shell, cx, MarkdownPreview::text);
    assert!(text.contains("Code --> Preview"), "{text}");
    // Drawn again and again, scrolled say, it's kept.
    for _ in 0..3 {
        press(window, "down", cx);
        draw(window, cx);
    }
    assert!(cx.read(|cx| cx.has_asset::<Diagram>(&key)), "kept while it's shown");
    assert_eq!(cx.read(diagram::shown), 1);
    // Its source changed, the diagram it drew goes.
    let (_, editor, _) = editing(window, &shell, "docs/flow.md", At::Line(4), cx);
    typed(window, &editor, "    Preview --> Code\n", cx);
    previewed(window, &shell, "docs/flow.md", cx);
    draw(window, cx);
    let text = preview(&shell, cx, MarkdownPreview::text);
    assert!(text.contains("Preview --> Code"), "{text}");
    assert!(!cx.read(|cx| cx.has_asset::<Diagram>(&key)), "let go once it's not shown");
    assert_eq!(cx.read(diagram::shown), 1, "the one it shows now");
    // Closed, it lets go of all it showed.
    with_shell(window, &shell, cx, |shell, _, cx| shell.forget_files(&root, cx));
    draw(window, cx);
    assert_eq!(cx.read(diagram::shown), 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
#[expect(clippy::float_cmp, reason = "scrolled back is scrolled to where it was, not near it")]
fn a_preview_scrolls_by_the_keys_a_reader_scrolls_by(cx: &mut TestAppContext) {
    let root = checkout("markdown-scroll");
    let long = "A paragraph.\n\n".repeat(200);
    write(&root, "docs/long.md", long.as_bytes());
    let (window, shell, _) = shell_showing(&root, cx);
    // Shown as it reads before it's read, as a file is just opened: the keyboard stays with it.
    previewed(window, &shell, "docs/long.md", cx);
    let scrolled = |keys: &str, cx: &mut TestAppContext| {
        press(window, keys, cx);
        draw(window, cx);
        preview(&shell, cx, |preview, _| preview.scrolled())
    };
    let line = scrolled("down", cx);
    assert!(line > 0., "an arrow scrolls a line");
    let page = scrolled("space", cx);
    assert!(page > line + 100., "Space scrolls a page: {page}");
    assert_eq!(scrolled("shift-space", cx), line, "and back");
    let end = scrolled("cmd-down", cx);
    assert!(end > page);
    assert_eq!(scrolled("pagedown", cx), end, "no further than the end");
    assert_eq!(scrolled("home", cx), 0.);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_diagram_is_as_big_as_it_s_drawn_or_as_the_column_lets_it_be(cx: &mut TestAppContext) {
    let root = checkout("markdown-size");
    // Tall and narrow, as a flowchart top to bottom is, below some of everything a page has;
    // and one wider than the column.
    let wide: String = std::iter::once("flowchart LR".to_owned())
        .chain((0..12).map(|at| format!("    S{at}[A step along] --> S{}[A step along]", at + 1)))
        .collect::<Vec<_>>()
        .join("\n");
    for (name, source) in [("tall", TALL), ("wide", wide.as_str())] {
        let text = format!("{PAGE}\n```mermaid\n{source}\n```\n");
        write(&root, &format!("docs/{name}.md"), text.as_bytes());
    }
    let (window, shell, _) = shell_showing(&root, cx);
    let look = cx.update(|cx| look(cx));
    // The diagram's size, and its card's, shown in a window this wide.
    let shown = |name: &str, cx: &mut TestAppContext| {
        let mut visual = laid_out(window, &shell, &format!("docs/{name}.md"), 1000., cx);
        let mut size = |at| {
            let size = visual.debug_bounds(at).unwrap_or_else(|| panic!("no {at}")).size;
            (size.width.as_f32(), size.height.as_f32())
        };
        (size("mermaid-diagram"), size("mermaid-card"))
    };
    let near = |a: f32, b: f32| (a - b).abs() < 1.;
    // The card's as tall as the diagram and the room around it, its border too.
    let around = 2. * PADDING + 2.;
    let drawn = diagram::draw(TALL, &look).unwrap();
    let ((width, height), (_, card)) = shown("tall", cx);
    let said = format!("{width}x{height} on {card}, drawn {}x{}", drawn.width, drawn.height);
    assert!(near(width, drawn.width) && near(height, drawn.height), "{said}");
    assert!(near(card, height + around), "{said}");
    // Narrowed to the column, and as much shorter.
    let drawn = diagram::draw(&wide, &look).unwrap();
    let ((width, height), (_, card)) = shown("wide", cx);
    let said = format!("{width}x{height} on {card}, drawn {}x{}", drawn.width, drawn.height);
    assert!(width < drawn.width - 50., "{said}");
    assert!(near(height, width * drawn.height / drawn.width), "{said}");
    assert!(near(card, height + around), "{said}");
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn a_table_wider_than_the_column_scrolls_sideways_rather_than_squeezing(cx: &mut TestAppContext) {
    let root = checkout("markdown-table");
    // Ten columns, three rows: of a dozen words a cell, as a spreadsheet pasted in has, far
    // wider than the column; and of a word, which fits it.
    let table = |cell: &str| {
        let row = |cell: &str| format!("|{}\n", format!(" {cell} |").repeat(10));
        row("Column") + &row("---") + &row(cell).repeat(3)
    };
    let words = "Hong Kong and Singapore with USD, EUR and GBP local in two days";
    write(&root, "docs/wide.md", table(words).as_bytes());
    write(&root, "docs/narrow.md", table("Word").as_bytes());
    let (window, shell, _) = shell_showing(&root, cx);
    // How tall the file at `path` is, shown as it reads in a window too short for either.
    let tall = |path: &str, cx: &mut TestAppContext| {
        laid_out(window, &shell, path, 300., cx);
        preview(&shell, cx, |preview, _| preview.content_height())
    };
    let (wide, narrow) = (tall("docs/wide.md", cx), tall("docs/narrow.md", cx));
    // Its columns kept wide enough to read, a row of a dozen words is a line or two taller than
    // one of a word; squeezed into the column, a word or two a line, it was four times as tall.
    assert!(wide < 2. * narrow, "{wide} for the wide table, {narrow} for the narrow one");
    let _ = std::fs::remove_dir_all(&root);
}

/// How long a long file shown as it reads takes to draw, a page down at a time: a measure to
/// run by hand, with `--ignored --nocapture`.
#[gpui_kit::test]
#[ignore = "a measure, not a check"]
fn how_long_a_long_file_takes_to_draw_as_it_scrolls(cx: &mut TestAppContext) {
    let root = checkout("markdown-long");
    let section = "## A section\n\nWords in a paragraph, **some bold**, `some code` and a [link](x.md).\n\n- One\n- Two\n\n```rust\nfn main() {}\n```\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n";
    let long = section.repeat(250);
    write(&root, "docs/long.md", long.as_bytes());
    let (window, shell, _) = shell_showing(&root, cx);
    previewed(window, &shell, "docs/long.md", cx);
    draw(window, cx);
    // A page each, as Space scrolls: an arrow held is repeated by the app, not by presses.
    let started = std::time::Instant::now();
    for _ in 0..20 {
        press(window, "space", cx);
        draw(window, cx);
    }
    let elapsed = started.elapsed() / 20;
    let scrolled = preview(&shell, cx, |preview, _| preview.scrolled());
    assert!(scrolled > 10_000., "{scrolled}");
    eprintln!("{} lines, scrolled {scrolled}: {elapsed:?} a page", long.lines().count());
    let _ = std::fs::remove_dir_all(&root);
}

/// Some of everything a page has.
const PAGE: &str = "# A page

Words **in bold**, *in italics*, `as code` and [a link](docs/guide.md).

> A quote.

- One
- Two
  - Under it

| A | B |
|---|---|
| 1 | `2` |

```rust
fn main() {}
```
";

/// A flowchart top to bottom, with labels on its arrows.
const TALL: &str = "flowchart TD
    A[Open a markdown file] --> B{Preview?}
    B -->|Yes| C[Render with TextView]
    B -->|No| D[Edit the source]
    C --> E[(Mermaid SVG)]
    D --> B";
