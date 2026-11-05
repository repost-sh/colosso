//! SVG drawings, drawn by resvg, the renderer GPUI draws its own SVGs with. A drawing is
//! parsed once, on the viewer's thread, and then drawn tile by tile at whatever zoom it's
//! shown at, so it stays sharp however far in it's zoomed. The system's fonts are read only
//! for a drawing with text in it, once for every drawing.

use std::path::Path;
use std::sync::{Arc, LazyLock};

use gpui_kit::RenderImage;
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{self, fontdb};

use super::bitmap::unpremultiplied;
use super::lane::{Job, Opened, Source};

/// The largest drawing opened: a larger one would take seconds to parse, and more memory
/// than it's worth.
const MOST_BYTES: u64 = 64 << 20;

/// The system's fonts, read once, the first time a drawing has text.
static SYSTEM_FONTS: LazyLock<Arc<fontdb::Database>> = LazyLock::new(|| {
    let mut fonts = fontdb::Database::new();
    fonts.load_system_fonts();
    Arc::new(fonts)
});

/// A parsed drawing.
pub struct Svg {
    tree: usvg::Tree,
}

/// Parses the drawing at `path`, gzipped (`.svgz`) or not; pictures it links to are found
/// beside it.
pub fn open(path: &Path) -> Result<(Svg, Opened), String> {
    let bytes = std::fs::metadata(path).map_err(|error| error.to_string())?.len();
    if bytes > MOST_BYTES {
        return Err(format!("it's {} MB, more than a drawing is opened at", bytes >> 20));
    }
    let data = std::fs::read(path).map_err(|error| error.to_string())?;
    let tree = usvg::Tree::from_data(&data, &options(path)).map_err(|error| error.to_string())?;
    let size = tree.size();
    let size = (size.width(), size.height());
    Ok((Svg { tree }, Opened::Pages(vec![size])))
}

/// How a drawing at `path` is parsed: with no fonts until its text asks for them.
fn options(path: &Path) -> usvg::Options<'static> {
    let default = usvg::FontResolver::default_font_selector();
    let select_font = Box::new(move |font: &usvg::Font, fonts: &mut Arc<fontdb::Database>| {
        if fonts.is_empty() {
            *fonts = Arc::clone(&SYSTEM_FONTS);
        }
        default(font, fonts).or_else(|| {
            // A family the system hasn't got, or one of CSS's own names: any sans-serif.
            let query = fontdb::Query {
                families: &[fontdb::Family::SansSerif],
                ..fontdb::Query::default()
            };
            fonts.query(&query).or_else(|| fonts.faces().next().map(|face| face.id))
        })
    });
    usvg::Options {
        resources_dir: path.parent().map(Path::to_path_buf),
        font_resolver: usvg::FontResolver {
            select_font,
            select_fallback: usvg::FontResolver::default_fallback_selector(),
        },
        ..usvg::Options::default()
    }
}

impl Source for Svg {
    fn draw(&mut self, job: Job) -> Option<Arc<RenderImage>> {
        let Job::Tile(key) = job else {
            return None;
        };
        let size = self.tree.size();
        let (left, top, width, height) = key.pixels((size.width(), size.height()));
        let mut pixmap = Pixmap::new(width, height)?;
        let scale = key.level.scale();
        let transform = Transform::from_row(scale, 0., 0., scale, -(left as f32), -(top as f32));
        resvg::render(&self.tree, transform, &mut pixmap.as_mut());
        let mut pixels = pixmap.take();
        // resvg's pixels are red first, multiplied by their alpha; GPUI's are blue first,
        // unmultiplied: both made so in one pass.
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            unpremultiplied(pixel);
        }
        let buffer = image::RgbaImage::from_raw(width, height, pixels)?;
        Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
    }
}
