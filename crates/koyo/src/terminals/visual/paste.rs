//! An image pasted into a terminal (`docs/terminals.md` 7.3): ⌘V with only an image on the
//! clipboard gives a shell a path it reads back whole, to a file holding the image, as a
//! program that takes images reads it.

use gpui_kit::{ClipboardEntry, ClipboardItem, Image, ImageFormat};

use super::drive::{press, terminal, text, wait};
use crate::visual::{Drawn, Result};

const IMAGE: &[u8] = b"\x89PNG\r\n\x1a\nthe bytes a clipboard held";

/// `text` without the breaks its rows make in a long path.
fn unwrapped(text: &str) -> String {
    text.replace('\n', "")
}

pub(super) fn paste(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let image = Image::from_bytes(ImageFormat::Png, IMAGE.to_vec());
    let clipboard = ClipboardItem {
        entries: vec![ClipboardEntry::Image(image)],
    };
    drawn.cx.update(|cx| cx.write_to_clipboard(clipboard));
    // Held no longer than this: the check later waits for the tab's view to go.
    let view = terminal(drawn)?;
    let command = "printf 'pasted=[%s]\\n' ";
    (drawn.cx).update(|cx| view.update(cx, |view, _| view.paste_text(command.into())));
    drop(view);
    wait(drawn, "the command before the path", |text| text.contains("pasted=[%s]"))?;
    press(drawn, "cmd-v")?;
    wait(drawn, "the image's path at the prompt", |text| unwrapped(text).contains(".png"))?;
    press(drawn, "enter")?;
    wait(drawn, "the shell to read the path back", |text| {
        unwrapped(text).contains("pasted=[/")
    })?;
    let shown = unwrapped(&text(drawn)?);
    let path = (shown.rsplit("pasted=[").next())
        .and_then(|rest| rest.split(']').next())
        .ok_or("the shell printed no path")?;
    let saved = std::fs::read(path).map_err(|error| format!("{path}: {error}"))?;
    if saved != IMAGE {
        return Err(format!("{path} doesn't hold the image: {} bytes", saved.len()));
    }
    said.push(format!("an image pasted as {path}, whose bytes are the image's"));
    Ok(())
}
