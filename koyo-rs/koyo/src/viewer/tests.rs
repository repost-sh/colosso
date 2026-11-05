//! The viewer's readers on files made here: pictures of each kind the app's own encoders
//! write, a photo saved on its side, a document with a page turned, a drawing, and files that
//! aren't what they say.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use gpui_kit::RenderImage;
use image::codecs::gif::GifEncoder;
use image::{Delay, Frame, ImageFormat, Rgba, RgbaImage};

use super::lane::{Job, Lane, Opened, Out, Source};
use super::picture::{self, density};
use super::tiles::{Level, TileKey};
use super::{Kind, kind, pdf, svg};
use crate::test_support::scratch;

/// A pixel of a frame GPUI shows, as red, green, blue and alpha.
fn pixel(image: &RenderImage, frame: usize, x: usize, y: usize) -> [u8; 4] {
    let width = image.size(frame).width.0 as usize;
    let bytes = image.as_bytes(frame).unwrap();
    let at = (y * width + x) * 4;
    // GPUI's frames are blue first.
    [bytes[at + 2], bytes[at + 1], bytes[at], bytes[at + 3]]
}

fn size(image: &RenderImage, frame: usize) -> (i32, i32) {
    let size = image.size(frame);
    (size.width.0, size.height.0)
}

/// `picture` saved as `name` in a folder of its own, in the format its extension says.
fn save(folder: &Path, name: &str, picture: &RgbaImage) -> PathBuf {
    let path = folder.join(name);
    picture.save(&path).unwrap();
    path
}

/// Red on the left half, green on the right; the bottom right quarter half see-through.
fn halves(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        if x < width / 2 {
            Rgba([255, 0, 0, 255])
        } else if y < height / 2 {
            Rgba([0, 255, 0, 255])
        } else {
            Rgba([0, 255, 0, 128])
        }
    })
}

fn near(one: [u8; 4], other: [u8; 4]) -> bool {
    one.iter().zip(other).all(|(a, b)| a.abs_diff(b) <= 8)
}

#[test]
fn files_are_shown_as_what_their_extension_says() {
    assert_eq!(kind(Path::new("a/photo.JPG")), Some(Kind::Picture));
    assert_eq!(kind(Path::new("shot.heic")), Some(Kind::Picture));
    assert_eq!(kind(Path::new("logo.svg")), Some(Kind::Svg));
    assert_eq!(kind(Path::new("logo.svgz")), Some(Kind::Svg));
    assert_eq!(kind(Path::new("paper.pdf")), Some(Kind::Pdf));
    assert_eq!(kind(Path::new("main.rs")), None);
    assert_eq!(kind(Path::new("Makefile")), None);
}

#[test]
fn a_png_is_read_at_its_size_with_its_see_through_pixels_as_they_were() {
    let folder = scratch("viewer-png");
    let path = save(&folder, "halves.png", &halves(300, 200));
    let probe = picture::probe(&path).unwrap();
    assert_eq!(probe.pixels, (300, 200));
    assert_eq!(probe.density, 1);
    assert!(probe.see_through);
    assert!(probe.delays.is_empty());

    let image = picture::decode(&path, &probe, 300).unwrap();
    assert_eq!(size(&image, 0), (300, 200));
    assert!(near(pixel(&image, 0, 10, 10), [255, 0, 0, 255]));
    assert!(near(pixel(&image, 0, 290, 10), [0, 255, 0, 255]));
    // Quartz multiplies the colour by its alpha; it's taken back out for GPUI.
    assert!(near(pixel(&image, 0, 290, 190), [0, 255, 0, 128]), "{:?}", pixel(&image, 0, 290, 190));
}

#[test]
fn a_jpeg_is_decoded_no_larger_than_it_is_shown() {
    let folder = scratch("viewer-jpeg");
    let opaque = RgbaImage::from_fn(800, 400, |x, _| {
        if x < 400 { Rgba([255, 0, 0, 255]) } else { Rgba([0, 0, 255, 255]) }
    });
    let path = folder.join("wide.jpg");
    image::DynamicImage::ImageRgba8(opaque).to_rgb8().save(&path).unwrap();
    let probe = picture::probe(&path).unwrap();
    assert_eq!(probe.pixels, (800, 400));
    assert!(!probe.see_through);

    let image = picture::decode(&path, &probe, 400).unwrap();
    assert_eq!(size(&image, 0), (400, 200));
    assert!(near(pixel(&image, 0, 50, 100), [255, 0, 0, 255]));
    assert!(near(pixel(&image, 0, 350, 100), [0, 0, 255, 255]));
}

