//! The log's line for each change in a page's sight and sound (`koyo::sight`): the page starts
//! playing, is hidden to the engine, stops playing, and is shown again, each said once.

use std::sync::Mutex;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use koyo_browser_ipc::client::{Connection, Incoming};

use crate::asking::{evaluate, kept};
use crate::inside::open;
use crate::pump::idle;

/// The page plays two seconds of a quiet tone over and over, in an `<audio>` of its own, as
/// the person had pressed play.
const PLAY: &str = "(async () => { const rate = 8000, n = rate * 2; \
    const b = new DataView(new ArrayBuffer(44 + n * 2)); \
    const s = (at, text) => [...text].forEach((c, i) => b.setUint8(at + i, c.charCodeAt(0))); \
    s(0, 'RIFF'); b.setUint32(4, 36 + n * 2, true); s(8, 'WAVE'); s(12, 'fmt '); \
    b.setUint32(16, 16, true); b.setUint16(20, 1, true); b.setUint16(22, 1, true); \
    b.setUint32(24, rate, true); b.setUint32(28, rate * 2, true); b.setUint16(32, 2, true); \
    b.setUint16(34, 16, true); s(36, 'data'); b.setUint32(40, n * 2, true); \
    for (let i = 0; i < n; i++) b.setInt16(44 + i * 2, Math.sin(i * 2 * Math.PI * 440 / rate) * 8000, true); \
    const a = document.createElement('audio'); a.loop = true; a.volume = 0.01; \
    a.src = URL.createObjectURL(new Blob([b], { type: 'audio/wav' })); \
    document.body.append(a); await a.play(); return !a.paused; })()";

/// The page pauses its tone.
const PAUSE: &str = "document.querySelector('audio').pause(); true";

/// The log's lines of `koyo::sight`, kept as they come.
static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Kept;

impl log::Log for Kept {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == "koyo::sight"
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata())
            && let Ok(mut lines) = LINES.lock()
        {
            lines.push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

/// The page plays, is hidden, stops and is shown: a line for each, in that order, and no other.
pub fn each_change_once(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    log::set_logger(&Kept).map_err(|error| format!("a logger: {error}"))?;
    log::set_max_level(log::LevelFilter::Info);
    open(host, incoming)?;
    let played = evaluate(host, incoming, PLAY)?;
    if played != true {
        return Err(format!("the page didn't play: {played}"));
    }
    said("pane 1 (data:) plays sound")?;
    kept(host, false)?;
    said("pane 1 (data:) hidden to the engine")?;
    evaluate(host, incoming, PAUSE)?;
    said("pane 1 (data:) stopped playing: pause")?;
    kept(host, true)?;
    said("pane 1 (data:) shown to the engine")?;
    idle(Duration::from_secs(1));
    let lines = LINES.lock().map_err(|error| error.to_string())?.clone();
    if lines.len() != 4 {
        return Err(format!("the log said, of four changes: {lines:#?}"));
    }
    eprintln!("the log said: {lines:#?}");
    Ok(())
}

/// Waits for the log to have said `line`.
fn said(line: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let lines = LINES.lock().map_err(|error| error.to_string())?.clone();
        if lines.iter().any(|said| said == line) {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!("the log never said {line:?}, only {lines:#?}"));
        }
        idle(Duration::from_millis(50));
    }
}
