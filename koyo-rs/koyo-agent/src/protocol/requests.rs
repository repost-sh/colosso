//! What the agent waits on a person for (`docs/chat.md` C14): an approval, a question or a
//! proposed plan, and how each is answered: where it's shown, or by the message sent next.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ids::{MessageId, RequestId, RunId, ThreadId, Time};
use super::model::{Answers, ToolFamily};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    Approval,
    Question,
    Plan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    Pending,
    Answered,
    Cancelled,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: RequestId,
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub status: RequestStatus,
    pub payload: RequestPayload,
    pub answer: Option<Answer>,
    pub created_at: Time,
    pub resolved_at: Option<Time>,
    pub seq: u64,
}

/// What a request asks, by its `kind`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RequestPayload {
    Approval(Approval),
    Question(Questions),
    /// A proposed plan. Never live: the agent stopped at it, and the next message refines it.
    Plan {
        markdown: String,
    },
    /// A kind this build doesn't know, from a newer host.
    #[serde(untagged)]
    Unknown(Value),
}

impl RequestPayload {
    /// The kind of response it takes; `None` for a kind this build doesn't know.
    pub fn kind(&self) -> Option<RequestKind> {
        match self {
            RequestPayload::Approval(_) => Some(RequestKind::Approval),
            RequestPayload::Question(_) => Some(RequestKind::Question),
            RequestPayload::Plan { .. } => Some(RequestKind::Plan),
            RequestPayload::Unknown(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub tool: String,
    pub family: ToolFamily,
    pub title: String,
    /// What it's asked of, as a line of work goes by it once it has run.
    pub label: String,
    /// What is asked, shown as is: a command, a path, or else the input.
    pub detail: String,
    /// An edit's change to the file `detail` names: its lines, each starting `+` or `-`.
    pub change: Option<Vec<String>>,
    pub input: Value,
    pub reason: Option<String>,
    /// What allowing for this chat adds, as a person is offered it, when it can.
    pub allow_session: Option<AllowSession>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowSession {
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Questions {
    pub questions: Vec<Question>,
    /// Whether the agent waits for the answer; else the next message carries it.
    pub live: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub id: String,
    pub header: String,
    /// What's asked; the wire's `question`.
    #[serde(rename = "question")]
    pub text: String,
    pub multi_select: bool,
    pub options: Vec<QuestionOption>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
    pub preview: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    AllowSession,
    Deny,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanDecision {
    Implement,
    Dismiss,
}

/// How a person answers a request where it's shown; its kind is the request's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Approval {
        decision: Decision,
        message: Option<String>,
    },
    /// By the question's id. No answers at all is skipping it, which declines the tool.
    Question {
        answers: Answers,
    },
    Plan {
        decision: PlanDecision,
    },
}

/// How a request was answered: where it's shown, or by the message sent next.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Answer {
    Message {
        #[serde(rename = "messageId")]
        message_id: MessageId,
    },
    /// Where it's shown, as the person responded.
    #[serde(untagged)]
    Response(Response),
    /// A kind this build doesn't know, from a newer host.
    #[serde(untagged)]
    Unknown(Value),
}
