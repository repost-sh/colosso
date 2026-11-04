//! What ⌘V puts in a terminal when the clipboard holds more than text. A terminal only takes
//! text, so a copied file becomes its path, and a copied image a file of its own and that
//! file's path, which programs that take images (Claude Code among them) read as the image.

use std::io::{self, Cursor, Write as _};
use std::path::{Path, PathBuf};

use gpui_kit::{ClipboardEntry, ClipboardItem, Image, ImageFormat};

/// What a clipboard item pastes as.
#[derive(Debug, PartialEq)]
pub(crate) enum Pasted {
    Text(String),
    /// An image, to [`save`] first, as it can be large.
    Image(Image),
}

/// What `item` pastes as: its files' paths, its text, or its image, the order macOS's
/// clipboard reads them in. A copied file's text is only its name, so its path comes first.
pub(crate) fn pasted(item: ClipboardItem) -> Option<Pasted> {
    let paths = item.entries().iter().find_map(|entry| match entry {
        ClipboardEntry::ExternalPaths(paths) => Some(paths.paths()),
        _ => None,
    });
    if let Some(paths) = paths.filter(|paths| !paths.is_empty()) {
        let quoted: Vec<String> = paths.iter().map(|path| quoted(path)).collect();
        return Some(Pasted::Text(quoted.join(" ")));
    }
    if let Some(text) = item.text() {
        return Some(Pasted::Text(text));
    }
    item.into_entries().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(Pasted::Image(image)),
        _ => None,
    })
}

/// `path` as a shell reads it back: special characters escaped, as a terminal does for a file
/// dropped on it, or the whole of it in quotes when one can't be, so a line break in a name
/// never runs what follows it.
fn quoted(path: &Path) -> String {
    let path = path.to_string_lossy();
    if path.chars().any(char::is_control) {
        return format!("'{}'", path.replace('\'', r"'\''"));
    }
    let mut quoted = String::with_capacity(path.len());
    for c in path.chars() {
        if !(c.is_alphanumeric() || "/._-+=,:@%".contains(c)) {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted
}

/// Saves `image` to a new file in the temporary folder, which macOS clears of what's unused
/// for days, and gives its path as it's pasted.
pub(crate) fn save(image: &Image) -> io::Result<String> {
    save_in(&std::env::temp_dir(), image).map(|path| quoted(&path))
}

/// A new file only this user can read, as a clipboard's image can be private.
fn save_in(dir: &Path, image: &Image) -> io::Result<PathBuf> {
    let converted = attachable(image);
    let (extension, bytes) = match &converted {
        Some(png) => (ImageFormat::Png.extension(), png.as_slice()),
        None => (image.format.extension(), image.bytes.as_slice()),
    };
    let mut file = tempfile::Builder::new()
        .prefix("koyo-paste-")
        .suffix(&format!(".{extension}"))
        .tempfile_in(dir)?;
    file.write_all(bytes)?;
    Ok(file.keep()?.1)
}

/// `image` as a PNG when programs wouldn't read it otherwise: macOS apps often copy TIFF, which
/// Claude Code and the models behind such programs don't take. `None` to save it as it is.
fn attachable(image: &Image) -> Option<Vec<u8>> {
    if image.format != ImageFormat::Tiff {
        return None;
    }
    let converted = image::load_from_memory_with_format(&image.bytes, image::ImageFormat::Tiff)
        .and_then(|decoded| {
            let mut png = Cursor::new(Vec::new());
            decoded.write_to(&mut png, image::ImageFormat::Png)?;
            Ok(png.into_inner())
        });
    converted
        .inspect_err(|error| log::warn!("a pasted TIFF stays one: {error}"))
        .ok()
}

#[cfg(test)]
mod tests;
