//! `--visual-check-chat-markdown`: a chat's markdown and its floating prompts, drawn as the app
//! draws them on the fake provider, light and dark, in `.superpowers/qa/chat-markdown/`
//! (`docs/chat.md` 5.3): markdown in the person's bubbles and the answers, a long prompt closed and
//! opened, the composer while each construct is typed and with the caret in one, a chat of three
//! prompts at each place a scroll stops with what floats there (nothing until a scroll up, then
//! as far down as it went), the next prompt pushing a floating one up, a floating prompt opened
//! and gone to, the same places with the setting off and Settings › Chat, and a picture clicked
//! open in the viewer and closed with Esc. What each picture should show is checked before it's
//! saved. Glass is drawn as its fallback fill, as a picture leaves the native glass out, and
//! native controls, a switch among them, aren't in it either.
//!
//! ```sh
//! mkdir -p .superpowers/c/tmp &&
//!   KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$PWD/.superpowers/c TMPDIR=$PWD/.superpowers/c/tmp \
//!   KOYO_CONFIG_DIR=$PWD/.superpowers/qa/chat-markdown/config KOYO_LOG_DIR=$PWD/.superpowers/c/logs \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-chat-markdown
//! ```

use std::time::{Duration, Instant};

use gpui_kit::component::input::TextareaState;
use gpui_kit::{Entity, Modifiers, ScrollDelta, ScrollEvent, point, px};
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::model::{Prompt, Status};

use crate::chat::view::ChatView;
use crate::navigation::Route;
use crate::settings::{self, Preset};
use crate::settings_view::SettingsPage;
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result, checkout, prompt};

mod picture;

const OUT: &str = ".superpowers/qa/chat-markdown";
/// How long the host gets to answer, and a run to settle.
const PATIENCE: Duration = Duration::from_secs(30);
/// How long a text view gets to take in its text, off the main thread, before it's drawn.
const PARSED_IN: Duration = Duration::from_millis(400);

/// Every construct, in two prompts a bubble shows whole. The fake provider says each back after
/// "You said: ", so each starts with words the construct can follow.
const CONSTRUCTS: [&str; 2] = [
    "Every construct, the first half:\n## A heading\n**Bold**, *italic*, ~~struck~~ and \
     `code`, with [a link](https://koyo.dev).\n- A list item\n- [ ] A task\n1. A numbered one",
    "And the second:\n> A quote from the docs.\n```rust\nfn main() {}\n```\n| Construct | Marks \
     |\n|---|---|\n| Bold | `**` |",
];

/// What's typed in the composer for each construct, and what the picture's called.
const TYPED: [(&str, &str); 10] = [
    ("heading", "## A heading"),
    ("bold", "Make this **bold** and this __too__"),
    ("italic", "Some *italic* and _more_"),
    ("struck", "This is ~~gone~~"),
    ("code", "Run `cargo test` now"),
    ("code-block", "```rust\nfn main() {}\n```"),
    ("list", "- one\n- two\n1. three\n- [ ] four"),
    (
        "link",
        "See [the docs](https://koyo.dev) or <https://koyo.dev>",
    ),
    ("quote", "> A quote\n> on two lines"),
    ("table", "| a | b |\n|---|---|\n| 1 | 2 |"),
];

/// The three prompts of the chat that scrolls: the fake provider works for each before it answers,
/// so each heads a long section; the second is long enough to open.
const PROMPTS: [&str; 3] = [
    "Show your work: the first file.",
    "Show your work: the second, a longer prompt.\n- It makes\n- several points\n- that take\n- more \
     than\n- a few lines",
    "Show your work: the third.",
];

pub(super) fn draw() -> Result<Vec<String>> {
    crate::visual::draw_glass_as_fill();
    let mut check = Check {
        drawn: Drawn::open_on_checkout(OUT, &[])?,
        saved: Vec::new(),
    };
    check.history()?;
    check.long_prompt()?;
    check.composer()?;
    check.floating()?;
    check.picture()?;
    // The host the chats started stops with the check.
    let data = settings::data_dir().ok_or("there's no data folder")?;
    let host = koyo_agent::client::host_dir(&data);
    koyo_agent::client::stop_host(&host).map_err(|error| error.to_string())?;
    Ok(check.saved)
}

