//! How big what a file shown as it reads is, laid out at a window's size: a diagram as big as
//! it's drawn or as the column lets it be, its card around it, and a table wider than the column
//! scrolling sideways rather than squeezed into it.

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};

use super::super::diagram;
use super::super::mermaid::PADDING;
use super::{look, preview, previewed};
use crate::files::tests::{checkout, shell_showing};
use crate::shell::Shell;
use crate::test_support::write;

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
            let size = visual
                .debug_bounds(at)
                .unwrap_or_else(|| panic!("no {at}"))
                .size;
            (size.width.as_f32(), size.height.as_f32())
        };
        (size("mermaid-diagram"), size("mermaid-card"))
    };
    let near = |a: f32, b: f32| (a - b).abs() < 1.;
    // The card's as tall as the diagram and the room around it, its border too.
    let around = 2. * PADDING + 2.;
    let drawn = diagram::draw(TALL, &look).unwrap();
    let ((width, height), (_, card)) = shown("tall", cx);
    let said = format!(
        "{width}x{height} on {card}, drawn {}x{}",
        drawn.width, drawn.height
    );
    assert!(
        near(width, drawn.width) && near(height, drawn.height),
        "{said}"
    );
    assert!(near(card, height + around), "{said}");
    // Narrowed to the column, and as much shorter.
    let drawn = diagram::draw(&wide, &look).unwrap();
    let ((width, height), (_, card)) = shown("wide", cx);
    let said = format!(
        "{width}x{height} on {card}, drawn {}x{}",
        drawn.width, drawn.height
    );
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
    assert!(
        wide < 2. * narrow,
        "{wide} for the wide table, {narrow} for the narrow one"
    );
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
