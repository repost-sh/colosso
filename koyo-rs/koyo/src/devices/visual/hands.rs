//! The rows of a real iPad that need the owner's hands (`docs/devices.md` 5, M6), through the
//! app with `KOYO_VISUAL_PHONE=hands`, in the order he's asked to do them: turned four ways
//! (QR4), each followed and its corner landing; locked and unlocked (QR7); unplugged and plugged
//! back (QR8), the tab saying so and coming back by itself; a pause for his look at its camera
//! (U14); then left alone twelve minutes with its tab hidden (QR6), and its own lock once the tab
//! is closed. The only acts are approval D's: Control Centre opened from the corner and closed
//! with Home. Only states, sizes and times are said, each as it happens, to
//! `KOYO_VISUAL_PHONE_LOG`: a step missed is a FAIL and the walk goes on.

use std::time::{Duration, Instant};

use gpui_kit::Entity;
use koyo_device_wire::{Act, Answer, Ask, Change, Orientation, Point, Press, Refusal, RefusalCode, Screen, Spot};

use super::{close, open};
use crate::devices::view::DeviceView;
use crate::devices::{Link, Standing};
use crate::visual::{Drawn, Result};

/// How long each of the owner's steps is waited for.
const STEP: Duration = Duration::from_mins(5);
/// How long a turn's picture is given to settle: the host reads the turn every two seconds.
const SETTLES: Duration = Duration::from_secs(5);
/// The hold that keeps the device awake, with its tab hidden (QR6).
const HELD: Duration = Duration::from_mins(12);
/// How long its own lock is watched for once the tab is closed: past any Auto-Lock but Never.
const LOCKS_WITHIN: Duration = Duration::from_mins(20);

/// A line of the walk's, written as it happens.
fn note(line: &str) {
    if let Some(log) = std::env::var_os("KOYO_VISUAL_PHONE_LOG") {
        use std::io::Write as _;
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
        let file = std::fs::OpenOptions::new().create(true).append(true).open(log);
        let _ = file.and_then(|mut file| writeln!(file, "{at} {line}"));
    }
}

/// A row, kept and written.
fn say(lines: &mut Vec<String>, row: &str, what: &str, got: &str, pass: bool) {
    let line = format!("{row} | {what} | {got} | {}", if pass { "PASS" } else { "FAIL" });
    note(&line);
    lines.push(line);
}

/// Waits up to `limit` for `wanted` of the link; how long it took, or `None` if it didn't come.
fn until(drawn: &mut Drawn, link: &Entity<Link>, limit: Duration, wanted: impl Fn(&Link) -> bool) -> Option<Duration> {
    let began = Instant::now();
    let link = link.clone();
    drawn.until(limit, move |drawn| Ok(drawn.cx.read(|cx| wanted(link.read(cx))))).ok()?;
    Some(began.elapsed())
}

fn locked(link: &Link) -> bool {
    matches!(link.standing(), Standing::Locked(_))
}

/// Asks the device's host `ask` as an agent's tool would, and waits for its answer.
fn asked(drawn: &mut Drawn, link: &Entity<Link>, ask: Ask) -> std::result::Result<Answer, Refusal> {
    let limit = Duration::from_secs(20);
    let task = drawn.cx.update(|cx| link.update(cx, |link, cx| link.ask(ask, limit, cx)));
    let mut got = None;
    let finished = drawn.finish(limit + Duration::from_secs(2), async { got = Some(task.await) });
    match (finished, got) {
        (Ok(()), Some(answer)) => answer,
        _ => Err(Refusal::new(RefusalCode::TimedOut, "no answer")),
    }
}

/// What the tab says now.
fn notice(drawn: &mut Drawn, view: &Entity<DeviceView>) -> String {
    drawn.cx.update(|cx| view.update(cx, |view, cx| view.notice(cx))).map(|notice| notice.message).unwrap_or_default()
}