struct Check {
    drawn: Drawn,
    saved: Vec<String>,
}

impl Check {
    /// Saves what shows as `name`, light then dark.
    fn both(&mut self, name: &str) -> Result<()> {
        let themes: [(&dyn gpui_kit::Action, &str); 2] =
            [(&UseLightTheme, "light"), (&UseDarkTheme, "dark")];
        for (theme, look) in themes {
            self.drawn.cx.update(|cx| cx.dispatch_action(theme));
            self.settle_text()?;
            let path = self.drawn.save(&format!("{name}-{look}.png"))?;
            self.saved.push(format!("saved {}", path.display()));
        }
        self.drawn
            .cx
            .update(|cx| cx.dispatch_action(&UseLightTheme));
        self.drawn.draw()
    }

    /// Draws until text views have had the time to take in their text.
    fn settle_text(&mut self) -> Result<()> {
        let start = Instant::now();
        self.drawn
            .until(PATIENCE, |_| Ok(start.elapsed() >= PARSED_IN))?;
        self.redraw()
    }

    /// Draws twice: what floats is taken up once a frame's laid out, and drawn in the next.
    fn redraw(&mut self) -> Result<()> {
        self.drawn.draw()?;
        self.drawn.draw()
    }

    /// What's typed in `text` is `typed`, as if a person had typed it.
    fn set_typed(&mut self, text: &Entity<TextareaState>, typed: &str) -> Result<()> {
        let window = self.drawn.window;
        (self.drawn.cx)
            .update_window(window, |_, window, cx| {
                text.update(cx, |text, cx| text.set_value(typed, window, cx));
            })
            .map_err(|error| error.to_string())
    }

    /// A new chat saying `said`, once its run is over.
    fn chat(&mut self, said: Prompt) -> Result<ThreadId> {
        let id = self.drawn.chat_sending(&checkout()?, said)?;
        self.settled(&id)?;
        Ok(id)
    }

    /// Waits for chat `id`'s run to be over, and what it said to be drawn.
    fn settled(&mut self, id: &ThreadId) -> Result<()> {
        let chats = self
            .drawn
            .cx
            .read(|cx| self.drawn.shell.read(cx).chats().clone());
        let start = Instant::now();
        self.drawn.until(PATIENCE, |drawn| {
            let idle = drawn.cx.read(|cx| {
                let summary = chats.read(cx).summary(id);
                summary.is_some_and(|summary| summary.status == Status::Idle)
            });
            Ok(idle && start.elapsed() >= Duration::from_secs(2))
        })?;
        self.settle_text()
    }

    /// The tab of chat `id`.
    fn view(&self, id: &ThreadId) -> Result<Entity<ChatView>> {
        self.drawn
            .cx
            .read(|cx| {
                let shell = self.drawn.shell.read(cx);
                let (_, pane) = shell
                    .workspace
                    .find_pane(|pane| pane.chat.as_ref() == Some(id))?;
                shell.chat_tabs.view(pane).cloned()
            })
            .ok_or_else(|| "the chat has no tab".into())
    }

    fn history(&mut self) -> Result<()> {
        let id = self.chat(prompt(CONSTRUCTS[0]))?;
        self.drawn.send(&id, prompt(CONSTRUCTS[1]))?;
        self.settled(&id)?;
        self.both("history-markdown")
    }

    fn long_prompt(&mut self) -> Result<()> {
        let steps = (1..=16).fold(String::new(), |steps, step| {
            steps + &format!("\n- Step {step}: do what step {step} says, then check it.")
        });
        let id = self.chat(prompt(&format!("A long prompt, in steps:{steps}")))?;
        let view = self.view(&id)?;
        self.scroll_to(&view, 0)?;
        self.both("long-prompt-closed")?;
        let timeline = self
            .drawn
            .cx
            .read(|cx| view.read(cx).showing().map(|shown| shown.timeline.clone()));
        let timeline = timeline.ok_or("the chat shows nothing")?;
        self.drawn.cx.update(|cx| {
            timeline.update(cx, |timeline, cx| {
                let id = timeline.rows[0].id.clone();
                timeline.toggle(id, 0, cx);
            });
        });
        // Opened at the end, it grows up past the top: back to where it starts.
        self.scroll_to(&view, 0)?;
        self.both("long-prompt-open")
    }

