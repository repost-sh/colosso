//! The session holder's requests (`docs/agent-desktop.md` 6): JSON lines on its socket, one
//! answer per request, in order. The holder is `koyo desk-holder`, a daemon of the person's.

use serde::{Deserialize, Serialize};

use crate::layout::Size;

/// What the app asks the holder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ask", rename_all = "camelCase")]
pub enum HolderAsk {
    Status,
    /// Resize the agent screen; answered once the session's display says it's that size, or
    /// with the size it kept.
    Size {
        width: u32,
        height: u32,
    },
    /// The app is there, with `areas` areas open: sent every 30 s. Without one for two minutes,
    /// or with no area for ten, the holder ends the session (`docs/agent-desktop.md` 6).
    Keep {
        areas: u32,
    },
    /// End the session (the helper logs the account out) and stop.
    End,
}

/// Where the session is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SessionState {
    /// Logging in.
    Starting,
    /// The connection is held; the session runs.
    Held { since_ms: u64 },
    /// It couldn't log in, or lost the session; `message` says why.
    Failed { message: String },
    /// Ended as asked.
    Ended,
}

/// The holder's answer to every request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HolderStatus {
    #[serde(flatten)]
    pub state: SessionState,
    /// The agent screen as the session last reported it.
    pub screen: Option<Size>,
    /// The holder's build, so the app can tell an older one to make way.
    pub version: String,
    /// What a request couldn't do, when it couldn't.
    pub error: Option<String>,
}

/// Sends `ask` as one line on `stream` and reads one line back as the answer: the exchange the
/// app has with the holder and the holder with the helper.
///
/// # Errors
/// When the stream fails, or the answer isn't one.
pub fn exchange<Q: Serialize, A: serde::de::DeserializeOwned>(
    stream: &mut std::os::unix::net::UnixStream,
    ask: &Q,
) -> std::io::Result<A> {
    use std::io::{BufRead as _, Write as _};
    stream.write_all(line(ask).map_err(std::io::Error::other)?.as_bytes())?;
    let mut answer = String::new();
    std::io::BufReader::new(stream.try_clone()?).read_line(&mut answer)?;
    serde_json::from_str(&answer)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// One line of JSON, newline-terminated.
///
/// # Errors
/// When the value can't be written as JSON.
pub fn line<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let mut text = serde_json::to_string(value)?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_read_as_their_lines() {
        let ask: HolderAsk =
            serde_json::from_str(r#"{"ask":"size","width":3360,"height":1050}"#).unwrap();
        assert_eq!(
            ask,
            HolderAsk::Size {
                width: 3360,
                height: 1050
            }
        );
        assert_eq!(line(&HolderAsk::End).unwrap(), "{\"ask\":\"end\"}\n");
    }

    #[test]
    fn a_status_round_trips() {
        let status = HolderStatus {
            state: SessionState::Held { since_ms: 5 },
            screen: Some(Size {
                width: 1680,
                height: 1050,
            }),
            version: "0.1.0".into(),
            error: None,
        };
        let text = line(&status).unwrap();
        assert!(text.contains("\"state\":\"held\""), "{text}");
        assert_eq!(serde_json::from_str::<HolderStatus>(&text).unwrap(), status);
    }
}
