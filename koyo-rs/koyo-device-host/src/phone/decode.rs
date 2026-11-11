//! A real device's pictures (`docs/devices.md` 3.1, 5): HEVC in Annex-B form, a picture's units
//! as they came behind one RTP marker, decoded by the Mac's hardware decoder in its own format
//! and made BGRA in one surface of the host's own, which the ring turns upright into its slots.
//! A picture the decoder can't take asks the device for a whole one, twice a second at most; when
//! asking brings nothing decodable for two seconds, the stream is said to be stalled. While
//! nobody wants the picture, pictures are dropped undecoded; wanted again, decoding starts from a
//! whole picture asked for.

use std::ffi::c_void;
use std::ops::Range;
use std::ptr::NonNull;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use koyo_device_wire::LogLevel;
use objc2_core_foundation::{CFBoolean, CFRetained, Type};
use objc2_core_media::{CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMSampleTimingInfo, CMVideoFormatDescriptionCreateFromHEVCParameterSets, kCMTimeInvalid};
use objc2_core_video::{CVImageBuffer, CVPixelBuffer, CVPixelBufferGetHeight, CVPixelBufferGetWidth};
use objc2_io_surface::IOSurfaceRef;
use objc2_video_toolbox::{
    VTDecodeFrameFlags, VTDecodeInfoFlags, VTDecompressionOutputCallbackRecord, VTDecompressionSession, VTPixelTransferSession,
    VTSessionSetProperty, kVTDecompressionPropertyKey_RealTime,
};
use tokio::sync::Notify;

use crate::link::Teller;
use crate::ring::{self, Ring};

/// A keyframe asked for no more often than this.
const ASK_EVERY: Duration = Duration::from_millis(500);
/// Asking this long with nothing decodable coming is a stalled stream.
const STALLS_AFTER: Duration = Duration::from_secs(2);
/// How often the decoder's counts are logged, with `--stats`.
const COUNT_EVERY: Duration = Duration::from_secs(5);
const START: &[u8] = &[0, 0, 0, 1];

/// One unit of a picture: its type, and where its bytes are behind their start code.
struct Unit {
    kind: u8,
    bytes: Range<usize>,
}

/// The units of `bytes`, each behind `00 00 00 01`.
fn units(bytes: &[u8]) -> Vec<Unit> {
    let starts: Vec<usize> = memchr::memmem::find_iter(bytes, START).map(|at| at + START.len()).collect();
    (starts.iter().enumerate())
        .filter_map(|(index, &from)| {
            let to = starts.get(index + 1).map_or(bytes.len(), |next| next - START.len());
            let unit = bytes.get(from..to).filter(|unit| unit.len() >= 2)?;
            Some(Unit { kind: (unit[0] >> 1) & 0x3f, bytes: from..to })
        })
        .collect()
}

/// A picture's slices, each behind its length: the form the decoder takes.
fn slices(bytes: &[u8], units: &[Unit]) -> Vec<u8> {
    let mut slices = Vec::with_capacity(bytes.len());
    for unit in units.iter().filter(|unit| unit.kind <= 31) {
        slices.extend_from_slice(&u32::try_from(unit.bytes.len()).unwrap_or(0).to_be_bytes());
        slices.extend_from_slice(&bytes[unit.bytes.clone()]);
    }
    slices
}

/// Whether a unit is a slice of a whole picture (IRAP: a decoder can start from it).
fn key(units: &[Unit]) -> bool {
    units.iter().any(|unit| (16..=23).contains(&unit.kind))
}

/// What a framework's `…Create` gave back through its out-pointer, once it said it worked.
///
/// # Safety
/// `raw` must be what the call that returned `status` wrote, with a +1 retain count.
unsafe fn created<T: Type>(status: i32, raw: *mut T) -> Option<CFRetained<T>> {
    let raw = NonNull::new(raw).filter(|_| status == 0)?;
    // SAFETY: the caller's word: a +1 retain count, which this takes.
    Some(unsafe { CFRetained::from_raw(raw) })
}

