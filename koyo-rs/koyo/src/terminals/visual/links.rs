//! Paths a program prints, opened from the terminal: with ⌘ held over one it's underlined, and
//! a ⌘-click opens it in a tab as a click in the tree would: a picture shown as itself, and a
//! file at the line printed after its path, as a compiler prints a place.

use gpui_kit::{Entity, Modifiers, MouseMoveEvent, Pixels, PlatformInput, Point};
use image::{Rgba, RgbaImage};

use super::drive::{cell, command_click, press, send, text, type_line, wait, wait_until};
use crate::files::FilePane;
use crate::visual::{Drawn, Result, checkout};

pub fn links(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let repo = checkout()?.join(super::OUT).join("repo");
    let picture = RgbaImage::from_fn(240, 160, |x, y| {
        Rgba([u8::try_from(x).unwrap_or(255), u8::try_from(y).unwrap_or(255), 200, 255])
    });
    (picture.save(repo.join("shot.png"))).map_err(|error| error.to_string())?;
    let source = repo.join("src");
    std::fs::create_dir_all(&source).map_err(|error| error.to_string())?;
    std::fs::write(source.join("main.rs"), "fn main() {\n    println!(\"hi\");\n}\n")
        .map_err(|error| error.to_string())?;

    type_line(drawn, "clear; printf 'look at shot.png\\nfailed at src/main.rs:2:5\\n'")?;
    let printed = |text: &str| text.lines().any(|line| line == "failed at src/main.rs:2:5");
    wait(drawn, "the paths printed", printed)?;

    // ⌘ held over the picture's path underlines it.
    let on_picture = place(drawn, "look at shot.png", "shot.png")?;
    send(
        drawn,
        PlatformInput::MouseMove(MouseMoveEvent {
            position: on_picture,
            pressed_button: None,
            modifiers: Modifiers::command(),
        }),
    )?;
    drawn.pause()?;
    said.push(format!("{}", drawn.save("link-underlined.png")?.display()));

    // ⌘-clicked, it opens in a tab, shown as a picture.
    command_click(drawn, on_picture)?;
    let file = opened(drawn, "shot.png")?;
    if drawn.cx.read(|cx| file.read(cx).shows_text()) {
        return Err("shot.png opened as text, not as a picture".into());
    }
    wait_until(drawn, "the picture to be drawn", |drawn| {
        Ok(drawn.cx.read(|cx| file.read(cx).viewer().is_some_and(|viewer| viewer.read(cx).is_drawn())))
    })?;
    said.push(format!("{}", drawn.save("link-picture.png")?.display()));
    press(drawn, "cmd-w")?;

    // A path and a line, as a compiler prints them, open the file at that line.
    wait(drawn, "the terminal again", printed)?;
    let on_place = place(drawn, "failed at src/main.rs:2:5", "src/main.rs")?;
    command_click(drawn, on_place)?;
    let file = opened(drawn, "src/main.rs")?;
    if !drawn.cx.read(|cx| file.read(cx).shows_text()) {
        return Err("src/main.rs didn't open as text".into());
    }
    drawn.pause()?;
    said.push(format!("{}", drawn.save("link-line.png")?.display()));
    said.push("⌘-clicked paths opened shot.png as a picture and src/main.rs at line 2".into());
    press(drawn, "cmd-w")?;
    wait(drawn, "the terminal after the links", printed)
}

/// The middle of the cell `needle` starts at, in the first row of the terminal reading `line`.
fn place(drawn: &mut Drawn, line: &str, needle: &str) -> Result<Point<Pixels>> {
    let text = text(drawn)?;
    let row = (text.lines()).position(|row| row == line);
    let row = row.ok_or_else(|| format!("no row reads {line:?}"))?;
    let col = line.find(needle).ok_or_else(|| format!("no {needle:?} in {line:?}"))?;
    let (col, row) = (u16::try_from(col), u16::try_from(row));
    cell(drawn, col.map_err(|e| e.to_string())?, row.map_err(|e| e.to_string())?)
}

/// The file the shown tab shows, once it's the one at `relative` in the scratch repository.
fn opened(drawn: &mut Drawn, relative: &str) -> Result<Entity<FilePane>> {
    let shell = drawn.shell.clone();
    let shown = move |drawn: &mut Drawn| {
        drawn.cx.read(|cx| {
            let file = shell.read(cx).viewing()?;
            (file.read(cx).path.ends_with(relative)).then_some(file)
        })
    };
    let what = format!("{relative} to open in a tab");
    wait_until(drawn, &what, |drawn| Ok(shown(drawn).is_some()))?;
    shown(drawn).ok_or(what)
}