/// QR4: Control Centre from the top right corner of the picture as it shows, closed with Home.
fn corner(drawn: &mut Drawn, link: &Entity<Link>, screen: &Screen, lines: &mut Vec<String>) {
    let (w, h) = (screen.size.width, screen.size.height);
    let swipe = Act::Swipe {
        from: Spot::Point(Point { x: w - 24., y: 2. }),
        to: Spot::Point(Point { x: w - 24., y: h * 0.45 }),
        ms: 350,
        fingers: 1,
    };
    let opened = asked(drawn, link, Ask::Act(swipe));
    let closed = asked(drawn, link, Ask::Act(Act::Button { button: Press::Home }));
    let change = |answer: &std::result::Result<Answer, Refusal>| match answer {
        Ok(Answer::Acted { change, .. }) => format!("{change:?}"),
        other => format!("{other:?}"),
    };
    let large = matches!(opened, Ok(Answer::Acted { change: Change::Large, .. }));
    let got = format!("opened: {}; closed: {}", change(&opened), change(&closed));
    say(lines, "QR4", &format!("{:?}: the corner lands", screen.orientation), &got, large);
}

/// Whether the picture's sides are as the way it's held says.
fn upright(screen: &Screen) -> bool {
    let landscape = matches!(screen.orientation, Orientation::LandscapeLeft | Orientation::LandscapeRight);
    (screen.size.width > screen.size.height) == landscape
}

/// QR4: three more ways it's held, each followed with its sides as the way says, and its corner.
fn turns(drawn: &mut Drawn, link: &Entity<Link>, first: Screen, lines: &mut Vec<String>) {
    let mut seen: Vec<Orientation> = vec![first.orientation];
    corner(drawn, link, &first, lines);
    while seen.len() < 4 {
        note(&format!("QR4: turn it a way not yet seen ({seen:?} so far)"));
        let known = seen.clone();
        let new = move |link: &Link| link.ready() && link.screen().is_some_and(|screen| !known.contains(&screen.orientation));
        if until(drawn, link, STEP, new).is_none() {
            return say(lines, "QR4", "four ways", &format!("only {seen:?} in {} s", STEP.as_secs()), false);
        }
        // The way it's held is read first; its picture follows within a read of the turn.
        let settled = until(drawn, link, SETTLES, |link| link.screen().is_some_and(|screen| upright(&screen)));
        let Some(screen) = drawn.cx.read(|cx| link.read(cx).screen()) else {
            continue;
        };
        seen.push(screen.orientation);
        let got = format!(
            "{:.0}x{:.0} points, upright {}",
            screen.size.width,
            screen.size.height,
            settled.map_or_else(|| format!("not within {} s", SETTLES.as_secs()), |took| format!("{} ms after", took.as_millis()))
        );
        say(lines, "QR4", &format!("{:?}: followed", screen.orientation), &got, settled.is_some_and(|took| took <= Duration::from_secs(2)));
        corner(drawn, link, &screen, lines);
    }
}

/// QR7: locked by his hand, the tab says so and an act is refused at once; unlocked, it's back.
fn lock(drawn: &mut Drawn, view: &Entity<DeviceView>, link: &Entity<Link>, lines: &mut Vec<String>) {
    note("QR7: lock it now, then unlock it");
    if until(drawn, link, STEP, locked).is_none() {
        return say(lines, "QR7", "locked", &format!("not seen in {} s", STEP.as_secs()), false);
    }
    let said = notice(drawn, view);
    say(lines, "QR7", "locked: the tab says", &format!("{said:?}"), said.starts_with("Unlock your iPad"));
    let asking = Instant::now();
    let refused = asked(drawn, link, Ask::Act(Act::Button { button: Press::Home }));
    let at_once = asking.elapsed();
    let code = refused.as_ref().err().map(|refusal| refusal.code);
    let pass = code == Some(RefusalCode::Locked) && at_once < Duration::from_secs(1);
    say(lines, "QR7", "an act while locked", &format!("{code:?} in {} ms", at_once.as_millis()), pass);
    let back = until(drawn, link, STEP, Link::ready);
    let got = back.map_or_else(|| format!("not back in {} s", STEP.as_secs()), |took| format!("ready {} s after the lock was seen", took.as_secs()));
    say(lines, "QR7", "unlocked: back by itself", &got, back.is_some());
}