/// Where a decoded picture goes: a BGRA surface of the host's own, the ring's source.
struct Output {
    ring: Ring,
    transfer: CFRetained<VTPixelTransferSession>,
    source: Option<(CFRetained<IOSurfaceRef>, CFRetained<CVPixelBuffer>)>,
    failed: u64,
}

/// The decoder's word for one picture: made BGRA in the source, and the ring told.
unsafe extern "C-unwind" fn decoded(
    output: *mut c_void,
    _: *mut c_void,
    status: i32,
    _: VTDecodeInfoFlags,
    image: *mut CVImageBuffer,
    _: objc2_core_media::CMTime,
    _: objc2_core_media::CMTime,
) {
    // SAFETY: the refcon is the `Output` its `Decoder` owns, decoding synchronously on the thread
    // that holds it; the image is the decoder's, valid for the call.
    let (output, image) = unsafe { (&mut *output.cast::<Output>(), image.as_ref()) };
    match image.filter(|_| status == 0) {
        Some(image) => output.present(image),
        None => output.failed += 1,
    }
}

impl Output {
    fn present(&mut self, image: &CVPixelBuffer) {
        let (width, height) = (CVPixelBufferGetWidth(image), CVPixelBufferGetHeight(image));
        let fits = (self.source.as_ref()).is_some_and(|(surface, _)| surface.width() == width && surface.height() == height);
        if !fits {
            self.source = ring::surface(width, height).and_then(|surface| Some((surface.clone(), ring::pixel_buffer(&surface)?)));
            if let Some((surface, _)) = &self.source
                && let Err(why) = self.ring.mount(surface.clone())
            {
                self.ring.log_error(&why);
            }
        }
        let Some((_, pixels)) = &self.source else {
            return;
        };
        // SAFETY: two pixel buffers of the same size; the transfer converts the decoder's format.
        let moved = unsafe { self.transfer.transfer_image(image, pixels) };
        if moved == 0 {
            self.ring.frame();
        } else {
            self.failed += 1;
        }
    }
}

/// The stream's decoder, on a thread of its own: fed each picture's bytes.
struct Decoder {
    output: Box<Output>,
    session: Option<CFRetained<VTDecompressionSession>>,
    format: Option<CFRetained<CMFormatDescription>>,
    sets: [Vec<u8>; 3],
    /// Pictures dropped while unwanted, or lost: the next decoded must be a whole one.
    waiting_key: bool,
    asked: Option<Instant>,
    /// Since when keyframes have been asked for with nothing decoded, and whether that was said.
    asking_since: Option<Instant>,
    stall_said: bool,
    keyframe: Arc<Notify>,
    stalled: tokio::sync::mpsc::UnboundedSender<()>,
    received: u64,
    decoded: u64,
    skipped: u64,
}

impl Decoder {
    fn new(ends: Ends) -> Result<Decoder, String> {
        let mut transfer: *mut VTPixelTransferSession = std::ptr::null_mut();
        // SAFETY: the session comes back through the out-pointer, retained.
        let made = unsafe { VTPixelTransferSession::create(None, NonNull::from(&mut transfer)) };
        // SAFETY: what the call above wrote.
        let transfer = unsafe { created(made, transfer) }.ok_or("no pixel transfer session")?;
        Ok(Decoder {
            output: Box::new(Output { ring: ends.ring, transfer, source: None, failed: 0 }),
            session: None,
            format: None,
            sets: Default::default(),
            waiting_key: true,
            asked: None,
            asking_since: None,
            stall_said: false,
            keyframe: ends.keyframe,
            stalled: ends.stalled,
            received: 0,
            decoded: 0,
            skipped: 0,
        })
    }

    /// Asks the device for a whole picture, at most twice a second; says the stream stalled when
    /// asking has brought nothing for a while.
    fn ask_keyframe(&mut self) {
        let since = *self.asking_since.get_or_insert_with(Instant::now);
        if since.elapsed() >= STALLS_AFTER && !self.stall_said {
            self.stall_said = true;
            let _ = self.stalled.send(());
        }
        if self.asked.is_some_and(|at| at.elapsed() < ASK_EVERY) {
            return;
        }
        self.asked = Some(Instant::now());
        self.keyframe.notify_one();
    }