    /// Each construct typed in the composer of the chat on show, then the caret in a bold span.
    fn composer(&mut self) -> Result<()> {
        let id = self.chat(prompt("The composer formats what's typed."))?;
        let view = self.view(&id)?;
        let text = self
            .drawn
            .cx
            .read(|cx| view.read(cx).composer.read(cx).text.clone());
        let window = self.drawn.window;
        for (name, typed) in TYPED {
            self.set_typed(&text, typed)?;
            let styled = self.drawn.cx.read(|cx| text.read(cx).decorations().len());
            if styled == 0 {
                return Err(format!("{name}: what's typed has no styles"));
            }
            self.both(&format!("composer-{name}"))?;
        }
        // A caret isn't drawn in a window that isn't in front, as one off screen isn't: what's
        // checked is that it's in the bold word and that the styles are the same with it there.
        let typed = "Make this **bold** now";
        let styles = |cx: &gpui_kit::App| text.read(cx).decorations().to_vec();
        self.set_typed(&text, typed)?;
        let at_end = self.drawn.cx.read(styles);
        (self.drawn.cx)
            .update_window(window, |_, window, cx| {
                text.update(cx, |text, cx| {
                    text.set_selected_range(13..13, cx);
                    text.focus(window, cx);
                });
            })
            .map_err(|error| error.to_string())?;
        self.drawn.draw()?;
        let (inside, caret) = self
            .drawn
            .cx
            .read(|cx| (styles(cx), text.read(cx).selected_range()));
        if caret != (13..13) || inside != at_end || inside.is_empty() {
            return Err(format!(
                "the caret at {caret:?} changed the styles, or there are none"
            ));
        }
        self.both("composer-caret-in-bold")?;
        self.set_typed(&text, "")
    }

    /// Goes to row `row` of `view`'s timeline, and has what floats taken up.
    fn scroll_to(&mut self, view: &Entity<ChatView>, row: usize) -> Result<()> {
        self.drawn.cx.update(|cx| {
            let scroller = view.read(cx).showing().map(|shown| shown.scroller.clone());
            if let Some(scroller) = scroller {
                scroller.update(cx, |scroller, cx| scroller.scroll_to_item(row, cx));
            }
        });
        self.redraw()
    }

    /// Checks the prompt of row `expected` floats, or none does, and that its row is left empty
    /// meanwhile, then saves it as `name`.
    fn floats(
        &mut self,
        view: &Entity<ChatView>,
        expected: Option<usize>,
        name: &str,
    ) -> Result<()> {
        self.drawn.draw()?;
        let (floating, lifted) = self.drawn.cx.read(|cx| {
            let view = view.read(cx);
            let lifted = view
                .showing()
                .and_then(|shown| shown.timeline.read(cx).floating);
            (view.floating.as_ref().map(|floating| floating.at), lifted)
        });
        if floating != expected || lifted != expected {
            return Err(format!(
                "{name}: {floating:?} floats from {lifted:?}, not {expected:?}"
            ));
        }
        self.both(name)
    }

