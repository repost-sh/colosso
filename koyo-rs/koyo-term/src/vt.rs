//! The VT thread (`docs/terminals.md` 7.2, step 2): one per terminal view, owning the view's
//! engine. It restores the engine from the daemon's snapshot, feeds it the output that follows,
//! turns the view's keys, mouse and wheel into the bytes the program reads, and publishes an
//! owned frame for the view at most once per display refresh, however fast output arrives.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use koyo_term_control::{Connection, EventKind, Handle, Output, RequestBody, SnapshotChunk};
use koyo_term_engine::{
    Effect, Engine, Frame, Geometry, GridPoint, KeyAction, MouseInput, OptionAsAlt, Rgb, Scroll,
    SelectUnit, Size,
};
use crossbeam_channel::{Receiver, Sender, never, select};
use futures::channel::mpsc::UnboundedSender;
use parking_lot::Mutex;

use crate::client::Stream;
use crate::keys::OwnedKey;

/// The most often a frame goes out: a 120 Hz display's refresh.
const FRAME_INTERVAL: Duration = Duration::from_micros(8_333);
/// How long a program may hold the screen still (synchronized output) before it's shown
/// regardless.
const RENDER_HOLD_LIMIT: Duration = Duration::from_secs(1);
/// History the view's engine keeps, Ghostty's default.
const SCROLLBACK_BYTES: usize = 50 << 20;

/// What the view asks of its VT thread.
#[derive(Debug)]
pub enum Command {
    Key(OwnedKey),
    /// Text an input method committed, or a typed character the platform composed.
    Text(String),
    Mouse(MouseInput, Geometry),
    /// The wheel, in rows, negative being up, at a cell, with the modifiers held.
    Wheel {
        rows: i32,
        at: MouseInput,
        geometry: Geometry,
    },
    Scroll(Scroll),
    Select {
        anchor: GridPoint,
        head: GridPoint,
        unit: SelectUnit,
        rectangle: bool,
    },
    ClearSelection,
    /// The selected text, sent back on the channel.
    Copy(Sender<Option<String>>),
    /// Text pasted, which the daemon brackets as the program asked. It goes from here so it
    /// reaches the program in order with the keys around it.
    Paste {
        handle: Handle,
        text: String,
    },
    Focus(bool),
    Colors {
        foreground: Rgb,
        background: Rgb,
    },
    OptionAsAlt(OptionAsAlt),
}

/// What the VT thread tells its view.
#[derive(Debug)]
pub enum Update {
    /// A new frame waits in the slot.
    Frame,
    Event(Box<EventKind>),
    /// The program sent the daemon nothing new since the stream fell behind; a snapshot is
    /// on its way.
    Lagging,
    Disconnected,
}

/// The view's side of its VT thread.
pub struct Vt {
    commands: Sender<Command>,
    slot: Arc<Slot>,
}

struct Slot {
    frame: Mutex<Option<Arc<Frame>>>,
    /// An `Update::Frame` is on its way and the view hasn't taken the frame yet.
    signalled: AtomicBool,
}

impl Vt {
    /// Starts the thread for the terminal attached at `index`, its stream on `stream`.
    pub fn start(
        name: &str,
        index: u32,
        connection: Arc<Connection>,
        stream: Receiver<Stream>,
        updates: UnboundedSender<Update>,
    ) -> std::io::Result<Vt> {
        let (commands, received) = crossbeam_channel::unbounded();
        let slot = Arc::new(Slot {
            frame: Mutex::new(None),
            signalled: AtomicBool::new(false),
        });
        let for_thread = Arc::clone(&slot);
        // The engine can't cross threads, so the thread's state is made on the thread.
        std::thread::Builder::new()
            .name(format!("term-vt {name}"))
            .spawn(move || {
                let thread = Thread {
                    index,
                    connection,
                    engine: None,
                    snapshot: Vec::new(),
                    offset: 0,
                    dirty: false,
                    last_publish: None,
                    hold_until: None,
                    colors: None,
                    option_as_alt: OptionAsAlt::default(),
                    updates,
                    slot: for_thread,
                };
                thread.run(&stream, &received);
            })?;
        Ok(Vt { commands, slot })
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// The newest frame, when one was published since the last call.
    pub fn take_frame(&self) -> Option<Arc<Frame>> {
        self.slot.signalled.store(false, Ordering::Release);
        self.slot.frame.lock().take()
    }
}

struct Thread {
    index: u32,
    connection: Arc<Connection>,
    /// `None` until the first snapshot arrives.
    engine: Option<Engine>,
    snapshot: Vec<u8>,
    /// The next offset of the terminal's output the engine hasn't seen.
    offset: u64,
    /// The engine changed since the last frame went out.
    dirty: bool,
    last_publish: Option<Instant>,
    /// Synchronized output holds frames back until then.
    hold_until: Option<Instant>,
    colors: Option<(Rgb, Rgb)>,
    option_as_alt: OptionAsAlt,
    updates: UnboundedSender<Update>,
    slot: Arc<Slot>,
}

impl Thread {
    fn run(mut self, stream: &Receiver<Stream>, commands: &Receiver<Command>) {
        let mut stream = stream.clone();
        loop {
            let deadline = self.next_frame_at();
            let timer = deadline.map_or_else(never, crossbeam_channel::at);
            select! {
                recv(stream) -> message => match message {
                    Ok(message) => self.stream(message),
                    Err(_) => {
                        // The client let this terminal go.
                        stream = never();
                    }
                },
                recv(commands) -> command => match command {
                    Ok(command) => self.command(command),
                    // The view is gone.
                    Err(_) => return,
                },
                recv(timer) -> _ => {}
            }
            if self.next_frame_at().is_some_and(|at| at <= Instant::now()) {
                self.publish();
            }
        }
    }

