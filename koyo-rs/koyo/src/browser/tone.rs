//! How light a page is, for the glass over it (`lightness.rs`), and what its canvas is painted,
//! for what's behind it (`page.rs`). A page is drawn by Chromium in its own layers, which the
//! app can't read, so the page is asked: once a second while it's in sight, the background colour
//! at a grid of its points, through its `DevTools` session. Out of sight it's asked nothing and
//! nothing wakes for it; seen again, it's asked at once. That's what glass takes its tone from,
//! blurred as it is; pictures and gradients read as what's behind them.

use std::time::Duration;

use gpui_kit::Context;
use serde_json::json;

use super::view::BrowserView;

/// How many points across and down the page are asked about.
const ACROSS: usize = 12;
const DOWN: usize = 8;
/// How often a page in sight is asked.
const EVERY: Duration = Duration::from_secs(1);
/// How far a point's lightness moves before glass is drawn again for it.
const NOTICEABLE: f32 = 0.04;

/// The relative luminance of the opaque background at each point of an `ACROSS` by `DOWN` grid
/// over the page's viewport, row by row, as a comma-separated list. Where no element has one
/// it's the body's, which paints the whole canvas when the root has none, else white. Then,
/// after a semicolon, the canvas's colour itself (the root's, else the body's, else white) as
/// red, green and blue.
const ASK: &str = "(() => { const C = 12, R = 8, w = innerWidth, h = innerHeight, out = []; \
const lum = (c) => { if (!c.startsWith('rgb')) return null; const m = c.match(/[\\d.]+/g); \
if (!m || m.length < 3 || (m.length > 3 && +m[3] < 0.5)) return null; \
const f = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); }; \
return 0.2126 * f(+m[0]) + 0.7152 * f(+m[1]) + 0.0722 * f(+m[2]); }; \
for (let r = 0; r < R; r++) for (let c = 0; c < C; c++) { let v = null; \
for (const el of document.elementsFromPoint((c + 0.5) * w / C, (r + 0.5) * h / R)) { \
v = lum(getComputedStyle(el).backgroundColor); if (v !== null) break; } \
if (v === null && document.body) v = lum(getComputedStyle(document.body).backgroundColor); \
out.push(v === null ? 1 : Math.round(v * 1000) / 1000); } \
const bg = (el) => { const c = el ? getComputedStyle(el).backgroundColor : ''; \
const m = c.startsWith('rgb') ? c.match(/[\\d.]+/g) : null; \
return m && m.length >= 3 && !(m.length > 3 && +m[3] < 0.5) ? m.slice(0, 3).map((v) => Math.round(+v)).join(' ') : null; }; \
return out.join(',') + ';' + (bg(document.documentElement) || bg(document.body) || '255 255 255'); })()";

/// How light a page is at each point of the grid, and the colour of its canvas.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Tone(Vec<f32>, [u8; 3]);

impl Tone {
    /// What the page answered [`ASK`] with.
    fn parse(answer: &str) -> Option<Tone> {
        let (grid, canvas) = answer.split_once(';').unwrap_or((answer, ""));
        let cells: Vec<f32> = grid.split(',').filter_map(|cell| cell.parse().ok()).collect();
        let parts: Vec<u8> = canvas.split(' ').filter_map(|part| part.parse().ok()).collect();
        let canvas = <[u8; 3]>::try_from(parts).unwrap_or([255; 3]);
        (cells.len() == ACROSS * DOWN).then_some(Tone(cells, canvas))
    }

    /// The colour the page's canvas is painted, which shows wherever nothing of the page does.
    pub(super) fn canvas(&self) -> gpui_kit::Hsla {
        let [red, green, blue] = self.1.map(u32::from);
        gpui_kit::rgb(red << 16 | green << 8 | blue).into()
    }

