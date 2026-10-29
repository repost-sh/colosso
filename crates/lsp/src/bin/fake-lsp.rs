//! A language server that does what it's told, for the client's tests. Its one argument is a
//! script: a JSON array of steps, each an object keyed by what it does.
//!
//! - `{"recv": method}`: the next message must be a request or notification for `method`. A
//!   request's id is remembered, in order, for `reply`.
//! - `{"echo": method}`: as `recv`, then sends the message back as a `fake/received`
//!   notification, so the test sees what the client sent.
//! - `{"echo_reply": true}`: the next message must be an answer; it's sent back the same way.
//! - `{"expect": message}`: the next message must hold `message`: each of its fields, at any
//!   depth, the same in the one received, which may have more. Remembered as `recv` does.
//! - `{"reply": n, "result": value}`, `{"reply": n, "error": error}`: answers the `n`th
//!   remembered request.
//! - `{"send": message}`: sends `message`, `jsonrpc` added.
//! - `{"bytewise": message}`: sends it one byte per write, flushing each.
//! - `{"merged": [message, …]}`: sends them all in a single write.
//! - `{"raw": text}`: writes `text` as it is, framing and all.
//! - `{"stderr": line}`: writes `line` to stderr.
//! - `{"exit": code}`: exits with `code`.
//! - `{"hang": true}`: stops, reading nothing and never exiting.
//!
//! After the script it answers `shutdown`, and exits on `exit` or at the end of its input.
//! A message other than the script expects ends it with code 2, saying why on stderr.

use std::io::{self, BufRead, Read, StdinLock, StdoutLock, Write};
use std::process::ExitCode;

use serde_json::{Value, json};

fn main() -> ExitCode {
    let Some(script) = std::env::args().nth(1) else {
        return failed("usage: fake-lsp <script>");
    };
    let steps: Vec<Value> = match serde_json::from_str(&script) {
        Ok(steps) => steps,
        Err(error) => return failed(&format!("unreadable script: {error}")),
    };
    let mut fake = Fake {
        input: io::stdin().lock(),
        output: io::stdout().lock(),
        ids: Vec::new(),
    };
    for step in &steps {
        match fake.step(step) {
            Ok(None) => {}
            Ok(Some(code)) => return ExitCode::from(code),
            Err(message) => return failed(&message),
        }
    }
    fake.serve()
}

struct Fake {
    input: StdinLock<'static>,
    output: StdoutLock<'static>,
    /// The ids of the requests received, in order.
    ids: Vec<Value>,
}

impl Fake {
    /// Runs one step; `Some` is the code to exit with.
    fn step(&mut self, step: &Value) -> Result<Option<u8>, String> {
        if let Some(index) = step.get("reply") {
            let id = index
                .as_u64()
                .and_then(|index| self.ids.get(index as usize))
                .ok_or_else(|| format!("no request {index} to answer"))?
                .clone();
            let answer = match step.get("error") {
                Some(error) => json!({"id": id, "error": error}),
                None => {
                    json!({"id": id, "result": step.get("result").cloned().unwrap_or(Value::Null)})
                }
            };
            return self.send(&answer).map(|()| None);
        }
        let (key, value) = step
            .as_object()
            .and_then(|object| object.iter().next())
            .ok_or_else(|| format!("a step that's not an object: {step}"))?;
        match key.as_str() {
            "recv" | "echo" => {
                let message = self.next()?;
                let method = message.get("method").and_then(Value::as_str);
                if method != value.as_str() {
                    return Err(format!("expected {value}, received {message}"));
                }
                if let Some(id) = message.get("id") {
                    self.ids.push(id.clone());
                }
                if key == "echo" {
                    self.send(&json!({"method": "fake/received", "params": message}))?;
                }
            }
            "expect" => {
                let message = self.next()?;
                if !holds(&message, value) {
                    return Err(format!("expected {value}, received {message}"));
                }
                if let Some(id) = message.get("id") {
                    self.ids.push(id.clone());
                }
            }
            "echo_reply" => {
                let message = self.next()?;
                if message.get("method").is_some() {
                    return Err(format!("expected an answer, received {message}"));
                }
                self.send(&json!({"method": "fake/received", "params": message}))?;
            }
            "send" => self.send(value)?,
            "bytewise" => {
                for byte in frame(value) {
                    self.write(&[byte])?;
                }
            }
            "merged" => {
                let messages = value.as_array().ok_or("merged takes an array")?;
                self.write(&messages.iter().flat_map(frame).collect::<Vec<_>>())?;
            }
            "raw" => self.write(value.as_str().unwrap_or_default().as_bytes())?,
            "stderr" => {
                let line = value.as_str().unwrap_or_default();
                writeln!(io::stderr(), "{line}").map_err(|error| error.to_string())?;
            }
            "exit" => return Ok(Some(value.as_u64().unwrap_or(0) as u8)),
            "hang" => loop {
                std::thread::park();
            },
            _ => return Err(format!("an unknown step: {step}")),
        }
        Ok(None)
    }

    /// After the script: answers `shutdown`, and exits on `exit` or the end of the input.
    fn serve(mut self) -> ExitCode {
        loop {
            let message = match self.read() {
                Ok(Some(message)) => message,
                Ok(None) => return ExitCode::SUCCESS,
                Err(message) => return failed(&message),
            };
            match message.get("method").and_then(Value::as_str) {
                Some("shutdown") => {
                    let answer = json!({"id": message.get("id"), "result": null});
                    if let Err(message) = self.send(&answer) {
                        return failed(&message);
                    }
                }
                Some("exit") => return ExitCode::SUCCESS,
                _ => {}
            }
        }
    }

    fn next(&mut self) -> Result<Value, String> {
        self.read()?.ok_or_else(|| "the input ended".to_owned())
    }

    /// The next message, or `None` at the end of the input.
    fn read(&mut self) -> Result<Option<Value>, String> {
        let mut length = None;
        let mut line = String::new();
        loop {
            line.clear();
            if self
                .input
                .read_line(&mut line)
                .map_err(|error| error.to_string())?
                == 0
            {
                return Ok(None);
            }
            let header = line.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length:") {
                length = value.trim().parse::<usize>().ok();
            }
        }
        let length = length.ok_or("a message without a Content-Length")?;
        let mut body = vec![0; length];
        self.input
            .read_exact(&mut body)
            .map_err(|error| error.to_string())?;
        serde_json::from_slice(&body)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        self.write(&frame(message))
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.output
            .write_all(bytes)
            .and_then(|()| self.output.flush())
            .map_err(|error| error.to_string())
    }
}

/// Whether `value` has each of `expected`'s fields, the same, at any depth.
fn holds(value: &Value, expected: &Value) -> bool {
    match (value, expected) {
        (Value::Object(value), Value::Object(expected)) => expected
            .iter()
            .all(|(key, field)| value.get(key).is_some_and(|got| holds(got, field))),
        _ => value == expected,
    }
}

/// A message framed for the wire, `jsonrpc` added.
fn frame(message: &Value) -> Vec<u8> {
    let mut message = message.clone();
    if let Some(object) = message.as_object_mut() {
        object.insert("jsonrpc".to_owned(), json!("2.0"));
    }
    let body = message.to_string();
    format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

fn failed(message: &str) -> ExitCode {
    let _ = writeln!(io::stderr(), "fake-lsp: {message}");
    ExitCode::from(2)
}
