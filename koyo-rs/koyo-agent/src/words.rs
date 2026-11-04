//! How a row says what it shows, as a person would: what a tool did, how long it took, why a run
//! failed. What goes with the words, such as an icon, is the app's.

use crate::protocol::ids::Time;
use crate::protocol::methods::Model;
use crate::protocol::model::{
    Compaction, Effort, Failure, FailureKind, Item, ItemBody, ItemStatus, Mode, Run, TodoStatus,
    Tool, ToolFamily,
};
use crate::protocol::requests::{
    Answer, Approval, Decision, PlanDecision, Question, Request, RequestPayload, RequestStatus,
    Response,
};

/// What shows under a final answer: "Opus 5.5 · High · 14:30", the model as `models` calls it.
pub fn answer_meta(run: &Run, models: &[Model]) -> String {
    let model = run.model.as_ref().map(|id| {
        let known = models.iter().find(|model| model.id == *id);
        known.map_or_else(|| id.clone(), |model| model_name(model).to_owned())
    });
    let parts = [
        model,
        run.effort.map(|effort| effort_label(effort).to_owned()),
        run.ended_at.as_ref().and_then(clock),
    ];
    parts.into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

/// What a model goes by: what the catalog calls it, less what it says of it in brackets.
pub fn model_name(model: &Model) -> &str {
    model.name.split(" (").next().unwrap_or(&model.name)
}

pub fn effort_label(effort: Effort) -> &'static str {
    match effort {
        Effort::Low => "Low",
        Effort::Medium => "Medium",
        Effort::High => "High",
        Effort::Xhigh => "Extra high",
        Effort::Max => "Max",
    }
}

fn parsed(time: &Time) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(&time.0).ok()
}

/// A time of day, here.
pub fn clock(time: &Time) -> Option<String> {
    let at = parsed(time)?.with_timezone(&chrono::Local);
    Some(at.format("%H:%M").to_string())
}

/// How long a settled run took of the agent's own time: what it waited on a person for isn't work.
/// It doesn't wait on a plan, which is answered after the run; nor past its end on a question left
/// when the host stopped.
pub fn worked_for(run: &Run, requests: &[Request]) -> Option<String> {
    let started = parsed(run.started_at.as_ref()?)?;
    let ended = parsed(run.ended_at.as_ref()?)?;
    let waited: i64 = (requests.iter())
        .filter(|request| request.run_id == run.id)
        .filter(|request| !matches!(request.payload, RequestPayload::Plan { .. }))
        .filter_map(|request| {
            let answered = parsed(request.resolved_at.as_ref()?)?.min(ended);
            Some((answered - parsed(&request.created_at)?).num_seconds())
        })
        .sum();
    let worked = (ended - started).num_seconds() - waited;
    Some(duration(worked.max(1).unsigned_abs()))
}

pub fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Ask => "Ask",
        Mode::AcceptEdits => "Accept edits",
        Mode::Auto => "Auto",
        Mode::FullAccess => "Full access",
    }
}

/// How full the conversation is: "42 % · 84k of 200k".
pub fn context_line(held: u64, room: u64) -> String {
    let percent = held.saturating_mul(100).checked_div(room).unwrap_or(0);
    format!("{percent} % · {} of {}", tokens(held), tokens(room))
}

/// A length of time as a person says it: "41s", "1m 3s", "2h 5m".
pub fn duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
    }
}

/// A number of tokens as a person says it: "31k".
fn tokens(count: u64) -> String {
    if count < 1000 {
        count.to_string()
    } else {
        format!("{}k", count / 1000)
    }
}

pub fn compacted(compaction: &Compaction) -> String {
    match (compaction.before, compaction.after) {
        (Some(before), Some(after)) => {
            format!("Context compacted · {} → {} tokens", tokens(before), tokens(after))
        }
        _ => "Context compacted".to_owned(),
    }
}

pub fn failed(failure: &Failure) -> String {
    match (failure.kind, failure.retry_at.as_ref().and_then(clock)) {
        (FailureKind::UsageLimit, Some(at)) => format!("Usage limit reached · resets {at}"),
        (FailureKind::UsageLimit, None) => "Usage limit reached".to_owned(),
        _ => failure.message.clone(),
    }
}

