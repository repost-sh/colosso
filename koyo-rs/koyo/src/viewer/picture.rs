//! Pictures read with the system's decoders (`ImageIO`), as Preview reads them: what a picture
//! is (its size, whether it's see-through, whether it moves) is read without decoding it, and
//! it's then decoded at no more pixels than are shown, upright as the camera held it, its
//! colours in sRGB. A JPEG decoded at half or a quarter of its size costs a fraction of the
//! whole, as the decoder skips the detail it would throw away.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::RenderImage;
use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, CFURL, CGFloat, CGPoint,
    CGRect, CGSize,
};
use objc2_core_graphics::{CGContext, CGImage, CGImageAlphaInfo};
use objc2_image_io::{
    CGImageSource, kCGImagePropertyAPNGDelayTime, kCGImagePropertyAPNGUnclampedDelayTime,
    kCGImagePropertyDPIWidth, kCGImagePropertyGIFDelayTime, kCGImagePropertyGIFDictionary,
    kCGImagePropertyGIFUnclampedDelayTime, kCGImagePropertyHEICSDelayTime,
    kCGImagePropertyHEICSDictionary, kCGImagePropertyHEICSUnclampedDelayTime,
    kCGImagePropertyHasAlpha, kCGImagePropertyOrientation, kCGImagePropertyPNGDictionary,
    kCGImagePropertyPixelHeight, kCGImagePropertyPixelWidth, kCGImagePropertyWebPDelayTime,
    kCGImagePropertyWebPDictionary, kCGImagePropertyWebPUnclampedDelayTime,
    kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform,
    kCGImageSourceShouldCache, kCGImageSourceShouldCacheImmediately,
    kCGImageSourceThumbnailMaxPixelSize,
};

use super::bitmap::Canvas;
use super::lane::{Job, Opened, Source};

/// The most an animation's frames may take together, decoded: past it they're decoded
/// smaller, as a long GIF at its full size would take more memory than it's worth.
const ANIMATION_BYTES: f64 = 96. * 1024. * 1024.;
/// The longest side a picture is decoded at, however far it's zoomed: the GPU takes a texture
/// four times as long, but one this size is already 256 MB.
pub const LONGEST: u32 = 8192;
/// A frame shown for less than this is shown for `SLOW_FRAME`, as browsers do: GIFs made for
/// old browsers say 0 or 10 ms and mean "as fast as is sensible".
const QUICKEST_FRAME: Duration = Duration::from_millis(20);
const SLOW_FRAME: Duration = Duration::from_millis(100);

/// What a picture is, read from its header.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    /// Its size as shown, upright, in pixels.
    pub pixels: (u32, u32),
    /// How many of its pixels make a point: 2 for a picture saved at 144 dpi, as a Retina
    /// screenshot is, so it's shown at the size it was on screen; 1 otherwise.
    pub density: u32,
    /// Whether it may have see-through pixels.
    pub see_through: bool,
    /// Which of the file's images is shown: the first, or the largest of an icon's sizes.
    pub index: usize,
    /// How long each frame shows, for an animation; empty for a still picture.
    pub delays: Vec<Duration>,
}

impl Probe {
    /// Its longest side, in pixels.
    pub fn longest(&self) -> u32 {
        self.pixels.0.max(self.pixels.1)
    }
}

/// A picture as its viewer's thread holds it: where it is and what it read of it.
pub struct Picture {
    path: PathBuf,
    probe: Probe,
}

/// Reads what the picture at `path` is, for its viewer's thread to decode it as it's shown.
pub fn open(path: &Path) -> Result<(Picture, Opened), String> {
    let probe = probe(path)?;
    let picture = Picture {
        path: path.to_path_buf(),
        probe: probe.clone(),
    };
    Ok((picture, Opened::Picture(probe)))
}

impl Source for Picture {
    fn draw(&mut self, job: Job) -> Option<Arc<RenderImage>> {
        let Job::Whole(longest) = job else {
            return None;
        };
        decode(&self.path, &self.probe, longest)
            .inspect_err(|why| log::warn!("{} couldn't be decoded: {why}", self.path.display()))
            .ok()
    }
}