    /// The mean lightness within `[left, top, right, bottom]`, each a share of the page's width
    /// or height; none where that's none of the page.
    pub(super) fn lightness(&self, [left, top, right, bottom]: [f32; 4]) -> Option<f32> {
        let cell = |share: f32, cells: usize| ((share.clamp(0., 1.) * cells as f32) as usize).min(cells - 1);
        if right <= left || bottom <= top {
            return None;
        }
        let (columns, rows) = (cell(left, ACROSS)..=cell(right, ACROSS), cell(top, DOWN)..=cell(bottom, DOWN));
        let cells: Vec<f32> = rows
            .flat_map(|row| columns.clone().map(move |column| row * ACROSS + column))
            .filter_map(|at| self.0.get(at).copied())
            .collect();
        (!cells.is_empty()).then(|| cells.iter().sum::<f32>() / cells.len() as f32)
    }

    /// Whether `other` differs enough for glass to look different over it.
    fn differs(&self, other: &Tone) -> bool {
        self.1 != other.1 || self.0.iter().zip(&other.0).any(|(a, b)| (a - b).abs() > NOTICEABLE)
    }
}

impl BrowserView {
    /// Has the page asked how light it is, now and each second after: `tell_sight` starts it
    /// while the page is drawn (`BrowserView::drawn`) and drops it once it isn't.
    pub(super) fn watch_tone(&mut self, cx: &mut Context<Self>) {
        if self.tone_watch.is_some() {
            return;
        }
        self.tone_watch = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(asked) = this.update(cx, |this, cx| {
                    #[cfg(test)]
                    {
                        this.tone_turns += 1;
                    }
                    this.is_open(cx).then(|| {
                        let pane = this.pane;
                        let params = json!({ "expression": ASK, "returnByValue": true });
                        (this.browsers).update(cx, |browsers, _| browsers.call(pane, "Runtime.evaluate", params))
                    })
                }) else {
                    return;
                };
                if let Some(answered) = asked
                    && let Ok(Ok(answer)) = answered.await
                    && let Some(tone) = (answer.pointer("/result/value"))
                        .and_then(serde_json::Value::as_str)
                        .and_then(Tone::parse)
                {
                    let kept = this.update(cx, |this, cx| this.toned(tone, cx));
                    if kept.is_err() {
                        return;
                    }
                }
                cx.background_executor().timer(EVERY).await;
            }
        }));
    }

    /// Kept, and the view drawn again, only if glass over the page would look different.
    fn toned(&mut self, tone: Tone, cx: &mut Context<Self>) {
        if (self.tone.as_ref()).is_none_or(|was| was.differs(&tone)) {
            log::debug!("page {} is {:?} light at its foot", self.pane, tone.lightness([0., 0.9, 1., 1.]));
            self.tone = Some(tone);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui_kit::{AppContext as _, Entity, TestAppContext};

    use super::{ACROSS, DOWN, Tone};
    use crate::browser::BrowserView;
    use crate::browser::view::sight::Shown;
    use crate::budget::{Built, built_after, ratchet, within};
    use crate::settings::Layout;
    use crate::test_support::{on_screen, open_page_view, shell_on};

    fn tone(of: impl Fn(usize, usize) -> f32) -> Tone {
        let cells = (0..DOWN).flat_map(|row| (0..ACROSS).map(move |column| (column, row)));
        let text: Vec<String> = cells.map(|(column, row)| of(column, row).to_string()).collect();
        Tone::parse(&text.join(",")).unwrap()
    }

    #[test]
    fn an_answer_of_another_shape_is_no_tone() {
        assert_eq!(Tone::parse("1,0.5"), None);
        assert_eq!(Tone::parse(""), None);
    }

    #[test]
    fn a_region_is_as_light_as_the_page_is_there() {
        let page = tone(|_, row| if row == DOWN - 1 { 0. } else { 1. });
        assert!(page.lightness([0., 0.9, 1., 1.]).unwrap() < 0.01);
        assert!(page.lightness([0., 0., 1., 0.5]).unwrap() > 0.99);
        assert_eq!(page.lightness([0.5, 0.5, 0.5, 0.6]), None);
    }

    #[test]
    fn the_canvas_is_the_colour_the_page_names() {
        let grid = vec!["1"; ACROSS * DOWN].join(",");
        let dark = Tone::parse(&format!("{grid};18 18 18")).unwrap();
        assert_eq!(dark.canvas(), gpui_kit::Hsla::from(gpui_kit::rgb(0x0012_1212)));
        let white = gpui_kit::Hsla::from(gpui_kit::rgb(0x00ff_ffff));
        assert_eq!(Tone::parse(&grid).unwrap().canvas(), white);
        assert_eq!(Tone::parse(&format!("{grid};red")).unwrap().canvas(), white);
        assert!(dark.differs(&Tone::parse(&grid).unwrap()));
    }

    #[test]
    fn a_small_change_isnt_one() {
        let (white, nearly, dark) = (tone(|_, _| 1.), tone(|_, _| 0.98), tone(|_, _| 0.2));
        assert!(!white.differs(&nearly));
        assert!(white.differs(&dark));
    }

    /// A page that turns dark from light, as the page says at most once a second while it's
    /// drawn, builds its view again, whose glass takes its tone from the page.
    #[gpui_kit::test]
    fn a_page_turning_dark_builds_its_view_again(cx: &mut TestAppContext) {
        let (window, shell, _) = shell_on("budget-tone", Layout::Orca, cx);
        let view = open_page_view(window, &shell, "https://example.com/", cx);
        let toned = |lightness: f32| {
            let view = view.clone();
            move |cx: &mut TestAppContext| view.update(cx, |view, cx| view.toned(tone(|_, _| lightness), cx))
        };
        built_after(window, cx, toned(1.));

        let held = Built {
            shell: 0,
            views: 2,
            frames: 1,
        };
        ratchet(built_after(window, cx, toned(0.98)), Built::default(), "a page a shade darker");
        within(built_after(window, cx, toned(0.)), held, "a page turned dark");
    }

    /// Has `view` drawn, as its page element has it while it draws it, until what's returned
    /// goes: a test's page is never drawn, as the notice that no engine runs covers it.
    fn drawn(view: &Entity<BrowserView>, cx: &mut TestAppContext) -> Entity<Shown> {
        let shown = cx.new(|cx| Shown::new(view.downgrade(), cx));
        view.update(cx, |view, cx| {
            view.shown = Some(shown.downgrade());
            view.tell_sight(cx);
        });
        shown
    }

    /// A page wakes to ask how light it is only while a person can see it: in a window nobody
    /// sees, or no longer drawn as its tab isn't shown, it keeps no wake-up of its own, and
    /// seen again it asks at once.
    #[gpui_kit::test]
    fn a_page_out_of_sight_keeps_no_wake_up_and_asks_at_once_when_seen(cx: &mut TestAppContext) {
        let (window, shell, _) = shell_on("tone-sight", Layout::Orca, cx);
        let view = open_page_view(window, &shell, "https://example.com/", cx);
        view.update(cx, BrowserView::pretend_opened);
        let shown = drawn(&view, cx);
        let turns_after = |seconds: u64, cx: &mut TestAppContext| {
            cx.executor().advance_clock(Duration::from_secs(seconds));
            cx.run_until_parked();
            view.read_with(cx, |view, _| view.tone_turns)
        };
        let seen = turns_after(0, cx);
        assert_eq!(turns_after(3, cx), seen + 3, "a turn each second while it's seen");

        on_screen(window, false, cx);
        let unseen = turns_after(0, cx);
        assert_eq!(turns_after(3, cx), unseen, "it woke in a window nobody sees");
        on_screen(window, true, cx);
        assert_eq!(turns_after(0, cx), unseen + 1, "seen again, it asks at once");

        drop(shown);
        cx.update(|_| {});
        let hidden = turns_after(0, cx);
        assert_eq!(turns_after(3, cx), hidden, "it woke no longer drawn");
        let _shown = drawn(&view, cx);
        assert_eq!(turns_after(0, cx), hidden + 1, "drawn again, it asks at once");
    }
}
