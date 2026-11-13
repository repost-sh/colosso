//! A page's `<video>` plays H.264 with AAC in MP4, which `WhatsApp`'s video and most of the
//! web's is in: the clip runs, its sound is decoded and its picture reaches the surface the
//! app shows. An engine packaged from CEF's own build can't, and fails this with the engine's
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
use koyo_browser_ipc::HostEvent;
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
    // The picture is in the frames the app is handed: along the middle row, white a quarter
    // of the way in and black three quarters, where a page that drew no clip is blue.
    session.wait("the clip's picture in the page's frame", |event, seen| {
        matches!(event, HostEvent::Frame { pane: 1, .. })
            && seen.is_some_and(|seen| light(seen.row[25]) && dark(seen.row[75]))
    });
    close(session, &launch);
}

/// Whether a BGRA pixel is near white.
fn light(pixel: [u8; 4]) -> bool {
    pixel[..3].iter().all(|&part| part > 200)
}

/// Whether a BGRA pixel is near black.
fn dark(pixel: [u8; 4]) -> bool {
    pixel[..3].iter().all(|&part| part < 60)
}
