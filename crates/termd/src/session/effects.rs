//! What the engine reports as it parses the program's output, acted on by the session: query
//! replies back to the program when no client answers them, the title and working directory
//! into the record, bells and notifications out to the clients.

use colosso_term_control::EventKind;
use colosso_term_engine::Effect;

use super::Session;

impl Session {
    pub(super) fn effects(&mut self) {
        let answers = self
            .clients
            .values()
            .any(|attached| attached.client.answers_queries());
        for effect in self.engine.take_effects() {
            match effect {
                Effect::WriteBack(bytes) => {
                    if !answers {
                        self.write_pty(&bytes);
                    }
                }
                Effect::Title(title) => {
                    self.record.title = Some(title.clone());
                    self.publish(false);
                    self.daemon
                        .broadcast(&self.record.handle, &EventKind::Title { title });
                }
                Effect::Pwd(uri) => {
                    if let Some(path) = file_uri_path(&uri) {
                        self.record.cwd.clone_from(&path);
                        let outside = self.record.outside_worktree();
                        self.publish(false);
                        self.daemon.broadcast(
                            &self.record.handle,
                            &EventKind::Cwd {
                                cwd: path,
                                outside_worktree: outside,
                            },
                        );
                    }
                }
                Effect::Bell => {
                    self.status.bell(std::time::Instant::now());
                    self.daemon.broadcast(&self.record.handle, &EventKind::Bell);
                }
                Effect::Notification { title, body } => {
                    self.status.notification(&title, &body);
                    self.daemon.broadcast(
                        &self.record.handle,
                        &EventKind::Notification { title, body },
                    );
                }
                Effect::Progress(progress) => {
                    self.record.progress = match progress {
                        colosso_term_engine::Progress::Remove => None,
                        colosso_term_engine::Progress::Set(pct) => {
                            Some(colosso_term_control::Progress::Percent(pct))
                        }
                        colosso_term_engine::Progress::Error(pct) => {
                            Some(colosso_term_control::Progress::Error(pct))
                        }
                        colosso_term_engine::Progress::Indeterminate => {
                            Some(colosso_term_control::Progress::Indeterminate)
                        }
                        colosso_term_engine::Progress::Paused(pct) => {
                            Some(colosso_term_control::Progress::Paused(pct))
                        }
                    };
                    self.publish(false);
                }
                Effect::Clipboard(_) | Effect::RenderHold(_) => {}
            }
        }
    }
}

/// `file://host/path` to the path, percent-decoded; or zsh's `kitty-shell-cwd://host/path`,
/// which Ghostty's integration sends as it is.
fn file_uri_path(uri: &str) -> Option<std::path::PathBuf> {
    if let Some(rest) = uri.strip_prefix("kitty-shell-cwd://") {
        return Some(rest[rest.find('/')?..].into());
    }
    let rest = uri.strip_prefix("file://")?;
    let path = &rest[rest.find('/')?..];
    let mut bytes = Vec::with_capacity(path.len());
    let mut chars = path.bytes();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let hex: Vec<u8> = chars.by_ref().take(2).collect();
            if let Ok(text) = std::str::from_utf8(&hex)
                && let Ok(value) = u8::from_str_radix(text, 16)
            {
                bytes.push(value);
                continue;
            }
            bytes.push(b'%');
            bytes.extend(hex);
        } else {
            bytes.push(byte);
        }
    }
    Some(std::path::PathBuf::from(
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

pub(super) fn signal_number(name: &str) -> i32 {
    match name {
        "SIGHUP" => 1,
        "SIGINT" => 2,
        "SIGQUIT" => 3,
        "SIGABRT" => 6,
        "SIGKILL" => 9,
        "SIGSEGV" => 11,
        "SIGPIPE" => 13,
        "SIGALRM" => 14,
        "SIGTERM" => 15,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            file_uri_path("file://mac.local/Users/x/a%20b"),
            Some("/Users/x/a b".into())
        );
        assert_eq!(file_uri_path("file:///tmp"), Some("/tmp".into()));
        assert_eq!(
            file_uri_path("kitty-shell-cwd://mac.local/Users/x/a b"),
            Some("/Users/x/a b".into())
        );
        assert_eq!(file_uri_path("https://x"), None);
    }
}