#[test]
fn a_photo_saved_on_its_side_is_shown_upright() {
    let folder = scratch("viewer-orientation");
    // Red on top, blue below, as the sensor saw it; its EXIF says to turn it a quarter
    // clockwise to show it.
    let sensor = RgbaImage::from_fn(80, 40, |_, y| {
        if y < 20 { Rgba([255, 0, 0, 255]) } else { Rgba([0, 0, 255, 255]) }
    });
    let mut jpeg = Vec::new();
    image::DynamicImage::ImageRgba8(sensor)
        .to_rgb8()
        .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
        .unwrap();
    let path = folder.join("side.jpg");
    std::fs::write(&path, with_orientation(&jpeg, 6)).unwrap();

    let probe = picture::probe(&path).unwrap();
    assert_eq!(probe.pixels, (40, 80));
    let image = picture::decode(&path, &probe, 80).unwrap();
    assert_eq!(size(&image, 0), (40, 80));
    // Turned a quarter clockwise, the top (red) is on the right.
    assert!(near(pixel(&image, 0, 35, 40), [255, 0, 0, 255]), "{:?}", pixel(&image, 0, 35, 40));
    assert!(near(pixel(&image, 0, 5, 40), [0, 0, 255, 255]), "{:?}", pixel(&image, 0, 5, 40));
}

/// `jpeg` with an EXIF block saying the camera was held at `orientation`.
fn with_orientation(jpeg: &[u8], orientation: u8) -> Vec<u8> {
    let mut tiff = b"MM\0\x2a\0\0\0\x08".to_vec();
    tiff.extend([0, 1]); // one entry
    tiff.extend([0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, orientation, 0, 0]);
    tiff.extend([0, 0, 0, 0]); // no next directory
    let mut exif = b"Exif\0\0".to_vec();
    exif.extend(tiff);
    let length = u16::try_from(exif.len() + 2).unwrap();
    let mut out = jpeg[..2].to_vec();
    out.extend([0xff, 0xe1]);
    out.extend(length.to_be_bytes());
    out.extend(exif);
    out.extend(&jpeg[2..]);
    out
}

#[test]
fn a_moving_gif_keeps_every_frame_and_how_long_each_shows() {
    let folder = scratch("viewer-gif");
    let path = folder.join("moving.gif");
    let colours = [Rgba([255, 0, 0, 255]), Rgba([0, 255, 0, 255]), Rgba([0, 0, 255, 255])];
    let delays = [50, 200, 0];
    {
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = GifEncoder::new(file);
        let frames = colours.iter().zip(delays).map(|(colour, ms)| {
            Frame::from_parts(
                RgbaImage::from_pixel(32, 16, *colour),
                0,
                0,
                Delay::from_saturating_duration(Duration::from_millis(ms)),
            )
        });
        encoder.encode_frames(frames).unwrap();
    }
    let probe = picture::probe(&path).unwrap();
    assert_eq!(probe.pixels, (32, 16));
    // A frame that says it shows for no time shows as long as a browser shows it.
    let ms: Vec<u128> = probe.delays.iter().map(Duration::as_millis).collect();
    assert_eq!(ms, [50, 200, 100]);

    let image = picture::decode(&path, &probe, 32).unwrap();
    assert_eq!(image.frame_count(), 3);
    for (frame, colour) in colours.iter().enumerate() {
        assert!(near(pixel(&image, frame, 16, 8), colour.0), "frame {frame}");
    }
}

#[test]
fn retina_screenshots_are_shown_at_the_points_they_were_on_screen() {
    assert_eq!(density(None), 1);
    assert_eq!(density(Some(72.)), 1);
    assert_eq!(density(Some(144.)), 2);
    assert_eq!(density(Some(216.)), 3);
    assert_eq!(density(Some(300.)), 1);
}

#[test]
fn a_file_that_isnt_a_picture_says_so() {
    let folder = scratch("viewer-broken");
    let path = folder.join("broken.png");
    std::fs::write(&path, b"not a picture at all").unwrap();
    assert!(picture::probe(&path).is_err());
    let path = folder.join("broken.pdf");
    std::fs::write(&path, b"%PDF-1.4 and nothing else").unwrap();
    let Err(why) = pdf::open(&path) else {
        panic!("a broken PDF opened");
    };
    assert!(!why.is_empty());
}