/// Reads what the picture at `path` is, without decoding it.
pub fn probe(path: &Path) -> Result<Probe, String> {
    let source = source(path)?;
    // SAFETY: a source ImageIO made, asked about its own images.
    let count = unsafe { source.count() };
    if count == 0 {
        return Err("it holds no picture".into());
    }
    // SAFETY: as above.
    let kind = unsafe { source.r#type() }.map(|kind| kind.to_string()).unwrap_or_default();
    let delays = if count > 1 { delays(&source, &kind, count) } else { Vec::new() };
    let index = if !delays.is_empty() {
        0
    } else if count > 1 && matches!(kind.as_str(), "com.microsoft.ico" | "com.apple.icns" | "com.microsoft.cur") {
        largest(&source, count)
    } else {
        // SAFETY: as above.
        unsafe { source.primary_image_index() }
    };
    let properties = properties(&source, index).ok_or("its header can't be read")?;
    // SAFETY: constants ImageIO defines, read from the dictionary it made.
    let (width, height, orientation, dpi, alpha) = unsafe {
        (
            number(&properties, kCGImagePropertyPixelWidth),
            number(&properties, kCGImagePropertyPixelHeight),
            number(&properties, kCGImagePropertyOrientation),
            number(&properties, kCGImagePropertyDPIWidth),
            flag(&properties, kCGImagePropertyHasAlpha),
        )
    };
    let (Some(width), Some(height)) = (width, height) else {
        return Err("it doesn't say how big it is".into());
    };
    let (width, height) = (width as u32, height as u32);
    // Orientations 5 to 8 turn the picture on its side.
    let pixels = if matches!(orientation.map(|o| o as u32), Some(5..=8)) {
        (height, width)
    } else {
        (width, height)
    };
    Ok(Probe {
        pixels,
        density: density(dpi),
        // A format that doesn't say (JPEG) is asked its image, which isn't decoded for it.
        see_through: alpha.unwrap_or_else(|| has_alpha(&source, index)),
        index,
        delays,
    })
}

/// Whether image `index` of `source` keeps an alpha channel, as its pixels would be decoded:
/// the image is only described, not decoded, as the source caches nothing. One that can't be
/// read is taken to be see-through, which only costs a pass over its pixels.
fn has_alpha(source: &CGImageSource, index: usize) -> bool {
    // SAFETY: a source ImageIO made, asked for one of its own images with no options.
    let Some(image) = (unsafe { source.image_at_index(index, None) }) else {
        return true;
    };
    !matches!(
        CGImage::alpha_info(Some(&image)),
        CGImageAlphaInfo::None | CGImageAlphaInfo::NoneSkipFirst | CGImageAlphaInfo::NoneSkipLast
    )
}

/// How many pixels make a point for a picture saved at `dpi`: 2 or 3 for Retina screenshots,
/// which are saved at 144 and 216 dpi, and 1 for anything else, which is shown pixel for
/// point.
pub fn density(dpi: Option<f64>) -> u32 {
    let Some(dpi) = dpi else {
        return 1;
    };
    [2u32, 3]
        .into_iter()
        .find(|times| (dpi - 72. * f64::from(*times)).abs() < 1.)
        .unwrap_or(1)
}

/// Decodes the picture `probe` read at `path`, its longest side at most `longest` pixels, every
/// frame of it when it moves: fewer pixels when its frames would take more memory than an
/// animation is given.
pub fn decode(path: &Path, probe: &Probe, longest: u32) -> Result<Arc<RenderImage>, String> {
    let source = source(path)?;
    let mut longest = longest.clamp(1, LONGEST);
    if !probe.delays.is_empty() {
        let (width, height) = probe.pixels;
        let bytes = f64::from(width) * f64::from(height) * 4. * probe.delays.len() as f64;
        let scale = (ANIMATION_BYTES / bytes).sqrt().min(1.);
        longest = longest.min(((f64::from(probe.longest()) * scale) as u32).max(1));
    }
    let frames: Vec<_> = if probe.delays.is_empty() {
        vec![frame(&source, probe.index, longest, probe.see_through, None)?]
    } else {
        (probe.delays.iter().enumerate())
            .map(|(index, delay)| frame(&source, index, longest, probe.see_through, Some(*delay)))
            .collect::<Result<_, _>>()?
    };
    Ok(Arc::new(RenderImage::new(frames)))
}

/// Image `index` of `source`, its longest side at most `longest`, drawn into a bitmap of its
/// own.
fn frame(
    source: &CGImageSource,
    index: usize,
    longest: u32,
    see_through: bool,
    delay: Option<Duration>,
) -> Result<image::Frame, String> {
    let most = CFNumber::new_i64(i64::from(longest));
    let yes = CFBoolean::new(true);
    // SAFETY: constants ImageIO defines.
    let keys: [&CFString; 4] = unsafe {
        [
            kCGImageSourceCreateThumbnailFromImageAlways,
            kCGImageSourceThumbnailMaxPixelSize,
            kCGImageSourceCreateThumbnailWithTransform,
            kCGImageSourceShouldCacheImmediately,
        ]
    };
    let values: [&CFType; 4] = [yes, &most, yes, yes];
    let options = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
    // SAFETY: the options are the keys and values ImageIO documents for thumbnails.
    let image = unsafe { source.thumbnail_at_index(index, Some(options.as_opaque())) }
        .ok_or("it couldn't be decoded")?;
    draw(&image, see_through, delay)
}

/// `image` in a bitmap GPUI can show.
fn draw(image: &CGImage, see_through: bool, delay: Option<Duration>) -> Result<image::Frame, String> {
    let (width, height) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    let canvas = Canvas::new(width as u32, height as u32).ok_or("there's no memory to show it")?;
    let rect = CGRect::new(
        CGPoint::new(0., 0.),
        CGSize::new(width as CGFloat, height as CGFloat),
    );
    CGContext::draw_image(Some(canvas.context()), rect, Some(image));
    canvas
        .into_frame(see_through, delay)
        .ok_or_else(|| "its pixels don't fit its size".into())
}

/// A source for the file at `path` that keeps nothing decoded once it's asked for a picture.
fn source(path: &Path) -> Result<CFRetained<CGImageSource>, String> {
    let url = CFURL::from_file_path(path).ok_or("its path isn't one the system takes")?;
    // SAFETY: a constant ImageIO defines.
    let keys: [&CFString; 1] = [unsafe { kCGImageSourceShouldCache }];
    let values: [&CFType; 1] = [CFBoolean::new(false)];
    let options = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
    // SAFETY: the options are a key and value ImageIO documents for sources.
    unsafe { CGImageSource::with_url(&url, Some(options.as_opaque())) }
        .ok_or_else(|| "it isn't a picture the system reads".into())
}

/// The properties of image `index`, as a dictionary of strings to values.
fn properties(
    source: &CGImageSource,
    index: usize,
) -> Option<CFRetained<CFDictionary<CFString, CFType>>> {
    // SAFETY: an index under the source's count, asked without options.
    let properties = unsafe { source.properties_at_index(index, None) }?;
    // SAFETY: ImageIO's properties are keyed by strings.
    Some(unsafe { CFRetained::cast_unchecked(properties) })
}

fn number(dictionary: &CFDictionary<CFString, CFType>, key: &CFString) -> Option<f64> {
    dictionary.get(key)?.downcast::<CFNumber>().ok()?.as_f64()
}

fn flag(dictionary: &CFDictionary<CFString, CFType>, key: &CFString) -> Option<bool> {
    Some(dictionary.get(key)?.downcast::<CFBoolean>().ok()?.as_bool())
}

fn child(
    dictionary: &CFDictionary<CFString, CFType>,
    key: &CFString,
) -> Option<CFRetained<CFDictionary<CFString, CFType>>> {
    let child = dictionary.get(key)?.downcast::<CFDictionary>().ok()?;
    // SAFETY: ImageIO's nested properties are keyed by strings too.
    Some(unsafe { CFRetained::cast_unchecked(child) })
}

/// How long each of `count` frames shows, when the format is one that moves and its frames
/// say; empty otherwise, as for a TIFF's pages or an icon's sizes.
fn delays(source: &CGImageSource, kind: &str, count: usize) -> Vec<Duration> {
    // SAFETY: constants ImageIO defines.
    let (group, unclamped, clamped) = unsafe {
        match kind {
            "com.compuserve.gif" => (
                kCGImagePropertyGIFDictionary,
                kCGImagePropertyGIFUnclampedDelayTime,
                kCGImagePropertyGIFDelayTime,
            ),
            "public.png" => (
                kCGImagePropertyPNGDictionary,
                kCGImagePropertyAPNGUnclampedDelayTime,
                kCGImagePropertyAPNGDelayTime,
            ),
            "org.webmproject.webp" => (
                kCGImagePropertyWebPDictionary,
                kCGImagePropertyWebPUnclampedDelayTime,
                kCGImagePropertyWebPDelayTime,
            ),
            "public.heics" => (
                kCGImagePropertyHEICSDictionary,
                kCGImagePropertyHEICSUnclampedDelayTime,
                kCGImagePropertyHEICSDelayTime,
            ),
            _ => return Vec::new(),
        }
    };
    let mut delays = Vec::with_capacity(count);
    for index in 0..count {
        let seconds = properties(source, index)
            .and_then(|properties| child(&properties, group))
            .and_then(|group| number(&group, unclamped).or_else(|| number(&group, clamped)));
        let Some(seconds) = seconds else {
            return Vec::new();
        };
        let delay = Duration::from_secs_f64(seconds.max(0.));
        delays.push(if delay < QUICKEST_FRAME { SLOW_FRAME } else { delay });
    }
    delays
}

/// The index of the widest of `count` images: an icon file's largest size.
fn largest(source: &CGImageSource, count: usize) -> usize {
    (0..count)
        .max_by_key(|index| {
            properties(source, *index)
                // SAFETY: a constant ImageIO defines.
                .and_then(|properties| number(&properties, unsafe { kCGImagePropertyPixelWidth }))
                .map_or(0, |width| width as u32)
        })
        .unwrap_or(0)
}
