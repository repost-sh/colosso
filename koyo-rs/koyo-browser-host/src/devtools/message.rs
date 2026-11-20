//! A message of a page's own `DevTools` session as Chromium writes it, told apart without
//! reading the rest, as CEF tells them apart (`ProtocolParser` in its `devtools_util.cc`): an
//! answer is `{"id":N,"result":{…}}` or `{"id":N,"error":{…}}`, an event
//! `{"method":"…","params":{…}}`, and a message of a session attached under the page's ends in
//! that session's `"sessionId"`. What CEF hands an observer on the main thread is read in full
//! only where the main thread needs what's in it.

/// What Chromium answered a command with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Answered {
    /// Its `result`: the command ran.
    Result,
    /// Its `error`.
    Error,
}

/// An event, as [`event`] tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Event<'a> {
    pub(crate) method: &'a str,
    /// Its parameters' JSON, which it may lack.
    pub(crate) params: Option<&'a str>,
    /// The session attached under the page's it's of; none for the page's own.
    pub(crate) session: Option<&'a str>,
}

/// `text`'s id, what it answers with and that object's JSON, if it's an answer, in the page's
/// own session or one attached under it.
pub(crate) fn answer(text: &str) -> Option<(i64, Answered, &str)> {
    let (id, rest) = text.strip_prefix("{\"id\":")?.split_once(',')?;
    let id = id.parse().ok()?;
    let (answered, object) = match rest.strip_prefix("\"result\":") {
        Some(object) => (Answered::Result, object),
        None => (Answered::Error, rest.strip_prefix("\"error\":")?),
    };
    Some((id, answered, dictionary(closed(object)?.0)?))
}

/// `text`'s method, its parameters' JSON and its session, if it's an event.
pub(crate) fn event(text: &str) -> Option<Event<'_>> {
    let (method, rest) = text.strip_prefix("{\"method\":\"")?.split_once('"')?;
    if method.is_empty() {
        return None;
    }
    let (rest, session) = closed(rest)?;
    let params = match rest {
        "" => None,
        rest => Some(dictionary(rest.strip_prefix(",\"params\":")?)?),
    };
    Some(Event {
        method,
        params,
        session,
    })
}

/// The end of a message, `rest`, without the brace that closes it and the session it names
/// before that, if it names one: what's left, and the session.
fn closed(rest: &str) -> Option<(&str, Option<&str>)> {
    let named = (rest.strip_suffix("\"}"))
        .and_then(|head| head.rsplit_once(",\"sessionId\":\""))
        .filter(|(_, session)| !session.is_empty() && session.chars().all(|c| c.is_ascii_alphanumeric()));
    match named {
        Some((rest, session)) => Some((rest, Some(session))),
        None => Some((rest.strip_suffix('}')?, None)),
    }
}

/// `json` if it's an object as far as its ends say.
fn dictionary(json: &str) -> Option<&str> {
    (json.starts_with('{') && json.ends_with('}')).then_some(json)
}

#[cfg(test)]
mod tests;