/// A PDF of two pages 200 points wide and 100 tall, each black on its bottom left quarter;
/// the second turned a quarter clockwise.
fn two_pages() -> Vec<u8> {
    let content = "0 0 0 rg 0 0 100 50 re f";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 5 0 R >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Rotate 90 /Contents 5 0 R >>"
            .to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n{object}\nendobj\n", index + 1).bytes());
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for offset in offsets {
        pdf.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    pdf.extend(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1)
            .bytes(),
    );
    pdf
}

fn tile(page: usize, scale: f32) -> Job {
    Job::Tile(TileKey {
        page,
        level: Level::of(scale),
        col: 0,
        row: 0,
    })
}

#[test]
fn a_pdf_page_is_drawn_on_white_paper_and_a_turned_one_upright() {
    let folder = scratch("viewer-pdf");
    let path = folder.join("two.pdf");
    std::fs::write(&path, two_pages()).unwrap();
    let (mut document, opened) = pdf::open(&path).unwrap();
    let Opened::Pages(pages) = opened else {
        panic!("a PDF opened as a picture");
    };
    assert_eq!(pages, [(200., 100.), (100., 200.)]);

    let first = document.draw(tile(0, 1.)).unwrap();
    assert_eq!(size(&first, 0), (200, 100));
    let black = [0, 0, 0, 255];
    let white = [255, 255, 255, 255];
    // The bottom left quarter is black, the rest is the paper.
    assert!(near(pixel(&first, 0, 50, 75), black));
    assert!(near(pixel(&first, 0, 50, 25), white));
    assert!(near(pixel(&first, 0, 150, 75), white));

    // Turned a quarter clockwise, the bottom left quarter is the top left one.
    let turned = document.draw(tile(1, 1.)).unwrap();
    assert_eq!(size(&turned, 0), (100, 200));
    assert!(near(pixel(&turned, 0, 25, 50), black), "{:?}", pixel(&turned, 0, 25, 50));
    assert!(near(pixel(&turned, 0, 75, 50), white));
    assert!(near(pixel(&turned, 0, 25, 150), white));

    // Twice as many pixels a point, the page is drawn twice as large.
    let sharp = document.draw(tile(0, 2.)).unwrap();
    assert_eq!(size(&sharp, 0), (400, 200));
    assert!(near(pixel(&sharp, 0, 100, 150), black));
}

#[test]
fn a_drawing_is_drawn_at_the_scale_asked_with_its_see_through_parts_clear() {
    let folder = scratch("viewer-svg");
    let path = folder.join("half.svg");
    std::fs::write(
        &path,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">
            <rect width="50" height="50" fill="#ff0000"/>
        </svg>"##,
    )
    .unwrap();
    let (mut drawing, opened) = svg::open(&path).unwrap();
    let Opened::Pages(pages) = opened else {
        panic!("a drawing opened as a picture");
    };
    assert_eq!(pages, [(100., 50.)]);
    let image = drawing.draw(tile(0, 2.)).unwrap();
    assert_eq!(size(&image, 0), (200, 100));
    assert!(near(pixel(&image, 0, 50, 50), [255, 0, 0, 255]));
    assert_eq!(pixel(&image, 0, 150, 50)[3], 0);
}

#[test]
fn a_lane_reads_the_file_then_draws_what_its_asked_for() {
    let folder = scratch("viewer-lane");
    let path = save(&folder, "lane.png", &halves(64, 32));
    let (sender, mut received) = futures::channel::mpsc::unbounded();
    let lane = Lane::start("lane.png", move || picture::open(&path), sender).unwrap();
    let first = futures::executor::block_on(received.next()).unwrap();
    let Out::Opened(Opened::Picture(probe)) = first else {
        panic!("the lane said something else first");
    };
    assert_eq!(probe.pixels, (64, 32));
    lane.want([Job::Whole(32)]);
    let Out::Drawn(Job::Whole(32), image) = futures::executor::block_on(received.next()).unwrap()
    else {
        panic!("the lane didn't draw the picture");
    };
    let image: Arc<RenderImage> = image;
    assert_eq!(size(&image, 0), (32, 16));
    drop(lane);
    // Its thread ends with it, and says nothing more.
    assert!(futures::executor::block_on(received.next()).is_none());
}

#[test]
fn a_lane_says_why_a_file_cant_be_read() {
    let folder = scratch("viewer-lane-broken");
    let path = folder.join("broken.svg");
    std::fs::write(&path, "<svg").unwrap();
    let (sender, mut received) = futures::channel::mpsc::unbounded();
    let _lane = Lane::start("broken.svg", move || svg::open(&path), sender).unwrap();
    let Some(Out::Failed(why)) = futures::executor::block_on(received.next()) else {
        panic!("a broken drawing opened");
    };
    assert!(!why.is_empty());
}