    /// When the next frame should go out, if one is due.
    fn next_frame_at(&self) -> Option<Instant> {
        if !self.dirty || self.engine.is_none() {
            return None;
        }
        let paced = self
            .last_publish
            .map_or_else(Instant::now, |last| last + FRAME_INTERVAL);
        Some(self.hold_until.map_or(paced, |hold| hold.max(paced)))
    }

    fn publish(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        match engine.frame() {
            Ok(frame) => {
                log::trace!(
                    "attach {}: a frame of {} rows",
                    self.index,
                    frame.rows.len()
                );
                *self.slot.frame.lock() = Some(Arc::new(frame));
                if !self.slot.signalled.swap(true, Ordering::AcqRel) {
                    let _ = self.updates.unbounded_send(Update::Frame);
                }
            }
            Err(error) => log::warn!("couldn't read a frame: {error}"),
        }
        self.dirty = false;
        self.last_publish = Some(Instant::now());
    }

    fn stream(&mut self, message: Stream) {
        match message {
            Stream::Output(output) => self.output(&output),
            Stream::Snapshot(chunk) => self.snapshot(&chunk),
            Stream::Lagging => {
                let _ = self.updates.unbounded_send(Update::Lagging);
            }
            Stream::Event(EventKind::Size { size, pinned }) => {
                // The view lays a pinned grid out from it.
                let event = EventKind::Size { size, pinned };
                let _ = self.updates.unbounded_send(Update::Event(Box::new(event)));
                if let Some(engine) = &mut self.engine
                    && (engine.cols(), engine.rows()) != (size.cols, size.rows)
                {
                    let resized = Size {
                        cols: size.cols,
                        rows: size.rows,
                        cell_width_px: size.cell_width_px,
                        cell_height_px: size.cell_height_px,
                    };
                    if let Err(error) = engine.resize(resized) {
                        log::warn!("couldn't resize the view's engine: {error}");
                    }
                    self.dirty = true;
                }
            }
            Stream::Event(kind) => {
                let _ = self.updates.unbounded_send(Update::Event(Box::new(kind)));
            }
            Stream::Disconnected => {
                let _ = self.updates.unbounded_send(Update::Disconnected);
            }
        }
    }

    fn output(&mut self, output: &Output) {
        let end = output.offset + output.bytes.len() as u64;
        if end <= self.offset {
            return;
        }
        if self.engine.is_none() {
            // Output with no snapshot before it: the daemon replayed its ring instead.
            match Engine::new(Size::cells(80, 24), SCROLLBACK_BYTES) {
                Ok(engine) => self.engine = Some(engine),
                Err(error) => return log::error!("couldn't make an engine: {error}"),
            }
            self.apply_colors();
        }
        let skip = self.offset.saturating_sub(output.offset) as usize;
        if let Some(engine) = &mut self.engine {
            engine.write(&output.bytes[skip.min(output.bytes.len())..]);
        }
        self.offset = end;
        self.effects();
        self.dirty = true;
    }

    fn snapshot(&mut self, chunk: &SnapshotChunk) {
        if chunk.chunk == 0 {
            self.snapshot.clear();
        }
        self.snapshot.extend_from_slice(&chunk.bytes);
        if chunk.chunk + 1 < chunk.of {
            return;
        }
        let bytes = std::mem::take(&mut self.snapshot);
        let restored = if bytes.is_empty() {
            Engine::new(Size::cells(80, 24), SCROLLBACK_BYTES)
        } else {
            Engine::restore(&bytes)
        };
        match restored {
            Ok(engine) => {
                log::debug!(
                    "attach {}: restored a snapshot at {}",
                    self.index,
                    chunk.offset
                );
                self.engine = Some(engine);
                self.offset = chunk.offset;
                self.apply_colors();
                // Only what follows the snapshot is news: a restored screen rings no bells.
                if let Some(engine) = &mut self.engine {
                    engine.take_effects();
                }
                self.dirty = true;
            }
            Err(error) => log::error!("couldn't restore the terminal's snapshot: {error}"),
        }
    }

