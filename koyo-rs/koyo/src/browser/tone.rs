//! How light a native page is, for the glass over it (`lightness.rs`). An off-screen page's
//! frame is in a surface the app reads; a native page is drawn by Chromium in its own layers,
//! which the app can't read, so the page is asked instead: once a second while it's drawn, the
//! background colour at a grid of its points, through its `DevTools` session. That's what glass
//! takes its tone from, blurred as it is; pictures and gradients read as what's behind them.

use std::time::Duration;

use gpui_kit::Context;
use serde_json::json;

use super::view::BrowserView;

/// How many points across and down the page are asked about.
const ACROSS: usize = 12;
const DOWN: usize = 8;
/// How often a drawn page is asked.
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
    /// Has a native page asked how light it is for as long as the view lives, while it's
    /// drawn. Started once.
    pub(super) fn watch_tone(&mut self, cx: &mut Context<Self>) {
        if self.tone_watch.is_some() || !self.is_native() {
            return;
        }
        self.tone_watch = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(asked) = this.update(cx, |this, cx| {
                    (this.shown.is_some() && this.is_open(cx)).then(|| {
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
                    let kept = this.update(cx, |this, cx| {
                        let changed = (this.tone.as_ref()).is_none_or(|was| was.differs(&tone));
                        if changed {
                            log::debug!("page {} is {:?} light at its foot", this.pane, tone.lightness([0., 0.9, 1., 1.]));
                            this.tone = Some(tone);
                            // Glass over the page is drawn by whoever lays it there.
                            cx.notify();
                            cx.refresh_windows();
                        }
                    });
                    if kept.is_err() {
                        return;
                    }
                }
                cx.background_executor().timer(EVERY).await;
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::{ACROSS, DOWN, Tone};

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
        // Dark along the foot, white above.
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
        // None named, or one that isn't a colour: white, as a page with no background is.
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
}