/// One sentence for work done together: "Read 3 files and ran 2 commands", or "Thought" for
/// nothing but thinking.
pub fn work_summary<'a>(items: impl Iterator<Item = &'a Item>) -> String {
    let mut counts: Vec<(ToolFamily, usize)> = Vec::new();
    let mut declined = 0;
    for item in items {
        if item.status == ItemStatus::Declined {
            declined += 1;
            continue;
        }
        let Some(family) = family_of(item) else {
            continue;
        };
        match counts.iter_mut().find(|(counted, _)| *counted == family) {
            Some((_, count)) => *count += 1,
            None => counts.push((family, 1)),
        }
    }
    let mut sentence = String::new();
    let last = counts.len().saturating_sub(1);
    for (at, (family, count)) in counts.iter().enumerate() {
        if at > 0 {
            sentence.push_str(if at == last { " and " } else { ", " });
        }
        sentence.push_str(&did(*family, *count));
    }
    let mut letters = sentence.chars();
    let first = letters.next().map(|first| first.to_uppercase().chain(letters).collect::<String>());
    match (first, declined) {
        (Some(done), 0) => done,
        (Some(done), declined) => format!("{done} · {declined} declined"),
        (None, 0) => "Thought".to_owned(),
        (None, declined) => format!("{declined} declined"),
    }
}

/// What an item of work counts as in a sentence; thinking isn't counted.
fn family_of(item: &Item) -> Option<ToolFamily> {
    match &item.body {
        ItemBody::Tool(tool) => Some(tool.family),
        ItemBody::Subagent(_) => Some(ToolFamily::Agent),
        ItemBody::Todos(_) => Some(ToolFamily::Todo),
        _ => None,
    }
}

fn did(family: ToolFamily, count: usize) -> String {
    let (verb, one, many) = match family {
        ToolFamily::Command => ("ran", "a command", "commands"),
        ToolFamily::Read => ("read", "a file", "files"),
        ToolFamily::Edit => ("changed", "a file", "files"),
        ToolFamily::Search => ("ran", "a search", "searches"),
        ToolFamily::Mcp => ("called", "a tool", "tools"),
        ToolFamily::Agent => ("ran", "an agent", "agents"),
        ToolFamily::Skill => ("used", "a skill", "skills"),
        ToolFamily::Other => ("used", "a tool", "tools"),
        ToolFamily::Web => return "searched the web".to_owned(),
        ToolFamily::Todo => return "updated the to-dos".to_owned(),
    };
    if count == 1 {
        format!("{verb} {one}")
    } else {
        format!("{verb} {count} {many}")
    }
}

/// What a tool's line says: "Ran cargo test", "Running cargo test", "Read src/main.rs", with
/// paths under `cwd` relative to it.
pub fn tool_line(tool: &Tool, status: ItemStatus, cwd: &str) -> String {
    let running = status == ItemStatus::Running || status == ItemStatus::Pending;
    let (doing, done) = match tool.family {
        ToolFamily::Command => ("Running", "Ran"),
        ToolFamily::Read => ("Reading", "Read"),
        ToolFamily::Edit => ("Editing", "Edited"),
        ToolFamily::Search => ("Searching for", "Searched for"),
        ToolFamily::Web => ("Searching the web for", "Searched the web for"),
        ToolFamily::Mcp => ("Calling", "Called"),
        ToolFamily::Skill | ToolFamily::Other => ("Using", "Used"),
        ToolFamily::Todo => return "Updated the to-dos".to_owned(),
        // What it was asked to do reads as a line of its own.
        ToolFamily::Agent => return tool.label.clone(),
    };
    let what = from_checkout(&tool.label, cwd);
    // It never ran: what it would have done is all there is to say, beside "Declined".
    if status == ItemStatus::Declined {
        return what;
    }
    let verb = if running { doing } else { done };
    format!("{verb} {what}")
}

