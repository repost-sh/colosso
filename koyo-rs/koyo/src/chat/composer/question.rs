//! A question the agent waits on, in a drawer attached above what's typed (`docs/chat.md` 5.3): one
//! thing at a time, its options as rows with their digits. A single choice goes on by itself after
//! a moment; several are chosen and then gone on from. What's typed is the "Other" answer, and
//! skipping answers with nothing, which declines the tool.

use std::collections::BTreeMap;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyElement, App, ClickEvent, Context, Div, IntoElement, KeyBinding, ParentElement as _,
    Stateful, StatefulInteractiveElement as _, Styled, Task, Window, actions, div, px,
};
use koyo_agent::protocol::ids::RequestId;
use koyo_agent::protocol::model::Answers;
use koyo_agent::protocol::requests::{Question, QuestionOption, Response};

use super::ChatComposer;
use super::asked::Asked;
use crate::chat::output::scroll_box;
use crate::shell::{glass, icon, icon_button, row, solid};
use crate::theme::Palette;

/// The composer's context while a question shows and nothing's typed, where the keys answer it.
pub(super) const CHOOSING: &str = "ChatChoosing";
/// How long a single choice shows before it goes on, so a person sees what they chose.
const GOES_ON_AFTER: Duration = Duration::from_millis(200);
/// The keys that choose the options, in their order.
const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];

actions!(chat, [NextQuestion, SkipQuestion]);

/// Chooses the option at this place of the question that shows.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = chat, no_json)]
pub(super) struct ChooseOption(pub(super) usize);

pub(super) fn bind_keys(cx: &mut App) {
    let context = format!("{CHOOSING} > Input");
    let context = Some(context.as_str());
    cx.bind_keys([
        KeyBinding::new("enter", NextQuestion, context),
        KeyBinding::new("cmd-backspace", SkipQuestion, context),
    ]);
    cx.bind_keys(
        (DIGITS.iter().enumerate())
            .map(|(option, digit)| KeyBinding::new(digit, ChooseOption(option), context)),
    );
}

pub(super) struct QuestionDrawer {
    pub(super) request: RequestId,
    questions: Vec<Question>,
    /// Which one shows; past the last once they're all answered, and that's been said.
    at: usize,
    /// What's been said to each, as far as it's got: the one that shows may have no answer yet.
    answers: Answers,
    /// The option a person points at, whose preview shows.
    pointing_at: Option<usize>,
    /// Waits to go on from a single choice.
    going_on: Option<Task<()>>,
}

impl QuestionDrawer {
    pub(super) fn new(request: RequestId, questions: Vec<Question>) -> QuestionDrawer {
        QuestionDrawer {
            request,
            questions,
            at: 0,
            answers: BTreeMap::new(),
            pointing_at: None,
            going_on: None,
        }
    }

    pub(super) fn shown(&self) -> Option<&Question> {
        self.questions.get(self.at)
    }

    /// What's chosen of the one that shows.
    fn chosen(&self) -> &[String] {
        let answer = self
            .shown()
            .and_then(|question| self.answers.get(&question.id));
        answer.map_or(&[], Vec::as_slice)
    }

    /// Chooses what's at `option`, or takes it back if several can be chosen. Whether it should
    /// go on from that, or `None` if there's no such option.
    fn choose(&mut self, option: usize) -> Option<bool> {
        let question = self.questions.get(self.at)?;
        let label = question.options.get(option)?.label.clone();
        let chosen = self.answers.entry(question.id.clone()).or_default();
        if !question.multi_select {
            *chosen = vec![label];
            return Some(true);
        }
        if let Some(held) = chosen.iter().position(|each| *each == label) {
            chosen.remove(held);
        } else {
            chosen.push(label);
        }
        Some(false)
    }

    /// Adds what was typed to the answer of the one that shows, and shows the next. Whether it
    /// did: there has to be an answer to go on from.
    fn go_on(&mut self, other: Option<String>) -> bool {
        let Some(question) = self.questions.get(self.at) else {
            return false;
        };
        let answer = self.answers.entry(question.id.clone()).or_default();
        if let Some(other) = other {
            // One choice and something typed are two answers to what takes one: the words win.
            if !question.multi_select {
                answer.clear();
            }
            answer.push(other);
        }
        if answer.is_empty() {
            return false;
        }
        self.show(self.at + 1);
        true
    }

    /// Shows the one at `at`, as it was left.
    fn show(&mut self, at: usize) {
        self.at = at;
        self.going_on = None;
        self.pointing_at = None;
    }

    /// The answers, once there's one for all of them.
    fn finished(&self) -> Option<Answers> {
        self.shown().is_none().then(|| self.answers.clone())
    }

