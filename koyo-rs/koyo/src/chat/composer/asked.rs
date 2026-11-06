//! What the agent waits on a person for now, as the composer holds and answers it (`docs/chat.md`
//! 5.3). Its own copy, which changes only with what's asked, since a token updates the thread and
//! the composer is drawn for anything it read. Handlers read the thread freely.

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::{
    Action, AnyElement, App, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    KeyBinding, ParentElement as _, SharedString, Styled as _, Window, actions, div, px,
};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::ids::RequestId;
use koyo_agent::protocol::requests::{
    Decision, PlanDecision, Questions, Request, RequestPayload, Response,
};
use koyo_agent::protocol::wire::{ErrorCode, WireError};

use super::question::{CHOOSING, ChooseOption, NextQuestion, QuestionDrawer, SkipQuestion};
use super::{ChatComposer, ComposerEvent};
use crate::chat::asks::{
    ANSWERABLE, AllowRequest, AllowRequestForChat, DenyRequest, SavePlan, StopRequest,
};
use crate::shell::{glass, icon, solid};
use crate::theme::Palette;

/// The composer's context while a plan is asked and nothing's typed, where the key implements it.
const PLANNED: &str = "ChatPlanned";
/// What a plan's file is called until a person says otherwise.
const PLAN_FILE: &str = "plan.md";

actions!(chat, [ImplementPlan]);

pub(super) fn bind_keys(cx: &mut App) {
    let context = format!("{PLANNED} > Input");
    cx.bind_keys([KeyBinding::new(
        "cmd-enter",
        ImplementPlan,
        Some(context.as_str()),
    )]);
}

pub(super) enum Asked {
    Nothing,
    /// What the keys answer, and what's typed declines; for this chat if that's something it can
    /// be allowed for.
    Approval {
        request: RequestId,
        for_chat: bool,
    },
    Question(QuestionDrawer),
    /// A plan: its drawer implements it, and what's typed refines it.
    Plan(RequestId),
    /// What the next message answers: a question the agent stopped waiting on.
    Message(RequestId),
}

impl Asked {
    pub(super) fn of(request: &Request) -> Asked {
        let id = request.id.clone();
        match &request.payload {
            RequestPayload::Approval(approval) => Asked::Approval {
                request: id,
                for_chat: approval.allow_session.is_some(),
            },
            RequestPayload::Question(Questions {
                questions,
                live: true,
            }) => Asked::Question(QuestionDrawer::new(id, questions.clone())),
            RequestPayload::Plan { .. } => Asked::Plan(id),
            RequestPayload::Question(_) | RequestPayload::Unknown(_) => Asked::Message(id),
        }
    }

    pub(super) fn request(&self) -> Option<&RequestId> {
        match self {
            Asked::Nothing => None,
            Asked::Question(drawer) => Some(&drawer.request),
            Asked::Approval { request, .. } | Asked::Plan(request) | Asked::Message(request) => {
                Some(request)
            }
        }
    }

    pub(super) fn placeholder(&self) -> &'static str {
        match self {
            Asked::Nothing => "Ask Claude…",
            Asked::Approval { .. } => "Say what to do instead…",
            Asked::Question(_) => "Or type your own answer…",
            Asked::Plan(_) => "Say what to change…",
            Asked::Message(_) => "Answer here…",
        }
    }

    /// The composer's context while nothing's typed, in which the keys answer what's asked. They
    /// do what they always do otherwise.
    pub(super) fn key_context(&self) -> Option<&'static str> {
        match self {
            Asked::Approval { .. } => Some(ANSWERABLE),
            Asked::Question(_) => Some(CHOOSING),
            Asked::Plan(_) => Some(PLANNED),
            Asked::Nothing | Asked::Message(_) => None,
        }
    }
}

impl ChatComposer {
    /// What's asked, attached above what's typed, for what's answered there.
    pub(super) fn drawer(
        &self,
        typed: &str,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let drawn = match &self.asked {
            Asked::Question(drawer) => drawer.draw(p, cx)?,
            Asked::Plan(_) => plan_ready(typed, p, cx).into_any_element(),
            _ => return None,
        };
        Some(attached_above(drawn, p).into_any_element())
    }

    /// Answers the approval asked now, if there is one and it can be answered so.
    fn answer(&mut self, decision: Decision, window: &mut Window, cx: &mut Context<Self>) {
        let Asked::Approval { for_chat, .. } = &self.asked else {
            return;
        };
        if decision == Decision::AllowSession && !for_chat {
            return;
        }
        let response = Response::Approval {
            decision,
            message: None,
        };
        self.respond(response, None, window, cx);
    }

