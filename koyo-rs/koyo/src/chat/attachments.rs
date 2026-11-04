//! What a message attaches (`docs/chat.md` 4.3.4): the app keeps each in the host's attachments
//! folder, under a name of its own, and the message names it there. A picture goes to the model as
//! an image, so it's made to fit what the model takes; any other file is only copied, and the agent
//! is told its path. Keeping one reads and writes, so it's for off the main thread. The tiles that
//! show them are made here too, each picture drawn from a small copy of it.

use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, DirBuilder, OpenOptions};
use std::future::Future;
use std::io::{self, Cursor, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::attachment::{
    Attachment as Tile, AttachmentContent, AttachmentMedia, AttachmentTitle,
};
use gpui_kit::{
    App, Asset, Axis, ClipboardEntry, ClipboardItem, ElementId, ImageCacheError, ParentElement as _,
    RenderImage, SharedString, Styled as _, Window, px,
};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, Frame, ImageFormat, ImageReader, RgbaImage};
use koyo_agent::client;
use koyo_agent::protocol::ids::MessageId;
use koyo_agent::protocol::model::Attachment;

use crate::files::{file_icon, file_name};
use crate::settings;
use crate::shell::icon;
use crate::theme::Palette;

/// The longest side a picture goes to the model with.
const MOST_SIDE: u32 = 2048;
/// The most a picture goes to the model as.
const MOST_BYTES: usize = 5 * 1024 * 1024;
/// How a picture too big as a PNG is made a JPEG, from the best that may do.
const JPEG_QUALITIES: [u8; 3] = [85, 60, 35];
/// What a pasted picture is called, having no name.
const PASTED: &str = "Pasted image";
/// What a file is said to be that isn't a picture, which is all the host needs to know of it.
const SOME_FILE: &str = "application/octet-stream";
/// The side of the small copy of a picture tiles draw: the biggest tile's, on a Retina screen.
const THUMBNAIL: u32 = 320;
/// How wide a tile that names a file gets.
const FILE_TILE: f32 = 180.;

/// Why something can't be attached, in words that follow "Can't attach this:".
type Failed = Box<dyn Error + Send + Sync>;

/// Where they're kept: the host's folder. Tests read and write none of a person's.
pub(super) fn folder() -> Option<PathBuf> {
    let data = settings::data_dir().filter(|_| !cfg!(test))?;
    Some(client::host_dir(&data).root().join("attachments"))
}

/// What's to be attached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Picture(Vec<u8>),
    File(PathBuf),
}

impl Source {
    /// What it goes by.
    pub(super) fn name(&self) -> SharedString {
        match self {
            Source::Picture(_) => PASTED.into(),
            Source::File(path) => file_name(path).into(),
        }
    }
}

/// What of a paste is to be attached rather than typed.
pub(super) fn sources_of(pasted: &ClipboardItem) -> Vec<Source> {
    (pasted.entries().iter())
        .flat_map(|entry| match entry {
            ClipboardEntry::String(_) => Vec::new(),
            ClipboardEntry::Image(image) => vec![Source::Picture(image.bytes.clone())],
            ClipboardEntry::ExternalPaths(paths) => {
                paths.paths().iter().cloned().map(Source::File).collect()
            }
        })
        .collect()
}

/// Keeps `source` in `folder`, or says why it can't.
pub(super) fn keep(source: Source, folder: &Path) -> Result<Attachment, String> {
    let name = source.name().to_string();
    match kept(source, folder) {
        Ok((file, mime)) => Ok(Attachment { file, name, mime }),
        Err(why) => Err(format!("Can't attach {name}: {why}.")),
    }
}

