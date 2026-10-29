//! One scenario, run in this process: the editor set up, the input repeated, each event and
//! the frame after it timed.

use std::time::Duration;

use anyhow::Result;
use gpui_kit::component::input::{EditorState, Position};
use gpui_kit::{
    App, Entity, Keystroke, Modifiers, PlatformInput, ScrollDelta, ScrollEvent, ScrollSource,
    SharedString, TouchPhase, Window, point, px,
};

use crate::harness::{Bench, HEIGHT, Setup, WIDTH};
use crate::scan;
use crate::scenarios::{Kind, Scenario};
use crate::stats::{Deadline, Measured};

/// Fresh editors opened per file; the first is cold, the rest find fonts and glyphs cached.
const OPENS: usize = 5;
const SCROLL_FRAMES: usize = 300;
const KEYSTROKES: usize = 200;
/// Widths stepped through, narrower and back, each one a whole-document re-wrap.
const RESIZE_STEPS: usize = 60;
/// What the query is typed as, then erased: a call the Rust fixtures make often.
const QUERY: &str = "cx.notify()";
/// The query left active while the file is edited.
const EDIT_QUERY: &str = "self";

pub fn run(scenario: Scenario, deadline: Deadline) -> Result<Measured> {
    let fixture = scenario.fixture;
    let edit: fn(&mut Run, SharedString) -> Result<()> = match scenario.kind {
        Kind::Open => |run, text| run.open(&text),
        Kind::Scroll if fixture.single_line => |run, text| run.scroll(text, true),
        Kind::Scroll => |run, text| run.scroll(text, false),
        Kind::Type | Kind::TypeWrap => Run::typing,
        Kind::Resize => Run::resize,
        Kind::SearchQuery => Run::search_query,
        Kind::SearchEdit => Run::search_edit,
        // The scans run no editor and open no window.
        Kind::Scan | Kind::ScanTouch | Kind::BurstIgnored | Kind::BurstTracked => {
            return scan::run(scenario, deadline);
        }
    };
    let text = SharedString::from(fixture.read()?);
    let mut bench = Bench::open(Setup {
        language: fixture.language,
        soft_wrap: scenario.kind.soft_wrap(),
    })?;
    let measured = Measured {
        scale_factor: bench.scale_factor()?,
        ..Measured::default()
    };
    let mut run = Run {
        bench,
        deadline,
        measured,
    };
    edit(&mut run, text)?;
    Ok(run.measured)
}

/// A scenario under way: its window, when it has to stop, and what it has measured.
struct Run {
    bench: Bench,
    deadline: Deadline,
    measured: Measured,
}

impl Run {
    /// Takes `count` samples of `frame(bench, i)`, stopping early when the time runs out.
    fn sample(
        &mut self,
        count: usize,
        mut frame: impl FnMut(&mut Bench, usize) -> Result<Duration>,
    ) -> Result<()> {
        for i in 0..count {
            if let Some(at) = self.deadline.passed() {
                self.measured.timed_out_at_s = Some(at);
                break;
            }
            let took = frame(&mut self.bench, i)?;
            self.measured.push(took);
        }
        Ok(())
    }

    fn note(&mut self, note: String) {
        self.measured.notes.push(note);
    }

    fn open(&mut self, text: &SharedString) -> Result<()> {
        self.sample(OPENS, |bench, _| {
            bench.new_editor()?;
            let editor = bench.editor();
            let text = text.clone();
            bench.frame(|window, cx| {
                editor.update(cx, |state, cx| state.set_value(text, window, cx));
            })
        })
    }

    fn scroll(&mut self, text: SharedString, sideways: bool) -> Result<()> {
        self.bench.load(text)?;
        let line_height = self
            .bench
            .read_editor(|state, _| state.line_height())
            .unwrap_or(px(20.));
        // Half a page in wheel lines, which the editor turns back into pixels by its line height.
        let half_page = if sideways { WIDTH / 2. } else { HEIGHT / 2. };
        let lines = (half_page / line_height.as_f32()).floor().max(1.);
        let mut direction = -1.;
        let mut travelled = 0.;
        self.sample(SCROLL_FRAMES, |bench, _| {
            let before = bench.read_editor(|state, _| state.scroll_offset());
            let delta = if sideways {
                point(direction * lines, 0.)
            } else {
                point(0., direction * lines)
            };
            let took = bench.frame(|window, cx| wheel(delta, window, cx))?;
            let after = bench.read_editor(|state, _| state.scroll_offset());
            let moved = (after.x - before.x).abs().as_f32() + (after.y - before.y).abs().as_f32();
            travelled += moved;
            if moved == 0. {
                // At an end: come back the other way.
                direction = -direction;
            }
            Ok(took)
        })?;
        self.note(if travelled == 0. {
            "the wheel events never moved the view".to_owned()
        } else {
            format!("scrolled {travelled:.0} px in all")
        });
        Ok(())
    }

    /// Types at the cursor, a letter then Backspace over and over, one keystroke a sample:
    /// the line never grows past one letter, so the view stays put and every sample is the
    /// same plain edit. Checks after each keystroke, untimed, that the text took it.
    fn type_keys(&mut self) -> Result<()> {
        // Built before sampling, so the timing holds no allocation of the bench's own.
        let mut keys: Vec<Keystroke> = (0..KEYSTROKES)
            .map(|i| {
                if i % 2 == 0 {
                    keystroke(letter(i / 2), true)
                } else {
                    keystroke("backspace", false)
                }
            })
            .collect();
        let base = self.bench.read_editor(|state, _| state.text().len());
        let mut missed = 0usize;
        self.sample(KEYSTROKES, |bench, i| {
            let key = std::mem::take(&mut keys[i]);
            let took = bench.frame(|window, cx| {
                window.dispatch_keystroke(key, cx);
            })?;
            let expected = if i % 2 == 0 { base + 1 } else { base };
            if bench.read_editor(|state, _| state.text().len()) != expected {
                missed += 1;
            }
            Ok(took)
        })?;
        if missed > 0 {
            self.note(format!(
                "{missed} keystrokes didn't change the text as typed"
            ));
        }
        Ok(())
    }