    /// One picture's bytes, as they came behind a marker.
    fn picture(&mut self, bytes: &[u8]) {
        self.received += 1;
        let units = units(bytes);
        let mut changed = false;
        for unit in units.iter().filter(|unit| (32..=34).contains(&unit.kind)) {
            let set = &mut self.sets[usize::from(unit.kind - 32)];
            if *set != bytes[unit.bytes.clone()] {
                *set = bytes[unit.bytes.clone()].to_vec();
                changed = true;
            }
        }
        if changed {
            self.make_session();
        }
        // Unwanted, pictures are dropped undecoded; but the first is decoded all the same: the
        // picture's size is how the device stands ready.
        let wanted = self.output.ring.wanted() || self.decoded == 0;
        if !wanted || (self.waiting_key && !key(&units)) {
            self.waiting_key = true;
            self.skipped += 1;
            if wanted {
                self.ask_keyframe();
            }
            return;
        }
        let slices = slices(bytes, &units);
        if slices.is_empty() {
            return;
        }
        let failed = self.output.failed;
        if !self.decode(slices) || self.output.failed != failed {
            self.waiting_key = true;
            self.ask_keyframe();
            return;
        }
        (self.waiting_key, self.asking_since, self.stall_said) = (false, None, false);
        self.decoded += 1;
    }

    /// Decodes one picture's slices, synchronously: whether the decoder took it.
    fn decode(&mut self, mut slices: Vec<u8>) -> bool {
        let (Some(session), Some(format)) = (&self.session, &self.format) else {
            return false;
        };
        let length = slices.len();
        let mut block: *mut CMBlockBuffer = std::ptr::null_mut();
        // SAFETY: the block refers to `slices`, alive until the synchronous decode returns; the
        // null allocator keeps CoreMedia from freeing it.
        let made = unsafe {
            CMBlockBuffer::create_with_memory_block(
                None,
                slices.as_mut_ptr().cast(),
                length,
                objc2_core_foundation::kCFAllocatorNull,
                std::ptr::null(),
                0,
                length,
                0,
                NonNull::from(&mut block),
            )
        };
        // SAFETY: what the call above wrote.
        let Some(block) = (unsafe { created(made, block) }) else {
            return false;
        };
        // SAFETY: the framework's constant, a plain value.
        let invalid = unsafe { kCMTimeInvalid };
        let timing = CMSampleTimingInfo { duration: invalid, presentationTimeStamp: invalid, decodeTimeStamp: invalid };
        let mut sample: *mut CMSampleBuffer = std::ptr::null_mut();
        // SAFETY: one sample of `length` bytes; the sample retains its block and format.
        let made = unsafe {
            CMSampleBuffer::create_ready(None, Some(&block), Some(format), 1, 1, &raw const timing, 1, &raw const length, NonNull::from(&mut sample))
        };
        // SAFETY: what the call above wrote.
        let Some(sample) = (unsafe { created(made, sample) }) else {
            return false;
        };
        // SAFETY: synchronous (no flags): the callback runs before this returns, on this thread,
        // with `output` alive; `slices` outlives the decode.
        let sent = unsafe { session.decode_frame(&sample, VTDecodeFrameFlags(0), std::ptr::null_mut(), std::ptr::null_mut()) };
        drop(slices);
        sent == 0
    }

