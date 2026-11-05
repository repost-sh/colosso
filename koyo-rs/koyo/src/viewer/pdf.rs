//! PDFs, drawn by Quartz, the renderer Preview and Safari draw them with. The document is
//! opened on the viewer's thread and stays there: Quartz reads a page only when it's drawn,
//! so a document of a thousand pages costs what's on screen. Each page is its crop box, turned
//! as the page says, on white paper.

use std::path::Path;
use std::sync::Arc;

use gpui_kit::RenderImage;
use objc2_core_foundation::{CFRetained, CFURL, CGAffineTransform, CGFloat, CGRect};
use objc2_core_graphics::{CGContext, CGPDFBox, CGPDFDocument, CGPDFPage};

use super::bitmap::Canvas;
use super::lane::{Job, Opened, Source};

/// An open document and how its pages sit.
pub struct Pdf {
    document: CFRetained<CGPDFDocument>,
    pages: Vec<PageBox>,
}

/// A page's crop box, how far it's turned clockwise, and its size upright, in points.
#[derive(Clone, Copy, Debug)]
struct PageBox {
    crop: CGRect,
    turns: u8,
    size: (f32, f32),
}

/// Opens the document at `path`: unlocked if it's locked with no password, as Preview does,
/// and refused if it needs one.
pub fn open(path: &Path) -> Result<(Pdf, Opened), String> {
    let url = CFURL::from_file_path(path).ok_or("its path isn't one the system takes")?;
    let document = CGPDFDocument::with_url(Some(&url)).ok_or("it isn't a PDF the system reads")?;
    if !CGPDFDocument::is_unlocked(Some(&document)) {
        let empty = std::ptr::NonNull::from(c"").cast();
        // SAFETY: a NUL-terminated empty string, which locked documents with no password take.
        unsafe { CGPDFDocument::unlock_with_password(Some(&document), empty) };
        if !CGPDFDocument::is_unlocked(Some(&document)) {
            return Err("it's locked with a password".into());
        }
    }
    let count = CGPDFDocument::number_of_pages(Some(&document));
    // A page Quartz can't read keeps its place, as a blank letter-sized page, so the pages
    // after it are still the ones their numbers say.
    let pages: Vec<PageBox> = (1..=count)
        .map(|number| CGPDFDocument::page(Some(&document), number).map_or(LETTER, |page| page_box(&page)))
        .collect();
    if pages.is_empty() {
        return Err("it has no pages".into());
    }
    let sizes = pages.iter().map(|page| page.size).collect();
    Ok((Pdf { document, pages }, Opened::Pages(sizes)))
}

/// A US letter page, upright.
const LETTER: PageBox = PageBox {
    crop: CGRect {
        origin: objc2_core_foundation::CGPoint { x: 0., y: 0. },
        size: objc2_core_foundation::CGSize {
            width: 612.,
            height: 792.,
        },
    },
    turns: 0,
    size: (612., 792.),
};

fn page_box(page: &CGPDFPage) -> PageBox {
    let crop = CGPDFPage::box_rect(Some(page), CGPDFBox::CropBox);
    let turns = (CGPDFPage::rotation_angle(Some(page)).rem_euclid(360) / 90) as u8;
    let (width, height) = (crop.size.width as f32, crop.size.height as f32);
    let size = if turns % 2 == 1 { (height, width) } else { (width, height) };
    PageBox { crop, turns, size }
}

/// What takes a point of the crop box, from its bottom left corner, to the page upright,
/// turned `turns` quarters clockwise.
fn upright(turns: u8, width: CGFloat, height: CGFloat) -> CGAffineTransform {
    let (a, b, c, d, tx, ty) = match turns {
        1 => (0., -1., 1., 0., 0., width),
        2 => (-1., 0., 0., -1., width, height),
        3 => (0., 1., -1., 0., height, 0.),
        _ => (1., 0., 0., 1., 0., 0.),
    };
    CGAffineTransform { a, b, c, d, tx, ty }
}

impl Source for Pdf {
    fn draw(&mut self, job: Job) -> Option<Arc<RenderImage>> {
        let Job::Tile(key) = job else {
            return None;
        };
        let page_box = *self.pages.get(key.page)?;
        let page = CGPDFDocument::page(Some(&self.document), key.page + 1)?;
        let (left, top, width, height) = key.pixels(page_box.size);
        let canvas = Canvas::new(width, height)?;
        canvas.paper();
        let context = Some(canvas.context());
        let scale = CGFloat::from(key.level.scale());
        let page_height = CGFloat::from(page_box.size.1) * scale;
        // Quartz counts up from the bottom: the tile's bottom edge is this far up the page.
        let bottom = page_height - CGFloat::from(top + height);
        CGContext::translate_ctm(context, -CGFloat::from(left), -bottom);
        CGContext::scale_ctm(context, scale, scale);
        let crop = page_box.crop;
        CGContext::concat_ctm(
            context,
            upright(page_box.turns, crop.size.width, crop.size.height),
        );
        CGContext::translate_ctm(context, -crop.origin.x, -crop.origin.y);
        CGContext::clip_to_rect(context, crop);
        CGContext::draw_pdf_page(context, Some(&page));
        let frame = canvas.into_frame(false, None)?;
        Some(Arc::new(RenderImage::new(vec![frame])))
    }
}