/// What's left of an approval once it's no longer waiting: "Allowed cargo test", in the person's
/// words if they gave any.
pub fn approval_line(request: &Request, approval: &Approval, cwd: &str) -> String {
    let what = from_checkout(&approval.label, cwd);
    let Some(Answer::Response(Response::Approval { decision, message })) = &request.answer else {
        return format!("No longer asked: {what}");
    };
    match (decision, message) {
        (Decision::Allow, _) => format!("Allowed {what}"),
        (Decision::AllowSession, _) => format!("Allowed {what} for this chat"),
        (Decision::Deny, Some(message)) => format!("Denied {what}: {message}"),
        (Decision::Deny, None) => format!("Denied {what}"),
        (Decision::Cancel, _) => format!("Stopped at {what}"),
    }
}

/// What's said of a question's request, a line for each thing it asks: what was asked while it
/// waits, and what the person said once it's answered.
pub fn question_lines(request: &Request, questions: &[Question]) -> Vec<String> {
    let line = |question: &Question| {
        let what = &question.text;
        match &request.answer {
            Some(Answer::Response(Response::Question { answers })) => {
                match answers.get(&question.id).filter(|chosen| !chosen.is_empty()) {
                    Some(chosen) => format!("{}: {}", question.header, chosen.join(", ")),
                    None => format!("Skipped: {what}"),
                }
            }
            Some(Answer::Message { .. }) => format!("Answered by your message: {what}"),
            _ if request.status == RequestStatus::Pending => format!("Asked: {what}"),
            _ => format!("No longer asked: {what}"),
        }
    };
    questions.iter().map(line).collect()
}

/// A plan's title, its first heading, and what's to show under it: the rest if it starts with
/// that heading, or else all of it.
pub fn plan_parts(markdown: &str) -> (Option<&str>, &str) {
    let start = markdown.trim_start();
    let (first, rest) = start.split_once('\n').unwrap_or((start, ""));
    match heading(first) {
        Some(title) => (Some(title), rest.trim_start()),
        None => (markdown.lines().find_map(heading), markdown),
    }
}

/// The words of a line of markdown that's a heading.
fn heading(line: &str) -> Option<&str> {
    let words = line.trim_start_matches('#');
    (words.len() < line.len() && words.starts_with(' ')).then(|| words.trim())
}

/// What became of a plan, once something did: nothing's said of one still waiting.
pub fn plan_outcome(request: &Request) -> Option<&'static str> {
    match request.answer.as_ref()? {
        Answer::Response(Response::Plan { decision }) => Some(match decision {
            PlanDecision::Implement => "Implemented",
            PlanDecision::Dismiss => "Dismissed",
        }),
        Answer::Message { .. } => Some("Refined by your message"),
        _ => None,
    }
}

/// `text` with the paths under `cwd` relative to it.
fn from_checkout(text: &str, cwd: &str) -> String {
    text.replace(&format!("{}/", cwd.trim_end_matches('/')), "")
}

/// What the line of an item of work says.
pub fn work_line(item: &Item, cwd: &str) -> String {
    match &item.body {
        ItemBody::Tool(tool) => tool_line(tool, item.status, cwd),
        ItemBody::Reasoning(text) => {
            let last = text.text.lines().rfind(|line| !line.trim().is_empty());
            last.unwrap_or("Thought").trim().to_owned()
        }
        ItemBody::Todos(todos) => {
            let done = (todos.todos.iter()).filter(|todo| todo.status == TodoStatus::Completed);
            format!("Updated the to-dos · {} of {} done", done.count(), todos.todos.len())
        }
        ItemBody::Subagent(agent) => agent.description.clone(),
        _ => "Worked".to_owned(),
    }
}

/// What the live row says of what a run is working on, if it's working on anything yet.
pub fn live_line(item: Option<&Item>, cwd: &str) -> String {
    match item.map(|item| &item.body) {
        Some(ItemBody::Tool(tool)) => tool_line(tool, ItemStatus::Running, cwd),
        Some(ItemBody::Subagent(agent)) => agent.description.clone(),
        Some(ItemBody::Reasoning(_)) => "Thinking…".to_owned(),
        _ => "Working…".to_owned(),
    }
}