    fn floating(&mut self) -> Result<()> {
        self.drawn.cx.update(|cx| {
            settings::update(cx, |settings| {
                settings.chat_verbosity = Preset::Verbose.verbosity();
            });
        });
        let id = self.chat(prompt(PROMPTS[0]))?;
        for said in &PROMPTS[1..] {
            self.drawn.send(&id, prompt(said))?;
            self.settled(&id)?;
        }
        let view = self.view(&id)?;
        let prompts = self.drawn.cx.read(|cx| {
            view.read(cx)
                .showing()
                .map(|shown| shown.timeline.read(cx).prompts.to_vec())
        });
        let prompts = prompts.ok_or("the chat shows nothing")?;
        let [first, second, third] = prompts[..] else {
            return Err(format!("three prompts, not {prompts:?}"));
        };
        for (on, ahead) in [(true, "float"), (false, "off")] {
            self.drawn
                .cx
                .update(|cx| settings::update(cx, |settings| settings.floating_prompts = on));
            // At the end, nothing; scrolling up brings the latest down as far, scrolling down
            // takes it back up.
            self.at_end(&view)?;
            self.floats(&view, None, &format!("{ahead}-bottom"))?;
            self.wheel(-50.)?;
            self.floats(&view, on.then_some(third), &format!("{ahead}-revealing"))?;
            self.wheel(-80.)?;
            self.floats(&view, on.then_some(third), &format!("{ahead}-revealed"))?;
            self.wheel(60.)?;
            self.floats(&view, on.then_some(third), &format!("{ahead}-hiding"))?;
            // Gone to the middle of a section, nothing, until a scroll up there.
            self.scroll_to(&view, second + 3)?;
            self.floats(&view, None, &format!("{ahead}-jumped"))?;
            self.wheel(400.)?;
            self.wheel(-300.)?;
            self.floats(&view, on.then_some(second), &format!("{ahead}-middle"))?;
            // Its place at the top, it's there; scrolling down, it goes with its place.
            self.scroll_to(&view, second)?;
            self.floats(&view, None, &format!("{ahead}-settled"))?;
            self.wheel(6.)?;
            self.floats(&view, None, &format!("{ahead}-scrolled-down"))?;
            // A turn up from the third at the top: the second comes down above it, pushed out.
            self.scroll_to(&view, third)?;
            self.wheel(-40.)?;
            self.floats(&view, on.then_some(second), &format!("{ahead}-pushed"))?;
            self.scroll_to(&view, first + 3)?;
            self.wheel(300.)?;
            self.wheel(-200.)?;
            self.floats(&view, on.then_some(first), &format!("{ahead}-previous"))?;
            self.scroll_to(&view, first)?;
            self.floats(&view, None, &format!("{ahead}-top"))?;
            if on {
                self.opened(&view, second)?;
            }
        }
        let shell = self.drawn.shell.clone();
        self.drawn.cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.navigate(Route::Settings(SettingsPage::Chat), cx);
            });
        });
        self.both("settings-chat")?;
        self.drawn.cx.update(|cx| {
            settings::update(cx, |settings| {
                settings.floating_prompts = true;
                settings.chat_verbosity = Preset::Normal.verbosity();
            });
            shell.update(cx, |shell, cx| shell.navigate(Route::Workspace, cx));
        });
        self.drawn.draw()
    }

    /// The second prompt floating, opened, then gone to.
    fn opened(&mut self, view: &Entity<ChatView>, second: usize) -> Result<()> {
        self.scroll_to(view, second + 3)?;
        self.wheel(400.)?;
        self.wheel(-300.)?;
        self.drawn
            .cx
            .update(|cx| view.update(cx, ChatView::open_floating));
        let open = self.drawn.cx.read(|cx| {
            view.read(cx)
                .floating
                .as_ref()
                .is_some_and(|floating| floating.open)
        });
        if !open {
            return Err("the floating prompt didn't open".into());
        }
        self.floats(view, Some(second), "float-open")?;
        self.drawn
            .cx
            .update(|cx| view.update(cx, ChatView::go_to_floating));
        self.drawn.draw()?;
        let top = self.drawn.cx.read(|cx| {
            view.read(cx)
                .showing()
                .map(|shown| shown.scroller.read(cx).logical_scroll_top().item_ix)
        });
        if top != Some(second) {
            return Err(format!("going to it went to row {top:?}, not {second}"));
        }
        self.floats(view, None, "float-gone-to")
    }

    /// The end of `view`'s timeline, followed.
    fn at_end(&mut self, view: &Entity<ChatView>) -> Result<()> {
        self.drawn.cx.update(|cx| {
            let scroller = view.read(cx).showing().map(|shown| shown.scroller.clone());
            if let Some(scroller) = scroller {
                scroller.update(
                    cx,
                    gpui_kit::component::message_scroller::MessageScrollerState::scroll_to_end,
                );
            }
        });
        self.settle_text()
    }

    /// A turn of the wheel over the timeline, `by` points down the chat.
    fn wheel(&mut self, by: f32) -> Result<()> {
        let window = self.drawn.window;
        self.drawn.cx.simulate_event(
            window,
            ScrollEvent {
                position: point(px(900.), px(400.)),
                delta: ScrollDelta::Pixels(point(px(0.), px(-by))),
                modifiers: Modifiers::default(),
                ..ScrollEvent::default()
            },
        );
        self.redraw()
    }
}