    /// The answer didn't go, so the last of them shows again.
    pub(super) fn not_finished(&mut self) {
        if self.shown().is_none() {
            self.show(self.questions.len().saturating_sub(1));
        }
    }

    /// Which option's preview shows: the one pointed at, or else what's chosen, or else the first.
    fn previewed(&self) -> Option<usize> {
        let options = &self.shown()?.options;
        let chosen = || {
            options
                .iter()
                .position(|option| self.chosen().contains(&option.label))
        };
        let at = self.pointing_at.or_else(chosen).unwrap_or(0);
        options.get(at)?.preview.as_ref().map(|_| at)
    }

    /// The drawer, while a question shows.
    pub(super) fn draw(&self, p: Palette, cx: &mut Context<ChatComposer>) -> Option<AnyElement> {
        let question = self.shown()?;
        let options = div().flex_1().min_w_0().flex().flex_col().gap_1().children(
            (question.options.iter().enumerate())
                .map(|(at, option)| self.option_row(at, option, p, cx)),
        );
        let has_previews = question
            .options
            .iter()
            .any(|option| option.preview.is_some());
        let previewed = self.previewed();
        // All are drawn, one showing: a text view that isn't drawn forgets what it parsed.
        let previews = has_previews.then(|| {
            scroll_box("chat-question-preview", p)
                .flex_1()
                .min_w_0()
                .p_2()
                .text_xs()
                .children(
                    (question.options.iter().enumerate()).filter_map(|(at, option)| {
                        let preview = option.preview.clone()?;
                        let text = TextView::markdown(("chat-option-preview", at), preview);
                        Some(
                            div()
                                .when(previewed != Some(at), Styled::hidden)
                                .child(text),
                        )
                    }),
                )
        });
        let drawer = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.heading(question, p, cx))
            .child(div().text_sm().child(question.text.clone()))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(options)
                    .children(previews),
            )
            .when(question.multi_select, |drawer| {
                drawer.child(self.next_button(cx))
            });
        Some(drawer.into_any_element())
    }

    /// What it's about and how far along, with the ways back and out.
    fn heading(&self, question: &Question, p: Palette, cx: &mut Context<ChatComposer>) -> Div {
        let count = self.questions.len();
        div()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(p.muted)
            .when(self.at > 0, |row| {
                row.child(
                    icon_button("chat-question-back", IconName::ChevronLeft, "Back", cx)
                        .small()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.previous_question(cx);
                        })),
                )
            })
            .child(question.header.clone())
            .when(count > 1, |row| {
                row.child(format!("{} of {count}", self.at + 1))
            })
            .child(div().flex_1())
            .child(
                glass("chat-question-skip", p.text, cx)
                    .small()
                    .label("Skip")
                    .tooltip("⌘⌫ with nothing typed")
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.skip_question(window, cx);
                    })),
            )
    }

    fn option_row(
        &self,
        at: usize,
        option: &QuestionOption,
        p: Palette,
        cx: &mut Context<ChatComposer>,
    ) -> Stateful<Div> {
        let chosen = self.chosen().contains(&option.label);
        let words = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(option.label.clone())
            .children(
                option
                    .description
                    .clone()
                    .filter(|words| !words.is_empty())
                    .map(|words| div().text_xs().text_color(p.muted).child(words)),
            );
        row(("chat-question-option", at), chosen, p)
            .h_auto()
            .items_start()
            .py_1()
            .on_hover(cx.listener(move |this, pointing: &bool, _, cx| {
                this.point_at(at, *pointing, cx);
            }))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.choose_option(at, window, cx);
            }))
            .child(
                div()
                    .w(px(14.))
                    .flex_shrink_0()
                    .text_color(p.muted)
                    .when(at < DIGITS.len(), |digit| digit.child((at + 1).to_string())),
            )
            .child(words)
            .when(chosen, |row| row.child(icon(IconName::Check, 14.)))
    }

    fn next_button(&self, cx: &mut Context<ChatComposer>) -> Div {
        let last = self.at + 1 == self.questions.len();
        let next = solid("chat-question-next", !self.chosen().is_empty(), cx)
            .small()
            .label(if last { "Send" } else { "Next" })
            .tooltip("↩ with nothing typed")
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.next_question(None, window, cx);
            }));
        div().flex().justify_end().child(next)
    }
}