/// What `source` is kept as in `folder`, and its type.
fn kept(source: Source, folder: &Path) -> Result<(String, String), Failed> {
    DirBuilder::new().recursive(true).mode(0o700).create(folder)?;
    let path = match source {
        Source::Picture(bytes) => return keep_picture(bytes, folder),
        Source::File(path) => path,
    };
    if path.is_dir() {
        return Err("it's a folder".into());
    }
    if is_picture(&path) {
        return keep_picture(fs::read(&path)?, folder);
    }
    let extension = path.extension().and_then(OsStr::to_str).unwrap_or("file");
    let file = fresh_name(extension);
    let copy = folder.join(&file);
    fs::copy(&path, &copy)?;
    fs::set_permissions(&copy, fs::Permissions::from_mode(0o600))?;
    Ok((file, SOME_FILE.to_owned()))
}

/// Whether the file at `path` is a picture this can read, whatever it's called.
fn is_picture(path: &Path) -> bool {
    let reader = ImageReader::open(path).and_then(ImageReader::with_guessed_format);
    reader.is_ok_and(|reader| reader.format().is_some_and(|format| format.reading_enabled()))
}

fn keep_picture(bytes: Vec<u8>, folder: &Path) -> Result<(String, String), Failed> {
    let (bytes, format) = fitted(bytes)?;
    let extension = format.extensions_str().first().copied().unwrap_or("image");
    let file = fresh_name(extension);
    let mut written =
        OpenOptions::new().write(true).create_new(true).mode(0o600).open(folder.join(&file))?;
    written.write_all(&bytes)?;
    Ok((file, format.to_mime_type().to_owned()))
}

/// `bytes` as the model takes a picture: as they are if it can, or else smaller and in a format it
/// knows. A photo stays a JPEG, as a PNG of it would only be bigger; anything else is a PNG for as
/// long as that fits.
fn fitted(bytes: Vec<u8>) -> Result<(Vec<u8>, ImageFormat), Failed> {
    let reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
    let format = reader.format().ok_or("it isn't a picture")?;
    let (width, height) = reader.into_dimensions()?;
    let too_long = width.max(height) > MOST_SIDE;
    if model_takes(format) && !too_long && bytes.len() <= MOST_BYTES {
        return Ok((bytes, format));
    }
    let mut picture = image::load_from_memory_with_format(&bytes, format)?;
    drop(bytes);
    if too_long {
        picture = picture.resize(MOST_SIDE, MOST_SIDE, FilterType::Lanczos3);
    }
    if format != ImageFormat::Jpeg {
        let mut png = Vec::new();
        picture.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)?;
        if png.len() <= MOST_BYTES {
            return Ok((png, ImageFormat::Png));
        }
    }
    Ok((as_jpeg(&picture)?, ImageFormat::Jpeg))
}

fn as_jpeg(picture: &DynamicImage) -> Result<Vec<u8>, Failed> {
    let flat = picture.to_rgb8();
    for quality in JPEG_QUALITIES {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, quality).encode_image(&flat)?;
        if jpeg.len() <= MOST_BYTES {
            return Ok(jpeg);
        }
    }
    Err("it's too big".into())
}

fn model_takes(format: ImageFormat) -> bool {
    matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP)
}

fn fresh_name(extension: &str) -> String {
    format!("{}.{extension}", MessageId::fresh().0)
}

/// The tile of what's kept: a picture filling a square `side` wide, and any other file by its name.
/// `folder` is where it's kept; without one there's no picture to read.
pub(super) fn tile(
    id: impl Into<ElementId>,
    attachment: &Attachment,
    folder: Option<&Path>,
    side: f32,
    p: Palette,
) -> Tile {
    let tile = bare_tile(id, p);
    let picture = folder.filter(|_| attachment.mime.starts_with("image/"));
    match picture.map(|folder| folder.join(&attachment.file)) {
        Some(path) => tile
            .axis(Axis::Vertical)
            .size(px(side))
            .media(AttachmentMedia::new().src(move |window: &mut Window, cx: &mut App| {
                window.use_asset::<Thumbnail>(&path, cx)
            })),
        None => tile
            .w(px(FILE_TILE))
            .media(AttachmentMedia::new().child(icon(file_icon(&attachment.name), 14.)))
            .content(
                AttachmentContent::new().title(AttachmentTitle::new(attachment.name.clone())),
            ),
    }
}

