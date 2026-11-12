//! What the real-device rows share: each act noted for the final report, what an act answered
//! in a few words, where a word reads on the screen (its place only), a tap, a press, the
//! device's own `devicectl`, a turn's sides, Control Centre from the corner, and a picture's
//! light as numbers. Nothing the device holds is kept.

use std::time::Duration;

use koyo_device_wire::{Act, Answer, Change, Orientation, Point, Press, Refusal, Screen, Source, Spot};

use super::Driver;

/// Notes an act done on the device, as the final report lists every one.
pub fn act(what: &str) {
    super::append("KOYO_REAL_PHONE_ACTS", &format!("- {} real_phone: {what}", super::now()));
}

/// What an act answered, in a few words.
pub fn said(answer: &Result<Answer, Refusal>) -> String {
    match answer {
        Ok(Answer::Acted { change, millis, .. }) => format!("{change:?} in {millis} ms"),
        other => format!("{other:?}"),
    }
}

/// Where a text snapshot reads `word`, in points: its place only, nothing else of the text.
pub fn place_of(host: &mut Driver, word: &str) -> Option<Point> {
    let Ok(Answer::Snapshot { tree, source, .. }) = host.snapshot() else {
        return None;
    };
    assert_eq!(source, Source::Text, "a real device has no tree");
    place(&tree, word)
}

pub fn tap(host: &mut Driver, at: Point) -> Result<Answer, Refusal> {
    host.act(Act::Tap {
        spot: Spot::Point(at),
        count: 1,
        hold_ms: 0,
        fingers: 1,
    })
}

pub fn press(host: &mut Driver, button: Press) -> Result<Answer, Refusal> {
    host.act(Act::Button { button })
}

/// Runs `devicectl device <what> <args>` on the device `udid`, quietly, for 15 s at most; what
/// went wrong. The device is named before `args`: after a bundle id, they'd be the app's.
pub fn devicectl(udid: &str, what: &[&str], args: &[&str]) -> Result<(), String> {
    let mut command = std::process::Command::new("/usr/bin/xcrun");
    command.args(["devicectl", "device"]).args(what).args(["--device", udid, "--quiet"]).args(args);
    koyo_device_wire::process::output(command, Duration::from_secs(15)).map(drop)
}

/// Whether the picture's sides are as the way it's held says.
pub fn upright(screen: &Screen) -> bool {
    let landscape = matches!(screen.orientation, Orientation::LandscapeLeft | Orientation::LandscapeRight);
    (screen.size.width > screen.size.height) == landscape
}

/// Control Centre opened by a swipe down from the top right corner, and closed with Home.
pub fn corner(host: &mut Driver, screen: &Screen) {
    let (w, h) = (screen.size.width, screen.size.height);
    act(&format!("Control Centre opened from the top right corner ({:?})", screen.orientation));
    let opened = host.act(Act::Swipe {
        from: Spot::Point(Point { x: w - 24., y: 2. }),
        to: Spot::Point(Point { x: w - 24., y: h * 0.45 }),
        ms: 350,
        fingers: 1,
    });
    act("Home pressed (Control Centre closed)");
    let closed = press(host, Press::Home);
    let large = matches!(opened, Ok(Answer::Acted { change: Change::Large, .. }));
    super::row("QR4", &format!("{:?}: the corner lands", screen.orientation), &format!("opened: {}; closed: {}", said(&opened), said(&closed)), large);
}

/// The light of the middle half of a picture, as its mean and spread (0 to 255): numbers only,
/// the picture itself is dropped.
pub fn middle_light(png: &[u8]) -> Option<(f64, f64)> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut pixels).ok()?;
    let (width, height, step) = (info.width as usize, info.height as usize, info.color_type.samples());
    let light: Vec<f64> = (height / 4..height * 3 / 4)
        .flat_map(|y| (width / 4..width * 3 / 4).map(move |x| (y * width + x) * step))
        .filter_map(|at| Some(0.299 * f64::from(*pixels.get(at)?) + 0.587 * f64::from(*pixels.get(at + 1)?) + 0.114 * f64::from(*pixels.get(at + 2)?)))
        .collect();
    let count = light.len() as f64;
    let mean = light.iter().sum::<f64>() / count;
    let spread = (light.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / count).sqrt();
    Some((mean, spread))
}

/// Where a snapshot's first line that reads `text` places its element, in points.
fn place(tree: &str, text: &str) -> Option<Point> {
    let line = tree.lines().find(|line| line.contains(&format!("{text:?}")))?;
    let (_, at) = line.rsplit_once('@')?;
    let (x, y) = at.split_once(',')?;
    Some(Point {
        x: x.trim().parse().ok()?,
        y: y.split_whitespace().next()?.parse().ok()?,
    })
}
