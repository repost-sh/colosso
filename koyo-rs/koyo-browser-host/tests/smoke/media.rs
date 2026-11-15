//! A page's `<video>` plays H.264 with AAC in MP4, which `WhatsApp`'s video and most of the
//! web's is in: the clip runs, its sound is decoded and its picture is what the page draws. An engine packaged from CEF's own build can't, and fails this with the engine's
//! reason; one whose framework `script/cef` built does (docs/browser.md 11).
//!
//! The clip is a second of a picture white on its left half and black on its right, with a
//! tone, in base64 so that the page can carry it in its address, as the other tests' pages
//! carry theirs:
//!
//! ```sh
//! ffmpeg -f lavfi -i color=c=white:s=48x54:r=12 -f lavfi -i color=c=black:s=48x54:r=12 \
//!   -f lavfi -i sine=frequency=440:sample_rate=22050 \
//!   -filter_complex '[0:v][1:v]hstack=inputs=2[v]' -map '[v]' -map 2:a -t 1 \
//!   -c:v libx264 -profile:v baseline -pix_fmt yuv420p -g 12 -crf 30 \
//!   -c:a aac -b:a 24k -ac 1 -movflags +faststart -map_metadata -1 clip.mp4
//! base64 -i clip.mp4 | tr -d '\n' > h264-aac.mp4.base64
//! ```
use crate::support::{Session, close, launch, open, until};
use std::time::Duration;

/// A page, blue where nothing is drawn, that the clip fills and plays over and over as soon
/// as it loads, muted, as a page may without being asked.
const PAGE: &str = concat!(
    "data:text/html,<title>media</title><body style='margin:0;background:rgb(0,128,255)'>",
    "<video muted autoplay loop style='display:block;width:100vw;height:100vh;object-fit:fill' ",
    "src='data:video/mp4;base64,",
    include_str!("h264-aac.mp4.base64"),
    "'></video>"
);

/// How the page's clip is getting on: how far in it is, the width of its picture, how much
/// of its sound is decoded, and the engine's reason once it has given up.
const CLIP: &str = "(video => JSON.stringify({ seconds: video.currentTime, width: video.videoWidth, \
    sound: video.webkitAudioDecodedByteCount, reason: video.error && video.error.message }))\
    (document.querySelector('video'))";

/// The red, green and blue of the clip's picture a quarter and three quarters of the way
/// along its middle row, as the page draws the clip to a canvas; `null` until it has one.
const PICTURE: &str = "(video => { if (!video.videoWidth) return 'null'; \
    const canvas = document.createElement('canvas'); canvas.width = 96; canvas.height = 54; \
    const pen = canvas.getContext('2d'); pen.drawImage(video, 0, 0, 96, 54); \
    const at = x => Array.from(pen.getImageData(x, 27, 1, 1).data.slice(0, 3)); \
    return JSON.stringify([...at(24), ...at(72)]); })(document.querySelector('video'))";

#[test]
#[ignore = "Needs a packaged browser engine whose framework script/cef built; see the module docs"]
fn a_page_plays_h264_with_aac() {
    let launch = launch("media");
    let mut session = Session::start(&launch);
    open(&mut session, PAGE, "media");
    let clip = until(
        &mut session,
        CLIP,
        "the clip to play or the engine to give up on it",
        Duration::from_secs(15),
        |clip| clip["seconds"].as_f64() > Some(0.2) || !clip["reason"].is_null(),
    );
    assert!(
        clip["reason"].is_null(),
        "the engine has no H.264 or AAC, as CEF's own build hasn't (script/cef builds one that \
         has): {}",
        clip["reason"]
    );
    assert_eq!(clip["width"], 96, "no H.264 picture: {clip}");
    assert!(clip["sound"].as_u64() > Some(0), "no AAC sound: {clip}");
    // The picture is the clip's: drawn to a canvas, white a quarter of the way along its
    // middle row and black three quarters.
    let picture = until(
        &mut session,
        PICTURE,
        "the clip's picture",
        Duration::from_secs(10),
        serde_json::Value::is_array,
    );
    let at = |index: usize| picture[index].as_u64().expect("a colour part");
    assert!((0..3).all(|part| at(part) > 200), "not white on its left: {picture}");
    assert!((3..6).all(|part| at(part) < 60), "not black on its right: {picture}");
    close(session, &launch);
}