/// A tile with nothing in it yet, in the theme's colours over the glass or a bubble.
pub(super) fn bare_tile(id: impl Into<ElementId>, p: Palette) -> Tile {
    Tile::new().id(id).bg(p.text.opacity(0.08)).border_color(p.hairline)
}

/// A picture as a tile draws it. The size a tile can show, as the picture itself could be 2048 px
/// a side, and what's drawn stays in memory as long as the app runs.
enum Thumbnail {}

impl Asset for Thumbnail {
    type Source = PathBuf;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    #[expect(clippy::manual_async_fn, reason = "an async fn would hold on to the App it's given")]
    fn load(path: PathBuf, _: &mut App) -> impl Future<Output = Self::Output> + Send + 'static {
        async move {
            let bytes = fs::read(path).map_err(|error: io::Error| ImageCacheError::Io(Arc::new(error)))?;
            let picture =
                image::load_from_memory(&bytes).map_err(|error| ImageCacheError::Image(Arc::new(error)))?;
            Ok(Arc::new(RenderImage::new(vec![Frame::new(thumbnail(&picture))])))
        }
    }
}

/// A small copy of `picture`, filling the square, its pixels in the blue-first order GPUI draws.
fn thumbnail(picture: &DynamicImage) -> RgbaImage {
    let mut small = picture.resize_to_fill(THUMBNAIL, THUMBNAIL, FilterType::Triangle).into_rgba8();
    for pixel in small.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    small
}

#[cfg(test)]
mod tests {
    use gpui_kit::{ExternalPaths, Image};
    use image::{Rgb, RgbImage};

    use super::*;
    use crate::test_support::scratch;

    fn encoded(picture: RgbImage, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(picture).write_to(&mut Cursor::new(&mut bytes), format).unwrap();
        bytes
    }

    fn sides(bytes: &[u8]) -> (u32, u32) {
        let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().unwrap();
        reader.into_dimensions().unwrap()
    }