impl ChatComposer {
    /// Chooses an option of the question that shows, and goes on from a single choice in a moment.
    /// A digit past the options is someone starting to type, which the key goes on to do.
    pub(super) fn choose_option(
        &mut self,
        option: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Asked::Question(drawer) = &mut self.asked else {
            return;
        };
        match drawer.choose(option) {
            None => {
                cx.propagate();
                return;
            }
            Some(false) => {}
            Some(true) => {
                drawer.going_on = Some(cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(GOES_ON_AFTER).await;
                    this.update_in(cx, |this, window, cx| {
                        this.next_question(None, window, cx);
                    })
                    .ok();
                }));
            }
        }
        cx.notify();
    }

    /// Goes on to the next question, with `typed` as an answer of this one, and answers the agent
    /// once they've all been.
    pub(super) fn next_question(
        &mut self,
        typed: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Asked::Question(drawer) = &mut self.asked else {
            return;
        };
        if !drawer.go_on(typed.clone()) {
            return;
        }
        let finished = drawer.finished();
        cx.notify();
        if let Some(answers) = finished {
            self.respond(Response::Question { answers }, typed, window, cx);
        }
    }

    fn previous_question(&mut self, cx: &mut Context<Self>) {
        if let Asked::Question(drawer) = &mut self.asked {
            drawer.show(drawer.at.saturating_sub(1));
            cx.notify();
        }
    }

    /// Answers with nothing, which is what skipping says. Nothing shows any more, as all that's
    /// left is for the answer to arrive.
    pub(super) fn skip_question(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Asked::Question(drawer) = &mut self.asked else {
            return;
        };
        if drawer.shown().is_none() {
            return;
        }
        drawer.show(drawer.questions.len());
        let response = Response::Question {
            answers: BTreeMap::new(),
        };
        self.respond(response, None, window, cx);
        cx.notify();
    }

    fn point_at(&mut self, option: usize, pointing: bool, cx: &mut Context<Self>) {
        let Asked::Question(drawer) = &mut self.asked else {
            return;
        };
        if pointing {
            drawer.pointing_at = Some(option);
        } else if drawer.pointing_at == Some(option) {
            drawer.pointing_at = None;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use koyo_agent::protocol::ids::RequestId;
    use koyo_agent::protocol::requests::Question;

    use super::QuestionDrawer;
    use crate::chat::tests::question;

    fn asked(text: &str, multi_select: bool) -> Question {
        question(text, multi_select, &["A", "B", "C"])
    }

    fn drawer(questions: Vec<Question>) -> QuestionDrawer {
        QuestionDrawer::new(RequestId("q".into()), questions)
    }

    #[test]
    fn going_back_shows_what_was_chosen_and_it_can_be_changed() {
        let mut drawer = drawer(vec![asked("one", false), asked("two", false)]);
        assert_eq!(drawer.choose(0), Some(true));
        assert!(drawer.go_on(None));
        assert!(
            drawer.chosen().is_empty(),
            "the next one starts with nothing chosen"
        );

        drawer.show(0);
        assert_eq!(drawer.chosen(), ["A"]);
        assert_eq!(drawer.choose(1), Some(true));
        assert!(drawer.go_on(None));
        assert_eq!(drawer.choose(2), Some(true));
        assert!(drawer.go_on(None));

        let answers = drawer.finished().expect("they've all been answered");
        assert_eq!(answers["one"], ["B"]);
        assert_eq!(answers["two"], ["C"]);
    }

    #[test]
    fn words_stand_for_a_single_choice_and_join_several() {
        let mut one = drawer(vec![asked("one", false)]);
        one.choose(0);
        assert!(one.go_on(Some("mine".into())));
        assert_eq!(one.answers["one"], ["mine"]);

        let mut several = drawer(vec![asked("one", true)]);
        several.choose(0);
        several.choose(1);
        assert!(several.go_on(Some("mine".into())));
        assert_eq!(several.answers["one"], ["A", "B", "mine"]);
    }

    #[test]
    fn an_answer_that_did_not_go_shows_the_last_again_as_it_was() {
        let mut drawer = drawer(vec![asked("one", false), asked("two", true)]);
        drawer.choose(0);
        assert!(drawer.go_on(None));
        drawer.choose(1);
        assert!(drawer.go_on(None));
        assert!(drawer.shown().is_none());

        drawer.not_finished();

        assert_eq!(drawer.shown().map(|shown| shown.id.as_str()), Some("two"));
        assert_eq!(drawer.chosen(), ["B"]);
    }

    #[test]
    fn a_preview_is_of_what_is_pointed_at_then_chosen_then_the_first() {
        let mut with_previews = asked("one", false);
        with_previews.options[0].preview = Some("a".into());
        with_previews.options[2].preview = Some("c".into());
        let mut drawer = drawer(vec![with_previews]);

        assert_eq!(drawer.previewed(), Some(0));
        drawer.choose(2);
        assert_eq!(drawer.previewed(), Some(2));
        drawer.pointing_at = Some(1);
        assert_eq!(drawer.previewed(), None, "that one has none");
        drawer.pointing_at = Some(0);
        assert_eq!(drawer.previewed(), Some(0));
    }
}