/// QR8: unplugged, the tab says so; plugged back in, it comes back by itself.
fn plug(drawn: &mut Drawn, view: &Entity<DeviceView>, link: &Entity<Link>, lines: &mut Vec<String>) {
    note("QR8: unplug it now, then plug it back in");
    let unplugged = |link: &Link| matches!(link.standing(), Standing::Ended { missing: None, .. });
    if until(drawn, link, STEP, unplugged).is_none() {
        let standing = drawn.cx.read(|cx| format!("{:?}", link.read(cx).standing()));
        return say(lines, "QR8", "unplugged", &format!("not seen in {} s: {standing}", STEP.as_secs()), false);
    }
    let said = notice(drawn, view);
    say(lines, "QR8", "unplugged: the tab says", &format!("{said:?}"), said.contains("was unplugged"));
    let back = until(drawn, link, STEP, |link| link.ready() || locked(link));
    let got = back.map_or_else(|| format!("not back in {} s", STEP.as_secs()), |took| format!("{} s after the unplug was seen", took.as_secs()));
    say(lines, "QR8", "plugged back: back by itself", &got, back.is_some());
}

/// QR6: twelve minutes with its tab open and hidden, nothing sent: still unlocked and running.
/// Then the tab closed, and how long its own lock takes.
fn awake(drawn: &mut Drawn, view: Entity<DeviceView>, link: &Entity<Link>, udid: &str, lines: &mut Vec<String>) -> Result<()> {
    note("QR6: leave it alone now, for about 25 minutes");
    let _picker = open(drawn, None, false)?;
    let went = until(drawn, link, HELD, |link| locked(link) || !link.ready());
    let standing = drawn.cx.read(|cx| format!("{:?}", link.read(cx).standing()));
    let got = went.map_or_else(|| format!("still unlocked after {} s", HELD.as_secs()), |after| format!("{standing} after {} s", after.as_secs()));
    say(lines, "QR6", "twelve minutes with the tab hidden", &got, went.is_none());
    close(drawn, view)?;
    let scratch = crate::devices::state_dir().ok_or("no devices folder")?;
    let closed = Instant::now();
    let args = ["device", "info", "lockState", "--device", udid];
    let (mut locked_after, mut last) = (None, String::new());
    while closed.elapsed() < LOCKS_WITHIN {
        let _ = drawn.until(Duration::from_secs(15), |_| Ok(false));
        match koyo_device_wire::real::devicectl(&args, &scratch) {
            Ok(state) if state["result"]["passcodeRequired"] == true => {
                locked_after = Some(closed.elapsed());
                break;
            }
            Ok(_) => {}
            Err(why) => last = why,
        }
    }
    let got = locked_after.map_or_else(
        || format!("not locked in {} s{}", LOCKS_WITHIN.as_secs(), if last.is_empty() { String::new() } else { format!(" ({last})") }),
        |after| format!("locked {} s after", after.as_secs()),
    );
    say(lines, "QR6", "the tab closed: its own lock", &got, locked_after.is_some());
    Ok(())
}

/// The owner's rows, on the iPad `udid` shown in `view`, standing ready with `first`; `short`,
/// the turns, the lock and the plug only, and the tab closed after.
pub(super) fn run(drawn: &mut Drawn, view: Entity<DeviceView>, link: &Entity<Link>, udid: &str, first: Screen, short: bool, lines: &mut Vec<String>) -> Result<()> {
    note(&format!("ready: {:?}; QR4 first", first.orientation));
    turns(drawn, link, first, lines);
    lock(drawn, &view, link, lines);
    plug(drawn, &view, link, lines);
    if short {
        note("done");
        return close(drawn, view);
    }
    // U14: his look at its camera and sound, with the tab shown.
    note("U14: open the Camera app, look, then go back Home");
    let _ = drawn.until(Duration::from_secs(90), |_| Ok(false));
    awake(drawn, view, link, udid, lines)
}