    /// A picture no format makes much smaller.
    fn noise(width: u32, height: u32) -> RgbImage {
        let mut state = 0x2545_f491_u32;
        RgbImage::from_fn(width, height, |_, _| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let [red, green, blue, _] = state.to_le_bytes();
            Rgb([red, green, blue])
        })
    }

    #[test]
    fn a_picture_the_model_takes_stays_as_it_is() {
        let bytes = encoded(RgbImage::new(40, 30), ImageFormat::Png);
        assert_eq!(fitted(bytes.clone()).unwrap(), (bytes, ImageFormat::Png));
    }

    #[test]
    fn a_picture_too_long_comes_down_to_the_longest_side() {
        let bytes = encoded(RgbImage::new(4096, 1024), ImageFormat::Png);
        let (fit, format) = fitted(bytes).unwrap();
        assert_eq!(format, ImageFormat::Png);
        assert_eq!(sides(&fit), (2048, 512));
    }

    #[test]
    fn a_photo_too_long_stays_a_photo() {
        let bytes = encoded(noise(4096, 1024), ImageFormat::Jpeg);
        let (fit, format) = fitted(bytes).unwrap();
        assert_eq!(format, ImageFormat::Jpeg);
        assert_eq!(sides(&fit), (2048, 512));
    }

    #[test]
    fn a_format_the_model_does_not_take_becomes_one_it_does() {
        let bytes = encoded(RgbImage::new(40, 30), ImageFormat::Bmp);
        let (fit, format) = fitted(bytes).unwrap();
        assert_eq!(format, ImageFormat::Png);
        assert_eq!(sides(&fit), (40, 30));
    }

    #[test]
    fn a_picture_too_big_as_a_png_becomes_a_jpeg_that_fits() {
        let bytes = encoded(noise(1600, 1600), ImageFormat::Png);
        assert!(bytes.len() > MOST_BYTES);
        let (fit, format) = fitted(bytes).unwrap();
        assert_eq!(format, ImageFormat::Jpeg);
        assert!(fit.len() <= MOST_BYTES);
        assert_eq!(sides(&fit), (1600, 1600));
    }

    #[test]
    fn what_is_not_a_picture_says_so() {
        let why = fitted(b"plain words".to_vec()).unwrap_err();
        assert_eq!(why.to_string(), "it isn't a picture");
    }

    #[test]
    fn a_picture_is_kept_under_a_name_of_its_own_only_a_person_can_read() {
        let folder = scratch("attachments-picture").join("attachments");
        let bytes = encoded(RgbImage::new(4, 4), ImageFormat::Png);

        let kept = keep(Source::Picture(bytes.clone()), &folder).unwrap();

        assert_eq!(kept.name, "Pasted image");
        assert_eq!(kept.mime, "image/png");
        assert_eq!(Path::new(&kept.file).extension().and_then(OsStr::to_str), Some("png"));
        let path = folder.join(&kept.file);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!((mode(&path), mode(&folder)), (0o600, 0o700));
        assert_ne!(keep(Source::Picture(bytes), &folder).unwrap().file, kept.file);
    }

    #[test]
    fn a_picture_is_known_by_what_is_in_it_not_what_it_is_called() {
        let root = scratch("attachments-named");
        let path = root.join("shot.dat");
        fs::write(&path, encoded(RgbImage::new(4, 4), ImageFormat::Png)).unwrap();

        let kept = keep(Source::File(path), &root.join("attachments")).unwrap();

        assert_eq!((kept.name.as_str(), kept.mime.as_str()), ("shot.dat", "image/png"));
    }

    #[test]
    fn any_other_file_is_copied() {
        let root = scratch("attachments-file");
        let path = root.join("notes.md");
        fs::write(&path, "# Notes\n").unwrap();
        let folder = root.join("attachments");

        let kept = keep(Source::File(path.clone()), &folder).unwrap();

        assert_eq!((kept.name.as_str(), kept.mime.as_str()), ("notes.md", SOME_FILE));
        assert_eq!(Path::new(&kept.file).extension().and_then(OsStr::to_str), Some("md"));
        assert_eq!(fs::read_to_string(folder.join(&kept.file)).unwrap(), "# Notes\n");
        assert!(path.exists(), "the person's own stays");
    }

    #[test]
    fn a_folder_and_a_file_gone_say_why_they_are_not_attached() {
        let root = scratch("attachments-refused");
        let folder = root.join("attachments");
        let refused = root.join("refused");
        fs::create_dir(&refused).unwrap();

        let why = keep(Source::File(refused), &folder).unwrap_err();
        assert_eq!(why, "Can't attach refused: it's a folder.");

        let gone = keep(Source::File(root.join("gone.txt")), &folder).unwrap_err();
        assert!(gone.starts_with("Can't attach gone.txt: "), "{gone}");
    }

    #[test]
    fn a_thumbnail_fills_its_square_with_the_pixels_blue_first() {
        let red = RgbImage::from_pixel(1000, 500, Rgb([255, 0, 0]));
        let small = thumbnail(&DynamicImage::ImageRgb8(red));
        assert_eq!(small.dimensions(), (THUMBNAIL, THUMBNAIL));
        assert_eq!(small.get_pixel(0, 0).0, [0, 0, 255, 255]);
    }

    #[test]
    fn a_paste_attaches_its_pictures_and_files_and_leaves_words_to_be_typed() {
        let picture = Image::from_bytes(gpui_kit::ImageFormat::Png, vec![1, 2, 3]);
        let paths = ExternalPaths(vec![PathBuf::from("/a/b.txt")].into());
        let pasted = ClipboardItem {
            entries: vec![
                ClipboardEntry::Image(picture),
                ClipboardEntry::ExternalPaths(paths),
            ],
        };
        assert_eq!(
            sources_of(&pasted),
            [Source::Picture(vec![1, 2, 3]), Source::File(PathBuf::from("/a/b.txt"))],
        );
        assert_eq!(sources_of(&ClipboardItem::new_string("words".to_owned())), []);
    }
}