    /// A decoder for the stream's parameter sets, once it has all three.
    fn make_session(&mut self) {
        if self.sets.iter().any(Vec::is_empty) {
            return;
        }
        let pointers: Vec<NonNull<u8>> = self.sets.iter().filter_map(|set| NonNull::new(set.as_ptr().cast_mut())).collect();
        let sizes: Vec<usize> = self.sets.iter().map(Vec::len).collect();
        let mut format: *const CMFormatDescription = std::ptr::null();
        // SAFETY: three parameter sets and their sizes; units behind four bytes of length.
        let made = unsafe {
            CMVideoFormatDescriptionCreateFromHEVCParameterSets(None, 3, NonNull::from(&pointers[0]), NonNull::from(&sizes[0]), 4, None, NonNull::from(&mut format))
        };
        // SAFETY: what the call above wrote.
        let Some(format) = (unsafe { created(made, format.cast_mut()) }).filter(|_| pointers.len() == 3) else {
            return self.output.ring.log_error(&format!("the stream's parameter sets weren't understood: {made}"));
        };
        self.end_session();
        let callback = VTDecompressionOutputCallbackRecord {
            decompressionOutputCallback: Some(decoded),
            decompressionOutputRefCon: std::ptr::from_mut::<Output>(&mut self.output).cast(),
        };
        let mut session: *mut VTDecompressionSession = std::ptr::null_mut();
        // SAFETY: the format above; the decoder's own output format, on surfaces of its own; the
        // callback's refcon is the boxed `Output`, which outlives the session.
        let made = unsafe { VTDecompressionSession::create(None, &format, None, None, &raw const callback, NonNull::from(&mut session)) };
        // SAFETY: what the call above wrote.
        let Some(session) = (unsafe { created(made, session) }) else {
            return self.output.ring.log_error(&format!("no decoder for the stream: {made}"));
        };
        // SAFETY: the session, and the framework's own key and value.
        unsafe { VTSessionSetProperty(&session, kVTDecompressionPropertyKey_RealTime, Some(CFBoolean::new(true))) };
        self.session = Some(session);
        self.format = Some(format);
        self.waiting_key = true;
    }

    fn end_session(&mut self) {
        if let Some(session) = self.session.take() {
            // SAFETY: the session, finished with.
            unsafe { session.invalidate() };
        }
    }

    fn counts(&self) -> String {
        format!(
            "pictures: {} received, {} decoded, {} skipped, {} failed",
            self.received, self.decoded, self.skipped, self.output.failed
        )
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        self.end_session();
    }
}

/// What the decoder is given and tells: the ring it paints, the device's keyframes asked for,
/// and a stalled stream said.
pub struct Ends {
    pub ring: Ring,
    pub keyframe: Arc<Notify>,
    pub stalled: tokio::sync::mpsc::UnboundedSender<()>,
}

/// Decodes the pictures that come on `pictures` until every sender has gone, on the thread it's
/// called on (the decoder's sessions stay on it); its counts logged every few seconds with
/// `stats`, and as it ends.
pub fn run(ends: Ends, pictures: &mpsc::Receiver<Vec<u8>>, log: &Teller, stats: bool) {
    let mut decoder = match Decoder::new(ends) {
        Ok(decoder) => decoder,
        Err(why) => return log.log(LogLevel::Error, format!("the device's pictures can't be decoded: {why}")),
    };
    let mut counted = Instant::now();
    while let Ok(picture) = pictures.recv() {
        objc2::rc::autoreleasepool(|_| decoder.picture(&picture));
        if stats && counted.elapsed() >= COUNT_EVERY {
            counted = Instant::now();
            log.log(LogLevel::Info, decoder.counts());
        }
    }
    log.log(LogLevel::Info, format!("the decoder ends: {}", decoder.counts()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pictures_units_are_told_apart_and_its_slices_put_behind_their_lengths() {
        // A VPS (32), an SPS (33), a PPS (34), an IDR slice (19), a delimiter (35).
        let mut bytes = Vec::new();
        for (kind, body) in [(32u8, &[1u8, 2][..]), (33, &[3, 4, 5]), (34, &[6]), (19, &[7, 8, 9, 10]), (35, &[0])] {
            bytes.extend_from_slice(&[0, 0, 0, 1, kind << 1]);
            bytes.extend_from_slice(body);
        }
        let found = units(&bytes);
        let kinds: Vec<u8> = found.iter().map(|unit| unit.kind).collect();
        assert_eq!(kinds, [32, 33, 34, 19, 35]);
        assert_eq!(&bytes[found[0].bytes.clone()], &[64u8, 1, 2]);
        assert!(key(&found));
        // The slice behind its length: 5 bytes (its header and four).
        assert_eq!(slices(&bytes, &found), [0, 0, 0, 5, 38, 7, 8, 9, 10]);
        // A slice of a picture that isn't whole: not a key.
        assert!(!key(&units(&[0, 0, 0, 1, 1 << 1, 9, 9])));
    }
}
