//! Pictures compressed to ASTC by the system's own encoder: `ImageIO`'s `org.khronos.astc`
//! destination, which writes the blocks every Apple Silicon GPU reads as it samples, after a
//! 16-byte header. It costs the binary nothing, as the viewer counts on `ImageIO` to read.

use objc2_core_foundation::{CFDictionary, CFMutableData, CFString, CFType};
use objc2_core_graphics::CGImage;
use objc2_image_io::{
    CGImageDestination, kCGImagePropertyASTCBlockSize, kCGImagePropertyASTCBlockSize4x4,
    kCGImagePropertyASTCBlockSize8x8,
};

/// How many pixels a block holds a side: the two sizes `ImageIO` names. Each block is 16 bytes,
/// so 4 × 4 is 8 bits a pixel, near lossless, and 8 × 8 is 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Blocks {
    Four,
    Eight,
}

impl Blocks {
    /// Pixels a side.
    pub fn side(self) -> u32 {
        match self {
            Blocks::Four => 4,
            Blocks::Eight => 8,
        }
    }

    /// How many bytes a picture `width` by `height` takes in these blocks.
    pub fn bytes(self, width: u32, height: u32) -> usize {
        let side = self.side();
        width.div_ceil(side) as usize * height.div_ceil(side) as usize * BLOCK_BYTES
    }
}

/// A picture in ASTC blocks, row by row from its top left.
pub struct Astc {
    pub blocks: Blocks,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// What every ASTC block takes.
const BLOCK_BYTES: usize = 16;
/// What an ASTC file starts with, then its block size and its size, before its blocks.
const MAGIC: [u8; 4] = [0x13, 0xab, 0xa1, 0x5c];
const HEADER: usize = 16;

/// `image` compressed to `blocks` by `ImageIO`.
pub fn encode(image: &CGImage, blocks: Blocks) -> Result<Astc, String> {
    let file = CFMutableData::new(None, 0).ok_or("there's no memory to compress it")?;
    let kind = CFString::from_static_str("org.khronos.astc");
    // SAFETY: a type `ImageIO` names, for one image, with no options.
    let destination = unsafe { CGImageDestination::with_data(&file, &kind, 1, None) }
        .ok_or("the system has no ASTC encoder")?;
    // SAFETY: constants `ImageIO` defines.
    let (key, size) = unsafe {
        let size = match blocks {
            Blocks::Four => kCGImagePropertyASTCBlockSize4x4,
            Blocks::Eight => kCGImagePropertyASTCBlockSize8x8,
        };
        (kCGImagePropertyASTCBlockSize, size)
    };
    let keys: [&CFString; 1] = [key];
    let values: [&CFType; 1] = [size];
    let properties = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
    // SAFETY: the image and the properties `ImageIO` documents for an ASTC destination.
    let written = unsafe {
        destination.add_image(image, Some(properties.as_opaque()));
        destination.finalize()
    };
    if !written {
        return Err("the system's ASTC encoder couldn't compress it".into());
    }
    // SAFETY: the data is the destination's, which nothing writes to once it's finalized.
    read(unsafe { file.as_bytes_unchecked() })
}

/// The picture an ASTC file holds, taken out of it.
pub fn read(file: &[u8]) -> Result<Astc, String> {
    let (header, data) = file
        .split_at_checked(HEADER)
        .ok_or("the file is shorter than its header")?;
    if header[..4] != MAGIC {
        return Err("the file isn't ASTC".into());
    }
    let blocks = match (header[4], header[5], header[6]) {
        (4, 4, 1) => Blocks::Four,
        (8, 8, 1) => Blocks::Eight,
        (x, y, z) => return Err(format!("its blocks are {x} × {y} × {z}")),
    };
    let side = |at: usize| u32::from_le_bytes([header[at], header[at + 1], header[at + 2], 0]);
    let (width, height) = (side(7), side(10));
    if side(13) != 1 {
        return Err("it's a picture in three dimensions".into());
    }
    if data.len() != blocks.bytes(width, height) {
        return Err(format!(
            "{} bytes of blocks, for {width} × {height} pixels",
            data.len()
        ));
    }
    Ok(Astc {
        blocks,
        width,
        height,
        data: data.to_vec(),
    })
}

/// An ASTC file's header for a picture `width` by `height` in `side` pixel blocks, as the
/// tests write files for `read` and for `ImageIO` to read back.
#[cfg(test)]
pub(super) fn header(side: u8, width: u32, height: u32) -> Vec<u8> {
    let mut header = MAGIC.to_vec();
    header.extend([side, side, 1]);
    header.extend(&width.to_le_bytes()[..3]);
    header.extend(&height.to_le_bytes()[..3]);
    header.extend([1, 0, 0]);
    header
}

#[cfg(test)]
mod tests {
    use objc2_core_graphics::CGBitmapContextCreateImage;

    use super::{Blocks, MAGIC, encode, header, read};
    use crate::viewer::bitmap::Canvas;

    #[test]
    fn a_file_gives_its_blocks_and_its_size() {
        let mut file = header(8, 20, 9);
        file.extend(vec![7; 3 * 2 * 16]);
        let astc = read(&file).unwrap();
        assert_eq!(
            (astc.blocks, astc.width, astc.height),
            (Blocks::Eight, 20, 9)
        );
        assert_eq!(astc.data.len(), 96);
    }

    #[test]
    fn a_file_that_says_more_or_less_than_it_holds_is_refused() {
        let mut file = header(4, 8, 8);
        file.extend(vec![0; 4 * 16 - 1]);
        assert!(read(&file).is_err(), "a byte short");
        assert!(read(&[0; 20]).is_err(), "not ASTC");
        assert!(read(&MAGIC).is_err(), "no header");
        let mut file = header(6, 6, 6);
        file.extend(vec![0; 16]);
        assert!(read(&file).is_err(), "blocks Metal has no format of here");
    }

    #[test]
    fn the_systems_encoder_compresses_a_picture_to_either_block_size() {
        let canvas = Canvas::new(100, 60).unwrap();
        canvas.paper();
        let image = CGBitmapContextCreateImage(Some(canvas.context())).unwrap();
        for (blocks, bytes) in [(Blocks::Four, 25 * 15 * 16), (Blocks::Eight, 13 * 8 * 16)] {
            let astc = encode(&image, blocks).unwrap();
            assert_eq!((astc.blocks, astc.width, astc.height), (blocks, 100, 60));
            assert_eq!(astc.data.len(), bytes);
        }
    }
}
