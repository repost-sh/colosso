//! A real device's own screen stream (`docs/devices.md` 5), as Device Hub's remote view starts
//! it: the sound first, then the video on the same session, over the device's display service.
//! The video comes as RTP; a picture is whole at the packet that carries the marker, and goes to
//! the decoder at once. The device is owed what Device Hub gives it (P13): every picture
//! acknowledged, a receiver's report every 50 ms, a word that this side is alive every second,
//! and a whole picture asked for when the decoder lost the thread. The sound is taken and dropped.
//!
//! The negotiation is adapted from idevice's `tools/src/coredevice_stream.rs` (MIT, Jackson
//! Coxson), by way of the proof of concept's `screen_stream.rs`, with the stream's own SSRC kept:
//! the feedback must carry it.


use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use idevice::core_device::{
    CallInfoBlob, DisplayServiceClient, HevcDepacketizer, RtpPacket, build_frame_ack, build_keyframe_request, build_liveness, build_rctl,
    build_screen_audio_offer, build_screen_video_offer, build_start_audio_parameters, build_start_video_parameters, is_rtcp,
};
use idevice::rsd::RsdHandshake;
use idevice::tcp::handle::{AdapterHandle, UdpSocketHandle};
use idevice::{ReadWrite, RsdService};
use tokio::sync::Notify;
use tokio::time::MissedTickBehavior;

/// What this side supports, as Device Hub says it for screen sharing: the device's own, larger
/// mask makes the negotiator offer a video it can't send.
const CLIENT_SUPPORTED_FEATURES: u64 = 140;
const REPORT_EVERY: Duration = Duration::from_millis(50);
const ALIVE_EVERY: Duration = Duration::from_secs(1);
/// The name this side gives in its feedback.
const NAME: &str = "koyo";

/// A stream under way: its service (it ends when this is dropped), and its sockets.
pub struct Stream {
    _service: DisplayServiceClient<Box<dyn ReadWrite>>,
    audio: UdpSocketHandle,
    video: UdpSocketHandle,
    ours: u32,
}

/// Starts the device's screen stream on its main display.
pub async fn start(adapter: &mut AdapterHandle, handshake: &mut RsdHandshake) -> Result<Stream, String> {
    let mut service = DisplayServiceClient::connect_rsd(adapter, handshake)
        .await
        .map_err(|error| format!("no display service: {error}"))?;
    let audio = adapter.bind_udp(0).await.map_err(|error| format!("no socket for the sound: {error}"))?;
    let video = adapter.bind_udp(0).await.map_err(|error| format!("no socket for the picture: {error}"))?;
    let (receiver, sender) = (adapter.host_ip().to_string(), adapter.peer_ip().to_string());
    // This end, as a Device Hub offer the device accepted describes it.
    let call = CallInfoBlob {
        call_id: 0,
        client_version: 1,
        device_type: "Mac17,7".into(),
        framework_version: "2205.3.1".into(),
        os_version: "25F71".into(),
        device_name: None,
        audio_device_uid: None,
    };
    let session = uuid::Uuid::new_v4();
    let call_id = || uuid::Uuid::new_v4().to_string().to_uppercase();
    let offer = build_screen_audio_offer(&call_id(), &call).map_err(|error| format!("the sound's offer: {error}"))?;
    let sound = build_start_audio_parameters(&receiver, audio.local_port(), &sender, 50000, offer, CLIENT_SUPPORTED_FEATURES, session);
    service.start_media_stream(sound).await.map_err(|error| format!("the device didn't start its sound: {error}"))?;
    let ours = u32::try_from(uuid::Uuid::new_v4().as_u128() & u128::from(u32::MAX)).unwrap_or(1);
    let offer = build_screen_video_offer(&call_id(), &call, ours).map_err(|error| format!("the picture's offer: {error}"))?;
    let picture = build_start_video_parameters(&receiver, video.local_port(), &sender, 50001, offer, CLIENT_SUPPORTED_FEATURES, 1, session);
    service.start_media_stream(picture).await.map_err(|error| format!("the device didn't start its picture: {error}"))?;
    Ok(Stream {
        _service: service,
        audio,
        video,
        ours,
    })
}

