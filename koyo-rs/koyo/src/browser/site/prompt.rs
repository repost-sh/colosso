//! A page asking for a permission (`docs/browser.md` 12.4): the question comes out of the site
//! controls' button, as in Arc, with "Not Now" and "Allow". Put away any other way, by a press
//! elsewhere or Escape, it's "Not Now": nothing is kept, and the page may ask again.

use gpui_kit::assets::IconName;
use gpui_kit::{
    App, Context, Div, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Render, SharedString, Size, Stateful, StatefulInteractiveElement as _, Styled as _,
    WeakEntity, Window, div, px, size,
};
use koyo_browser_ipc::{Answer, Permission};

use super::panel::{Tones, shown};
use crate::browser::view::BrowserView;
use crate::menu::MenuBody;
use crate::shell::icon;
use crate::theme;

/// How wide its content is, the room round it, its icon's round, between the round and the
/// words, and its buttons' height.
const WIDTH: f32 = 252.;
const PAD: f32 = 8.;
const ROUND: f32 = 32.;
const GAP: f32 = 10.;
const BUTTON: f32 = 34.;
/// Its words: their size, and a line's height. Not bold, as Arc's aren't.
const WORDS: f32 = 15.;
const LINE: f32 = 20.;
const MOST_LINES: usize = 4;

pub(in crate::browser) struct Prompt {
    page: WeakEntity<BrowserView>,
    /// The question it puts, as the host numbers it.
    id: u64,
    mark: IconName,
    says: SharedString,
    answered: bool,
}

impl Prompt {
    pub(in crate::browser) fn new(
        page: WeakEntity<BrowserView>,
        id: u64,
        origin: &str,
        permissions: &[Permission],
        cx: &mut Context<Self>,
    ) -> Self {
        // Put away without an answer: not now.
        cx.on_release(|prompt: &mut Self, cx| {
            if !prompt.answered {
                let id = prompt.id;
                let _ = prompt.page.update(cx, |page, cx| page.answered(id, Answer::Dismiss, cx));
            }
        })
        .detach();
        let mark = permissions.first().map_or(IconName::Settings2, |first| shown(*first).0);
        Prompt {
            page,
            id,
            mark,
            says: asks(origin, permissions).into(),
            answered: false,
        }
    }

    fn answer(&mut self, allow: bool, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.answered, true) {
            return;
        }
        let answer = if allow { Answer::Accept } else { Answer::Dismiss };
        let id = self.id;
        let _ = self.page.update(cx, |page, cx| page.answered(id, answer, cx));
    }

    fn button(&self, allow: bool, fill: Hsla, cx: &mut Context<Self>) -> Stateful<Div> {
        let (id, says) = if allow {
            ("site-prompt-allow", "Allow")
        } else {
            ("site-prompt-not-now", "Not Now")
        };
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .h(px(BUTTON))
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(9.))
            .bg(fill)
            .cursor_pointer()
            .hover(move |button| button.bg(fill.opacity(fill.a * 1.4)))
            .text_size(px(13.))
            .font_weight(gpui_kit::FontWeight::MEDIUM)
            .child(says)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.answer(allow, cx);
                crate::popup::close_from_inside(window, cx);
            }))
    }
}

/// The words' width.
const fn words_width() -> f32 {
    WIDTH - 2. * PAD - ROUND - GAP
}

/// What the page asks, as a question: "Allow example.com to access your camera, microphone?"
fn asks(origin: &str, permissions: &[Permission]) -> String {
    let who = origin.split_once("://").map_or(origin, |(_, host)| host);
    let mut things: Vec<&str> = Vec::new();
    let mut does: Vec<&str> = Vec::new();
    for permission in permissions {
        match permission {
            Permission::Camera => things.push("camera"),
            Permission::Microphone => things.push("microphone"),
            Permission::Screen => things.push("screen"),
            Permission::Location => things.push("location"),
            Permission::Clipboard => things.push("clipboard"),
            Permission::Midi => things.push("MIDI devices"),
            Permission::Sensors => things.push("motion and light sensors"),
            Permission::Fonts => things.push("fonts"),
            Permission::LocalNetwork => things.push("local network"),
            Permission::Files => things.push("files and folders"),
            Permission::Notifications => does.push("send you notifications"),
            Permission::Windows => does.push("place windows on all your displays"),
            Permission::Downloads => does.push("download several files"),
            Permission::Other => does.push("use another capability of your computer"),
        }
    }
    let access = (!things.is_empty()).then(|| format!("access your {}", things.join(", ")));
    let all: Vec<String> = (access.into_iter())
        .chain(does.into_iter().map(str::to_owned))
        .collect();
    format!("Allow {who} to {}?", all.join(", "))
}

impl MenuBody for Prompt {
    /// Return is Allow, the answer the popover leads with; it then closes, as at any Return.
    fn hears(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if key == "enter" {
            self.answer(true, cx);
        }
        false
    }

    fn extent(&self, window: &Window, _: &App) -> Size<Pixels> {
        let style = gpui_kit::TextStyle::default();
        // A point less than they're given, so a line that only just fits is counted as it wraps.
        let width = px(words_width() - 1.);
        let lines = crate::wrapped::wrapped_lines(&self.says, &style, px(WORDS), width, window);
        let words = lines.clamp(1, MOST_LINES) as f32 * LINE;
        size(px(WIDTH), px(PAD + words.max(ROUND) + 12. + BUTTON + PAD))
    }

    fn backdrop(&self, cx: &App) -> Option<Hsla> {
        Some(Tones::of(theme::palette(cx)).glass)
    }

    fn callout(&self) -> bool {
        true
    }
}

impl Render for Prompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Tones::of(theme::palette(cx));
        let round = div()
            .size(px(ROUND))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(t.off)
            .child(icon(self.mark, 16.).text_color(t.text));
        let words = div()
            .w(px(words_width()))
            .text_size(px(WORDS))
            .line_height(px(LINE))
            .line_clamp(MOST_LINES)
            .child(self.says.clone());
        div()
            .debug_selector(|| "site-prompt".to_owned())
            .size_full()
            .p(px(PAD))
            .flex()
            .flex_col()
            .gap(px(12.))
            .text_color(t.text)
            .child(div().flex().gap(px(GAP)).child(round).child(words))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(self.button(false, t.hover, cx))
                    .child(self.button(true, t.on.opacity(0.3), cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_reads_as_a_question_about_the_site() {
        assert_eq!(
            asks("https://www.experte.com", &[Permission::Camera, Permission::Microphone]),
            "Allow www.experte.com to access your camera, microphone?"
        );
        assert_eq!(
            asks("http://localhost:3000", &[Permission::Notifications]),
            "Allow localhost:3000 to send you notifications?"
        );
        assert_eq!(
            asks("https://a.example", &[Permission::Location, Permission::Downloads]),
            "Allow a.example to access your location, download several files?"
        );
    }
}