    fn effects(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        for effect in engine.take_effects() {
            match effect {
                Effect::RenderHold(true) => {
                    self.hold_until = Some(Instant::now() + RENDER_HOLD_LIMIT);
                }
                Effect::RenderHold(false) => self.hold_until = None,
                // The daemon's engine answers queries and reports the rest as events.
                Effect::Bell
                | Effect::WriteBack(_)
                | Effect::Title(_)
                | Effect::Pwd(_)
                | Effect::Clipboard(_)
                | Effect::Notification { .. }
                | Effect::Progress(_) => {}
            }
        }
    }

    fn apply_colors(&mut self) {
        if let (Some(engine), Some((foreground, background))) = (&mut self.engine, self.colors)
            && let Err(error) = engine.set_default_colors(foreground, background)
        {
            log::warn!("couldn't set the terminal's colours: {error}");
        }
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Colors {
                foreground,
                background,
            } => {
                self.colors = Some((foreground, background));
                self.apply_colors();
                self.dirty = true;
            }
            Command::OptionAsAlt(option) => self.option_as_alt = option,
            Command::Paste { handle, text } => {
                // Answered once the daemon wrote it: what's typed next comes after it.
                if let Err(error) = self.connection.request(RequestBody::Paste { handle, text }) {
                    log::warn!("couldn't paste: {error}");
                }
            }
            Command::Copy(reply) => {
                let text = self
                    .engine
                    .as_ref()
                    .and_then(|engine| engine.selection_text().ok().flatten());
                let _ = reply.send(text);
            }
            other => {
                let Some(engine) = &mut self.engine else {
                    return;
                };
                let bytes = match drive(engine, other, self.option_as_alt) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        log::warn!("couldn't encode input: {error}");
                        Vec::new()
                    }
                };
                self.dirty = true;
                if !bytes.is_empty()
                    && let Err(error) = self.connection.input(self.index, &bytes)
                {
                    log::warn!("couldn't send input: {error}");
                }
            }
        }
    }
}

/// Applies a view command to the engine; the bytes it sends the program, if any.
fn drive(
    engine: &mut Engine,
    command: Command,
    option_as_alt: OptionAsAlt,
) -> koyo_term_engine::Result<Vec<u8>> {
    let bytes = match command {
        Command::Key(key) => {
            let bytes = engine.encode_key(&key.as_input(), option_as_alt)?;
            if !bytes.is_empty() && key.action != KeyAction::Release {
                // Typing shows where the typing goes.
                engine.scroll(Scroll::Bottom);
                engine.clear_selection()?;
            }
            bytes
        }
        Command::Text(text) => {
            engine.scroll(Scroll::Bottom);
            engine.clear_selection()?;
            text.into_bytes()
        }
        Command::Mouse(input, geometry) => engine.encode_mouse(&input, geometry)?,
        Command::Wheel { rows, at, geometry } => wheel(engine, rows, at, geometry)?,
        Command::Scroll(by) => {
            engine.scroll(by);
            Vec::new()
        }
        Command::Select {
            anchor,
            head,
            unit,
            rectangle,
        } => {
            engine.select(anchor, head, unit, rectangle)?;
            Vec::new()
        }
        Command::ClearSelection => {
            engine.clear_selection()?;
            Vec::new()
        }
        Command::Focus(focused) => engine.encode_focus(focused)?,
        Command::Colors { .. }
        | Command::OptionAsAlt(_)
        | Command::Copy(_)
        | Command::Paste { .. } => Vec::new(),
    };
    Ok(bytes)
}

/// The wheel: reported to a program that tracks the mouse, arrow keys to a full-screen one
/// that doesn't, and otherwise a scroll through the history.
fn wheel(
    engine: &mut Engine,
    rows: i32,
    at: MouseInput,
    geometry: Geometry,
) -> koyo_term_engine::Result<Vec<u8>> {
    use koyo_term_engine::{MouseAction, MouseButton};
    if rows == 0 {
        return Ok(Vec::new());
    }
    if engine.is_mouse_tracking() {
        let button = if rows < 0 {
            MouseButton::WheelUp
        } else {
            MouseButton::WheelDown
        };
        let mut out = Vec::new();
        for _ in 0..rows.unsigned_abs().min(10) {
            let press = MouseInput {
                action: MouseAction::Press,
                button: Some(button),
                ..at
            };
            out.extend(engine.encode_mouse(&press, geometry)?);
        }
        return Ok(out);
    }
    if engine.wheel_sends_arrows() {
        let arrow: &[u8] = match (rows < 0, engine.application_cursor_keys()) {
            (true, true) => b"\x1bOA",
            (true, false) => b"\x1b[A",
            (false, true) => b"\x1bOB",
            (false, false) => b"\x1b[B",
        };
        return Ok(arrow.repeat(rows.unsigned_abs().min(30) as usize));
    }
    engine.scroll(Scroll::Rows(rows));
    Ok(Vec::new())
}