/// What the stream did, for the stats line.
#[derive(Default)]
pub struct Counts {
    pub pictures: u64,
    pub gaps: u64,
    pub keyframes: u64,
}

/// For QR12, in a debug build: `KOYO_DEVICE_HOST_DROP=<after ms>:<for ms>` drops the video
/// packets that come in that window of the host's first stream, once, so its decoding fails and
/// has to come back by a keyframe.
fn dropped_window() -> Option<(Duration, Duration)> {
    static TAKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !cfg!(debug_assertions) {
        return None;
    }
    let said = std::env::var("KOYO_DEVICE_HOST_DROP").ok()?;
    let (after, lasting) = said.split_once(':')?;
    let ms = |text: &str| text.trim().parse::<u64>().ok().map(Duration::from_millis);
    let window = (ms(after)?, ms(lasting)?);
    (!TAKEN.swap(true, std::sync::atomic::Ordering::Relaxed)).then_some(window)
}

/// Reads the stream until it ends, as a task of its own (nothing the session waits on holds it
/// up): each whole picture to `pictures`, the device given its feedback, a whole picture asked
/// for at each word on `keyframe`. Returns why it ended.
pub async fn pump(stream: Stream, pictures: mpsc::Sender<Vec<u8>>, keyframe: Arc<Notify>) -> (String, Counts) {
    let Stream { _service, audio, video, ours } = stream;
    // The sound is taken and dropped: the session is started with it.
    tokio::spawn(async move { while audio.recv().await.is_ok() {} });
    let mut depacketizer = HevcDepacketizer::new();
    let began = Instant::now();
    let mut report = tokio::time::interval(REPORT_EVERY);
    let mut alive = tokio::time::interval(ALIVE_EVERY);
    // A tick missed is skipped, not sent late in a burst.
    report.set_missed_tick_behavior(MissedTickBehavior::Skip);
    alive.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let (mut port, mut theirs, mut first, mut expected) = (None, 0u32, None, None);
    let (mut highest, mut frames) = (0u16, 0u16);
    let mut counts = Counts::default();
    let dropping = dropped_window();
    let why = loop {
        tokio::select! {
            got = video.recv() => {
                let Ok(datagram) = got else { break "the device's stream ended".to_owned() };
                if is_rtcp(&datagram.data) {
                    continue;
                }
                let Some(packet) = RtpPacket::parse(&datagram.data) else { continue };
                if dropping.is_some_and(|(after, lasting)| (after..after + lasting).contains(&began.elapsed())) {
                    continue;
                }
                port = Some(datagram.source_port);
                theirs = packet.ssrc;
                let base = *first.get_or_insert(packet.sequence_number);
                if expected.is_some_and(|expected| expected != packet.sequence_number) {
                    counts.gaps += 1;
                }
                expected = Some(packet.sequence_number.wrapping_add(1));
                highest = packet.sequence_number.wrapping_sub(base);
                depacketizer.push(packet.sequence_number, packet.timestamp, packet.payload);
                // A picture's last packet carries the marker: what is whole goes at once.
                if packet.marker {
                    frames = frames.wrapping_add(1);
                    counts.pictures += 1;
                    let _ = video.send_to(datagram.source_port, build_frame_ack(ours, packet.timestamp)).await;
                    let picture = depacketizer.take_output();
                    if !picture.is_empty() && pictures.send(picture).is_err() {
                        break "the decoder went".to_owned();
                    }
                }
            }
            _ = report.tick() => {
                if let Some(port) = port {
                    let elapsed = u16::try_from(began.elapsed().as_millis() & 0xffff).unwrap_or(0);
                    let _ = video.send_to(port, build_rctl(ours, elapsed, frames, highest)).await;
                }
            }
            _ = alive.tick() => {
                if let Some(port) = port {
                    let _ = video.send_to(port, build_liveness(ours, NAME, &[])).await;
                }
            }
            () = keyframe.notified() => {
                if let Some(port) = port {
                    counts.keyframes += 1;
                    let number = u8::try_from(counts.keyframes & 0xff).unwrap_or(0);
                    let _ = video.send_to(port, build_keyframe_request(ours, NAME, theirs, &[], number)).await;
                }
            }
        }
    };
    (why, counts)
}
