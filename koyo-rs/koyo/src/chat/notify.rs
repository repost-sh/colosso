//! What a chat tells a person who isn't looking (`docs/chat.md` 5.3): a macOS notification when it
//! needs them or finishes while Koyo isn't frontmost, which opens the chat when clicked.

use gpui_kit::Context;
use koyo_agent::protocol::model::{Status, ThreadSummary};
use koyo_agent::protocol::requests::RequestKind;

use super::status::is_frontmost;
use super::{Chats, ChatsEvent};

impl Chats {
    /// A person who isn't looking hears of what's changed in a chat, as `now` has it.
    pub(super) fn tell_of(&self, now: &ThreadSummary, cx: &mut Context<Self>) {
        let was = self.summaries.get(&now.id).filter(|_| !is_frontmost(cx));
        if let Some(said) = was.and_then(|was| news(was, now)) {
            cx.emit(ChatsEvent::News {
                id: now.id.clone(),
                title: now.title.clone(),
                said,
            });
        }
    }
}

/// What going from `was` to `now` says, if it's worth a person's attention.
fn news(was: &ThreadSummary, now: &ThreadSummary) -> Option<&'static str> {
    if let (None, Some(kind)) = (was.needs, now.needs) {
        return Some(match kind {
            RequestKind::Approval => "Needs your approval",
            RequestKind::Question => "Has a question for you",
            RequestKind::Plan => "Has a plan for you to look at",
        });
    }
    if was.status != Status::Working || now.needs.is_some() {
        return None;
    }
    match now.status {
        Status::Working => None,
        Status::Idle => Some("Finished"),
        Status::Failed => Some("Stopped with an error"),
    }
}

#[cfg(test)]
mod tests {
    use koyo_agent::protocol::model::{Status, ThreadSummary};
    use koyo_agent::protocol::requests::RequestKind;

    use super::news;
    use crate::chat::tests::summary as tests_summary;

    fn summary(status: Status, needs: Option<RequestKind>) -> ThreadSummary {
        ThreadSummary {
            status,
            needs,
            ..tests_summary("0001", "/work".as_ref(), "A chat", true)
        }
    }

    #[test]
    fn a_chat_says_so_when_it_starts_to_need_a_person() {
        let working = summary(Status::Working, None);
        let asking = summary(Status::Working, Some(RequestKind::Approval));
        assert_eq!(news(&working, &asking), Some("Needs your approval"));
        assert_eq!(news(&asking, &asking), None, "only as it starts");
    }

    #[test]
    fn a_chat_says_so_when_its_work_ends() {
        let working = summary(Status::Working, None);
        assert_eq!(news(&working, &summary(Status::Idle, None)), Some("Finished"));
        assert_eq!(news(&working, &summary(Status::Failed, None)), Some("Stopped with an error"));
    }

    #[test]
    fn nothing_else_is_news() {
        let idle = summary(Status::Idle, None);
        assert_eq!(news(&idle, &idle), None);
        assert_eq!(news(&idle, &summary(Status::Working, None)), None);
        assert_eq!(news(&summary(Status::Failed, None), &idle), None);
    }
}