    fn typing(&mut self, text: SharedString) -> Result<()> {
        let position = middle(&text);
        self.bench.load(text)?;
        self.bench.place_cursor(position)?;
        self.type_keys()
    }

    fn resize(&mut self, text: SharedString) -> Result<()> {
        self.bench.load(text)?;
        // Down from the full width to half of it in even steps, and back up.
        let half = RESIZE_STEPS / 2;
        let step = WIDTH / 2. / half as f32;
        self.sample(RESIZE_STEPS, |bench, i| {
            let narrowed = if i < half {
                i + 1
            } else {
                RESIZE_STEPS - i - 1
            };
            bench.set_width(px(WIDTH - step * narrowed as f32))
        })
    }

    /// Opens the search panel the way its shortcut does, and lets it draw and take focus.
    fn open_search(&mut self) -> Result<()> {
        let editor = self.bench.editor();
        self.bench.update_and_settle(|_, cx| {
            editor.update(cx, |state, cx| state.open_search(false, cx));
        })
    }

    /// Types two letters into the open panel's field, a frame after each, and notes it if
    /// they didn't make a two-letter query: the field is re-selected when it's drawn, so each
    /// letter replaces the last. Then clears the query.
    fn check_panel_typing(&mut self) -> Result<()> {
        for letter in ["s", "e"] {
            self.bench
                .update(|window, cx| window.dispatch_keystroke(keystroke(letter, true), cx))?;
            self.bench.draw()?;
        }
        let query = self
            .bench
            .read_editor(|state, _| state.search_session().query.clone());
        if query != "se" {
            self.note(format!(
                "typing \"se\" into the search panel left the query {query:?}: each keystroke \
                 replaced it, so the query is set as the panel sets it instead"
            ));
        }
        let editor = self.bench.editor();
        self.bench.update(|_, cx| set_query(&editor, "", cx))?;
        self.bench.pause()
    }

    fn search_query(&mut self, text: SharedString) -> Result<()> {
        self.bench.load(text)?;
        self.open_search()?;
        self.check_panel_typing()?;
        // The query typed a letter at a time, then erased, over and over: each change
        // searches the whole file again.
        let steps: Vec<&str> = (1..=QUERY.len())
            .chain((0..QUERY.len()).rev())
            .map(|length| &QUERY[..length])
            .collect();
        let editor = self.bench.editor();
        self.sample(KEYSTROKES, |bench, i| {
            let query = steps[i % steps.len()];
            bench.frame(|_, cx| set_query(&editor, query, cx))
        })?;
        self.bench.update(|_, cx| set_query(&editor, QUERY, cx))?;
        let matches = self
            .bench
            .read_editor(|state, _| state.search_session().matcher.len());
        self.note(format!("query {QUERY:?}: {matches} matches"));
        Ok(())
    }

    fn search_edit(&mut self, text: SharedString) -> Result<()> {
        let position = middle(&text);
        self.bench.load(text)?;
        self.open_search()?;
        let editor = self.bench.editor();
        self.bench
            .update_and_settle(|_, cx| set_query(&editor, EDIT_QUERY, cx))?;
        let matches = self
            .bench
            .read_editor(|state, _| state.search_session().matcher.len());
        self.bench.place_cursor(position)?;
        self.type_keys()?;
        let (query, active) = self.bench.read_editor(|state, _| {
            let session = state.search_session();
            (session.query.clone(), session.is_active())
        });
        self.note(format!(
            "query {query:?}: {matches} matches, still active: {active}"
        ));
        Ok(())
    }
}

fn wheel(delta: gpui_kit::Point<f32>, window: &mut Window, cx: &mut App) {
    let event = ScrollEvent {
        position: point(px(WIDTH / 2.), px(HEIGHT / 2.)),
        delta: ScrollDelta::Lines(delta),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    };
    window.dispatch_event(PlatformInput::Scroll(event), cx);
}

/// A key as the platform delivers it: the key, and the text it types if it types any.
fn keystroke(key: &str, types: bool) -> Keystroke {
    Keystroke {
        modifiers: Modifiers::default(),
        key: key.to_owned(),
        key_char: types.then(|| key.to_owned()),
    }
}

fn letter(i: usize) -> &'static str {
    const LETTERS: [&str; 26] = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r",
        "s", "t", "u", "v", "w", "x", "y", "z",
    ];
    LETTERS[i % LETTERS.len()]
}

/// Sets the query the way the panel's field does on every change to it.
fn set_query(editor: &Entity<EditorState>, query: &str, cx: &mut App) {
    editor.update(cx, |state, cx| {
        let case_insensitive = state.search_session().case_insensitive;
        state.set_search_query(query, case_insensitive, cx);
    });
}

/// The end of the middle line, or the middle of the only one.
fn middle(text: &str) -> Position {
    let count = text.lines().count();
    if count <= 1 {
        let half = text.encode_utf16().count() / 2;
        return Position::new(0, half as u32);
    }
    let line = count / 2;
    let width = text
        .lines()
        .nth(line)
        .map_or(0, |line| line.encode_utf16().count());
    Position::new(line as u32, width as u32)
}