    fn decide_plan(&mut self, decision: PlanDecision, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.asked, Asked::Plan(_)) {
            return;
        }
        if decision == PlanDecision::Implement {
            cx.emit(ComposerEvent::Sent);
        }
        self.respond(Response::Plan { decision }, None, window, cx);
    }

    /// Says `response` to what's asked now, in the words of `typed` if it was typed.
    pub(super) fn respond(
        &mut self,
        response: Response,
        typed: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(thread), Some(request)) = (&self.thread, self.asked.request()) else {
            return;
        };
        let request = request.clone();
        #[cfg(test)]
        self.said.push((request.clone(), response.clone()));
        let answered = self
            .chats
            .read(cx)
            .respond(thread.read(cx).id(), request, response);
        self.error = None;
        cx.spawn_in(window, async move |this, cx| {
            if let Err(error) = answered.await {
                this.update_in(cx, |this, window, cx| {
                    this.not_answered(typed.as_deref(), &error, window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// An answer didn't go. If what it answered was settled meanwhile that's no one's trouble, and
    /// what was typed stays for a person to send as they see fit (`docs/chat.md` 4.5.1).
    fn not_answered(
        &mut self,
        typed: Option<&str>,
        why: &ClientError,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settled = matches!(
            why,
            ClientError::Host(WireError {
                code: ErrorCode::Conflict | ErrorCode::RequestNotFound,
                ..
            })
        );
        let typed = typed.unwrap_or_default();
        if settled {
            self.give_back(typed, window, cx);
            return;
        }
        if let Asked::Question(drawer) = &mut self.asked {
            drawer.not_finished();
        }
        self.not_sent(typed, why, window, cx);
    }

    /// Asks where to keep the plan of `request`, and writes it there.
    fn save_plan(&mut self, request: &RequestId, cx: &mut Context<Self>) {
        let replica = self
            .thread
            .as_ref()
            .and_then(|thread| thread.read(cx).replica());
        let Some(markdown) = replica.and_then(|replica| replica.plan(request)) else {
            return;
        };
        let markdown = markdown.to_owned();
        let panel = cx.prompt_for_new_path(&self.cwd, Some(PLAN_FILE));
        cx.spawn(async move |this, cx| {
            let Some(path) = panel.await.ok().and_then(Result::ok).flatten() else {
                return;
            };
            let writing = cx
                .background_executor()
                .spawn(async move { std::fs::write(path, markdown) });
            if let Err(why) = writing.await {
                log::warn!("a plan wasn't saved: {why}");
                this.update(cx, |this, cx| {
                    this.error = Some(format!("The plan wasn't saved: {why}").into());
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// What happens on `A`: the approval asked now is answered with `decision`.
    fn answers<A: Action>(
        decision: Decision,
        cx: &mut Context<Self>,
    ) -> impl Fn(&A, &mut Window, &mut App) + 'static {
        cx.listener(move |this, _: &A, window, cx| this.answer(decision, window, cx))
    }

    /// `column`, which hears all that answers what's asked: the keys, and the rows' buttons.
    pub(super) fn hears_answers(column: Div, cx: &mut Context<Self>) -> Div {
        column
            .on_action(Self::answers::<AllowRequest>(Decision::Allow, cx))
            .on_action(Self::answers::<AllowRequestForChat>(
                Decision::AllowSession,
                cx,
            ))
            .on_action(Self::answers::<DenyRequest>(Decision::Deny, cx))
            .on_action(Self::answers::<StopRequest>(Decision::Cancel, cx))
            .on_action(cx.listener(|this, choice: &ChooseOption, window, cx| {
                this.choose_option(choice.0, window, cx);
            }))
            .on_action(cx.listener(|this, _: &NextQuestion, window, cx| {
                this.next_question(None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SkipQuestion, window, cx| {
                this.skip_question(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ImplementPlan, window, cx| {
                this.decide_plan(PlanDecision::Implement, window, cx);
            }))
            .on_action(cx.listener(|this, plan: &SavePlan, _, cx| this.save_plan(&plan.0, cx)))
    }
}

/// A plan's drawer: implement it, or with something typed, refine it by sending that.
fn plan_ready(typed: &str, p: Palette, cx: &mut Context<ChatComposer>) -> Div {
    let go = solid("chat-plan-go", true, cx).small();
    let go = if typed.trim().is_empty() {
        go.label("Implement")
            .tooltip("⌘↩ with nothing typed")
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.decide_plan(PlanDecision::Implement, window, cx);
            }))
    } else {
        go.label("Refine")
            .tooltip("Sends what's typed")
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.send(window, cx)))
    };
    let dismiss = glass("chat-plan-dismiss", p.text, cx)
        .small()
        .label("Dismiss")
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            this.decide_plan(PlanDecision::Dismiss, window, cx);
        }));
    drawer_line(IconName::ListChecks, "Plan ready", dismiss, go)
}

/// `drawn`, attached above what's typed, with a rule between them.
pub(super) fn attached_above(drawn: impl IntoElement, p: Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(drawn)
        .child(div().h(px(1.)).bg(p.hairline))
}

/// A drawer of a line: what it's about, and the two things to do about it, the one it leads to
/// last.
pub(super) fn drawer_line(
    mark: IconName,
    says: impl Into<SharedString>,
    other: Button,
    go: Button,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(icon(mark, 14.))
        .child(div().flex_1().text_sm().child(says.into()))
        .child(other)
        .child(go)
}
