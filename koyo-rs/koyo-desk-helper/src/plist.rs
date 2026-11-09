//! The two launchd plists the helper writes: its own daemon's and the host's launch agent in
//! the account (`docs/agent-desktop.md` 5.1, 7.1).

use std::fmt::Write as _;

use koyo_desk_wire::paths;

/// A plist value, as much of one as launchd needs.
pub enum Value {
    Str(String),
    Bool(bool),
    Int(i64),
    Array(Vec<Value>),
    Dict(Vec<(&'static str, Value)>),
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// Writing to a `String` can't fail, so `writeln!`'s result is dropped throughout.
fn write(value: &Value, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    match value {
        Value::Str(text) => {
            let _ = writeln!(out, "{pad}<string>{}</string>", escape(text));
        }
        Value::Bool(on) => {
            let _ = writeln!(out, "{pad}<{on}/>");
        }
        Value::Int(number) => {
            let _ = writeln!(out, "{pad}<integer>{number}</integer>");
        }
        Value::Array(items) => {
            let _ = writeln!(out, "{pad}<array>");
            for item in items {
                write(item, indent + 1, out);
            }
            let _ = writeln!(out, "{pad}</array>");
        }
        Value::Dict(entries) => {
            let _ = writeln!(out, "{pad}<dict>");
            for (key, item) in entries {
                let _ = writeln!(out, "{pad}  <key>{}</key>", escape(key));
                write(item, indent + 1, out);
            }
            let _ = writeln!(out, "{pad}</dict>");
        }
    }
}

/// `root` as a plist document.
pub fn document(root: &Value) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n",
    );
    write(root, 0, &mut out);
    out.push_str("</plist>\n");
    out
}

fn strings(items: &[&str]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|item| Value::Str((*item).to_owned()))
            .collect(),
    )
}

/// A job launchd starts at load and keeps alive, restarted no sooner than `throttle` seconds
/// apart, its output in `log`, with `extra` keys after.
fn job(
    label: &str,
    arguments: &[&str],
    throttle: i64,
    log: &str,
    extra: Vec<(&'static str, Value)>,
) -> String {
    let mut entries = vec![
        ("Label", Value::Str(label.into())),
        ("ProgramArguments", strings(arguments)),
        ("RunAtLoad", Value::Bool(true)),
        ("KeepAlive", Value::Bool(true)),
        ("ThrottleInterval", Value::Int(throttle)),
        ("StandardOutPath", Value::Str(log.into())),
        ("StandardErrorPath", Value::Str(log.into())),
    ];
    entries.extend(extra);
    document(&Value::Dict(entries))
}

/// The helper's daemon: `serve`, kept alive, its output in the shared folder's log.
pub fn daemon() -> String {
    let log = format!("{}/helper.log", paths::SHARED);
    job(
        paths::HELPER_LABEL,
        &[paths::HELPER_BIN, "serve"],
        10,
        &log,
        Vec::new(),
    )
}

/// The host's launch agent in the account's GUI session: the installed loader, told the
/// app's link socket, kept alive, its log in the account's own Library.
pub fn host_agent(home: &str) -> String {
    let executable = format!(
        "{}/{}/{}",
        paths::HOST_DIR,
        paths::HOST_APP,
        paths::HOST_EXECUTABLE
    );
    let log = format!("{home}/Library/Logs/Koyo Agent Desktop/host.log");
    job(
        paths::HOST_LABEL,
        &[&executable, "--link", paths::LINK_SOCKET],
        5,
        &log,
        vec![
            ("LimitLoadToSessionType", Value::Str("Aqua".into())),
            ("ProcessType", Value::Str("Interactive".into())),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_agent_runs_the_installed_loader_in_the_gui_session() {
        let plist = host_agent("/Users/koyo-agents");
        assert!(plist.contains("<string>/Library/Application Support/Koyo/Agent Desktop/host/Koyo Agent Desktop.app/Contents/MacOS/Koyo Agent Desktop</string>"), "{plist}");
        assert!(plist.contains("<key>LimitLoadToSessionType</key>\n  <string>Aqua</string>"));
        assert!(plist.contains("/Users/koyo-agents/Library/Logs/Koyo Agent Desktop/host.log"));
    }

    #[test]
    fn text_is_escaped() {
        let plist = document(&Value::Dict(vec![("A", Value::Str("a<b>&\"c\"".into()))]));
        assert!(
            plist.contains("<string>a&lt;b&gt;&amp;&quot;c&quot;</string>"),
            "{plist}"
        );
    }

    #[test]
    fn the_daemon_serves_from_its_installed_copy() {
        let plist = daemon();
        assert!(plist.contains("<string>/Library/PrivilegedHelperTools/dev.koyo.agent-desktop.helper</string>\n    <string>serve</string>"), "{plist}");
    }
}
