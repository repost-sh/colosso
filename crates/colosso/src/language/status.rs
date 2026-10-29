//! The line under a file its language's server knows, saying what the server is doing: a
//! view of its own, so a server reporting progress redraws only the line. When the server
//! failed, crashed or was stopped, clicking the line starts it again.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, Entity, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, px,
};

use super::Languages;
use super::server::{Server, State};
use crate::theme::{self, Palette};

/// How tall the line is.
pub const HEIGHT: f32 = 22.;

pub struct StatusLine {
    server: Entity<Server>,
    /// What it said when last drawn, so a report that changes nothing shown redraws nothing.
    said: (SharedString, bool),
    _server: Subscription,
}

impl StatusLine {
    pub fn new(server: Entity<Server>, cx: &mut Context<Self>) -> Self {
        let _server = cx.observe(&server, |this, _, cx| {
            let said = this.say(cx);
            if said != this.said {
                this.said = said;
                cx.notify();
            }
        });
        let mut line = StatusLine {
            server,
            said: (SharedString::default(), false),
            _server,
        };
        line.said = line.say(cx);
        line
    }

    /// What the line says, and whether clicking it starts the server again.
    fn say(&self, cx: &gpui_kit::App) -> (SharedString, bool) {
        let server = self.server.read(cx);
        let name = server.kind().name();
        let text = match server.state() {
            State::Stopped => format!("{name} stopped · click to start it"),
            State::Starting => format!("{name} starting…"),
            State::Indexing { done, total, title } if *total > 0 => {
                format!("{name} · {title} {done}/{total}")
            }
            State::Indexing { title, .. } => format!("{name} · {title}…"),
            State::Ready => match server.health() {
                Some(message) if !message.is_empty() => format!("{name} · {message}"),
                _ => format!("{name} · ready"),
            },
            State::Failed(why) => format!("{name} couldn't start: {why} · click to retry"),
            State::Crashed(why) if server.is_restarting() => {
                format!("{name} crashed, restarting: {why}")
            }
            State::Crashed(why) => format!("{name} crashed: {why} · click to retry"),
        };
        let retries = matches!(server.state(), State::Stopped | State::Failed(_))
            || matches!(server.state(), State::Crashed(_)) && !server.is_restarting();
        (text.into(), retries)
    }

    fn colour(&self, p: &Palette, cx: &gpui_kit::App) -> Hsla {
        match self.server.read(cx).state() {
            State::Ready => p.added,
            State::Failed(_) | State::Crashed(_) => p.danger,
            State::Stopped => p.muted,
            State::Starting | State::Indexing { .. } => p.modified,
        }
    }
}

impl Render for StatusLine {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let (text, retries) = self.said.clone();
        let dot = self.colour(&p, cx);
        let server = self.server.clone();
        div()
            .id("language-status")
            .h(px(HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .gap_1p5()
            .px_2()
            .border_t_1()
            .border_color(p.hairline)
            .text_xs()
            .text_color(p.muted)
            .overflow_hidden()
            .child(div().size(px(6.)).flex_none().rounded_full().bg(dot))
            .child(div().truncate().child(text))
            .when(retries, |line| {
                line.cursor_pointer()
                    .hover(|line| line.text_color(p.text))
                    .on_click(move |_, _, cx| {
                        let Some(Languages(registry)) = cx.try_global::<Languages>().cloned()
                        else {
                            return;
                        };
                        registry.update(cx, |registry, cx| registry.retry(&server, cx));
                    })
            })
    }
}
