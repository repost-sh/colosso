use std::os::unix::fs::PermissionsExt as _;

use gpui_kit::ExternalPaths;

use super::*;

/// An empty folder of the test's own under the checkout's `target`.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/test-paste")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png() -> Image {
    Image::from_bytes(ImageFormat::Png, b"\x89PNG not decoded".to_vec())
}

fn item(entries: Vec<ClipboardEntry>) -> ClipboardItem {
    ClipboardItem { entries }
}

fn files(paths: &[&str]) -> ClipboardEntry {
    let paths = paths.iter().map(|path| PathBuf::from(*path)).collect();
    ClipboardEntry::ExternalPaths(ExternalPaths(paths))
}

#[test]
fn a_copied_file_pastes_its_path_and_not_the_name_finder_copies_with_it() {
    let copied = item(vec![
        files(&["/Users/me/My Shots/one.png", "/tmp/two.png"]),
        ClipboardItem::new_string("one.png".to_owned()).entries()[0].clone(),
    ]);
    assert_eq!(
        pasted(copied),
        Some(Pasted::Text(r"/Users/me/My\ Shots/one.png /tmp/two.png".to_owned()))
    );
}

#[test]
fn text_pastes_as_text_and_an_image_alone_as_itself() {
    let text = ClipboardItem::new_string("echo hi".to_owned());
    assert_eq!(pasted(text), Some(Pasted::Text("echo hi".to_owned())));
    let image = item(vec![ClipboardEntry::Image(png())]);
    assert_eq!(pasted(image), Some(Pasted::Image(png())));
    assert_eq!(pasted(item(Vec::new())), None);
}

#[test]
fn a_path_is_quoted_so_a_shell_reads_it_back() {
    let of = |path: &str| quoted(Path::new(path));
    assert_eq!(of("/tmp/koyo-paste-1-2-3.png"), "/tmp/koyo-paste-1-2-3.png");
    assert_eq!(of("/tmp/a b(1)$'.png"), r"/tmp/a\ b\(1\)\$\'.png");
    assert_eq!(of("/tmp/日本語.png"), "/tmp/日本語.png");
    assert_eq!(of("/tmp/a\nrm 'b'"), "'/tmp/a\nrm '\\''b'\\'''");
}

#[test]
fn an_image_is_saved_whole_in_a_new_private_file() {
    let dir = scratch("saved");
    let image = png();
    let first = save_in(&dir, &image).unwrap();
    let second = save_in(&dir, &image).unwrap();
    assert_ne!(first, second);
    assert_eq!(first.extension().unwrap(), "png");
    assert_eq!(std::fs::read(&first).unwrap(), image.bytes);
    let mode = std::fs::metadata(&first).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_tiff_becomes_a_png_programs_can_read() {
    let dir = scratch("tiff");
    let picture = image::RgbaImage::from_pixel(3, 2, image::Rgba([200, 30, 40, 255]));
    let mut tiff = Cursor::new(Vec::new());
    picture.write_to(&mut tiff, image::ImageFormat::Tiff).unwrap();
    let saved = save_in(&dir, &Image::from_bytes(ImageFormat::Tiff, tiff.into_inner())).unwrap();
    assert_eq!(saved.extension().unwrap(), "png");
    let bytes = std::fs::read(&saved).unwrap();
    let read = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap();
    assert_eq!(read.to_rgba8(), picture);
}

#[test]
fn a_tiff_that_cant_be_read_is_saved_as_it_came() {
    let dir = scratch("broken");
    let broken = Image::from_bytes(ImageFormat::Tiff, b"not a tiff".to_vec());
    let saved = save_in(&dir, &broken).unwrap();
    assert_eq!(saved.extension().unwrap(), "tiff");
    assert_eq!(std::fs::read(&saved).unwrap(), broken.bytes);
}
