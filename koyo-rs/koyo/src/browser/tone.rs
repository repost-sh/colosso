//! How light a page is, for the glass over it (`lightness.rs`), and what its canvas is painted,
//! for what's behind it (`page.rs`). A page is drawn by Chromium in its own layers, which the
//! app can't read, so the page says it: the background colour at a grid of its points, as it
//! first shows and whenever something that could change it happens, only when it changed, and
//! never while it's hidden (the host's `tone.rs`). The app asks nothing and wakes for nothing.
//! That's what glass takes its tone from, blurred as it is; pictures and gradients read as
//! what's behind them.

use gpui_kit::Context;

use super::view::BrowserView;

/// How far a point's lightness moves before glass is drawn again for it.
const NOTICEABLE: f32 = 0.04;

/// How light a page is at each point of its grid, and the colour of its canvas, as it said.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Tone(koyo_browser_ipc::Tone);

impl Tone {
    /// The colour the page's canvas is painted, which shows wherever nothing of the page does.
    pub(super) fn canvas(&self) -> gpui_kit::Hsla {
        let [red, green, blue] = self.0.canvas.map(u32::from);
        gpui_kit::rgb(red << 16 | green << 8 | blue).into()
    }

    /// The mean lightness within `[left, top, right, bottom]`, each a share of the page's width
    /// or height; none where that's none of the page.
    pub(super) fn lightness(&self, [left, top, right, bottom]: [f32; 4]) -> Option<f32> {
        let (across, down) = (koyo_browser_ipc::Tone::ACROSS, koyo_browser_ipc::Tone::DOWN);
        let cell = |share: f32, cells: usize| ((share.clamp(0., 1.) * cells as f32) as usize).min(cells - 1);
        if right <= left || bottom <= top {
            return None;
        }
        let (columns, rows) = (cell(left, across)..=cell(right, across), cell(top, down)..=cell(bottom, down));
        let cells: Vec<f32> = rows
            .flat_map(|row| columns.clone().map(move |column| row * across + column))
            .filter_map(|at| self.0.lightness.get(at).copied())
            .collect();
        (!cells.is_empty()).then(|| cells.iter().sum::<f32>() / cells.len() as f32)
    }

    /// Whether `other` differs enough for glass to look different over it.
    fn differs(&self, other: &Tone) -> bool {
        self.0.canvas != other.0.canvas
            || self.0.lightness.len() != other.0.lightness.len()
            || (self.0.lightness.iter().zip(&other.0.lightness)).any(|(a, b)| (a - b).abs() > NOTICEABLE)
    }
}

impl BrowserView {
    /// The page said its tone: kept, and the view drawn again, only if glass over the page
    /// would look different.
    pub(super) fn toned(&mut self, tone: koyo_browser_ipc::Tone, cx: &mut Context<Self>) {
        let tone = Tone(tone);
        if (self.tone.as_ref()).is_none_or(|was| was.differs(&tone)) {
            log::debug!("page {} is {:?} light at its foot", self.pane, tone.lightness([0., 0.9, 1., 1.]));
            self.tone = Some(tone);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use gpui_kit::{AppContext as _, TestAppContext, px, size};
    use koyo_browser_ipc::{AppRequest, HostEvent, Tone as Said};

    use super::Tone;
    use crate::browser::BrowserView;
    use crate::browser::host::fake::FakeHost;
    use crate::budget::{Built, built_after, ratchet, within};
    use crate::test_support::{draw, in_window, open_page_view, shell_on};

    fn said(of: impl Fn(usize, usize) -> f32, canvas: [u8; 3]) -> Said {
        let cells = (0..Said::DOWN).flat_map(|row| (0..Said::ACROSS).map(move |column| (column, row)));
        Said { lightness: cells.map(|(column, row)| of(column, row)).collect(), canvas }
    }

    fn tone(of: impl Fn(usize, usize) -> f32) -> Tone {
        Tone(said(of, [255; 3]))
    }

    #[test]
    fn a_region_is_as_light_as_the_page_is_there() {
        let page = tone(|_, row| if row == Said::DOWN - 1 { 0. } else { 1. });
        assert!(page.lightness([0., 0.9, 1., 1.]).unwrap() < 0.01);
        assert!(page.lightness([0., 0., 1., 0.5]).unwrap() > 0.99);
        assert_eq!(page.lightness([0.5, 0.5, 0.5, 0.6]), None);
    }

    #[test]
    fn the_canvas_is_the_colour_the_page_names() {
        let dark = Tone(said(|_, _| 1., [18, 18, 18]));
        assert_eq!(dark.canvas(), gpui_kit::Hsla::from(gpui_kit::rgb(0x0012_1212)));
        assert_eq!(tone(|_, _| 1.).canvas(), gpui_kit::Hsla::from(gpui_kit::rgb(0x00ff_ffff)));
        assert!(dark.differs(&tone(|_, _| 1.)));
    }

    #[test]
    fn a_small_change_isnt_one() {
        let (white, nearly, dark) = (tone(|_, _| 1.), tone(|_, _| 0.98), tone(|_, _| 0.2));
        assert!(!white.differs(&nearly));
        assert!(white.differs(&dark));
    }

    /// A page that turns dark from light, as it says when it changes, builds its view again,
    /// whose glass takes its tone from the page.
    #[gpui_kit::test]
    fn a_page_turning_dark_builds_its_view_again(cx: &mut TestAppContext) {
        let (window, shell, _) = shell_on("budget-tone", cx);
        let view = open_page_view(window, &shell, "https://example.com/", cx);
        let toned = |lightness: f32| {
            let view = view.clone();
            move |cx: &mut TestAppContext| {
                view.update(cx, |view, cx| view.toned(said(|_, _| lightness, [255; 3]), cx));
            }
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

    /// A page drawn and left alone costs the engine nothing for its tone: whatever the app
    /// asks it, answered each time, it asks once at most, however long the page is shown; the
    /// tone the host says is the page's.
    #[gpui_kit::test]
    fn a_drawn_page_left_alone_is_asked_once_at_most(cx: &mut TestAppContext) {
        cx.update(crate::test_support::init);
        let host = FakeHost::serve("tone-asked-once", cx);
        let (window, view) = in_window(size(px(800.), px(600.)), cx, |window, cx| {
            cx.new(|cx| BrowserView::new("https://a.example/".into(), Path::new("/work/app"), window, cx))
        });
        draw(window, cx);
        assert!(view.read_with(cx, |view, _| view.drawn()), "the page is drawn");
        let pane = view.read_with(cx, |view, _| view.pane);
        host.says(HostEvent::Tone { pane, tone: said(|_, _| 0., [0; 3]) }, cx);
        let white = format!("{};255 255 255", vec!["1"; Said::ACROSS * Said::DOWN].join(","));
        let mut asked = Vec::new();
        for _ in 0..10 {
            for request in host.asked(cx) {
                if let AppRequest::Cdp { pane, call, method, .. } = request {
                    let result = Some(serde_json::json!({ "result": { "value": white } }));
                    host.says(HostEvent::CdpReply { pane, call, result, error: None }, cx);
                    asked.push(method);
                }
            }
            cx.executor().advance_clock(Duration::from_secs(1));
        }
        assert!(asked.len() <= 1, "ten seconds of a page shown, asked {asked:?}");
        let dark = view.read_with(cx, |view, _| view.tone.as_ref().and_then(|tone| tone.lightness([0., 0., 1., 1.])));
        assert_eq!(dark, Some(0.), "the page is as dark as the host said");
    }
}
