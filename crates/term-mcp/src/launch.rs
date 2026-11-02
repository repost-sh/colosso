//! `app_launch`, the one app tool the stdio relay answers itself (`docs/app-control.md` 5):
//! when nothing answers on the control socket it starts Colosso, then waits up to 30 seconds
//! for the socket to answer.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use colosso_app_control::transport::Connection;
use colosso_app_control::{
    APP_LAUNCH, APP_LAUNCH_DESCRIPTION, APP_LAUNCH_TITLE, ControlError, Endpoint,
};
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Value, json};

/// How long `app_launch` waits for a started app to answer.
pub const LAUNCH_WAIT: Duration = Duration::from_secs(30);
/// How often it looks while waiting.
const POLL: Duration = Duration::from_millis(100);

/// Starts the app; an error says why it couldn't. It returns once the app is on its way,
/// not once it answers.
pub type Spawner = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

/// How the relay starts the app and knows it started.
#[derive(Clone)]
pub struct Launcher {
    endpoint: Result<Endpoint, ControlError>,
    spawn: Spawner,
    wait: Duration,
}

impl Launcher {
    /// Starts the app with `spawn` and waits for it on `endpoint`.
    pub fn new(endpoint: Result<Endpoint, ControlError>, spawn: Spawner) -> Launcher {
        Launcher {
            endpoint,
            spawn,
            wait: LAUNCH_WAIT,
        }
    }

    /// Starts this executable as the app: the bundle with `open -a` when the executable is
    /// inside a `.app`, else the executable itself with no arguments, detached, its output
    /// thrown away.
    pub fn this_app(endpoint: Result<Endpoint, ControlError>) -> Launcher {
        Launcher::new(endpoint, Arc::new(spawn_this_app))
    }

    /// Waits `wait` for the app instead of [`LAUNCH_WAIT`].
    #[must_use]
    pub fn wait(mut self, wait: Duration) -> Launcher {
        self.wait = wait;
        self
    }

    /// Starts the app unless it answers already, then waits for it to answer:
    /// `{ launched, alreadyRunning, waitedMs }`.
    pub(crate) async fn launch(&self) -> Result<Value, ControlError> {
        let endpoint = self.endpoint.clone()?;
        let answers = || Connection::connect(&endpoint).is_ok();
        if answers() {
            return Ok(json!({ "launched": false, "alreadyRunning": true, "waitedMs": 0 }));
        }
        let spawn = Arc::clone(&self.spawn);
        tokio::task::spawn_blocking(move || spawn())
            .await
            .map_err(|error| {
                ControlError::internal(format!("The relay's worker stopped: {error}"))
            })?
            .map_err(|error| ControlError::internal(format!("Can't start Colosso: {error}")))?;
        let started = Instant::now();
        loop {
            if answers() {
                let waited = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                return Ok(
                    json!({ "launched": true, "alreadyRunning": false, "waitedMs": waited }),
                );
            }
            if started.elapsed() >= self.wait {
                return Err(ControlError::timeout(format!(
                    "Colosso was started but didn't answer within {} ms.",
                    self.wait.as_millis()
                )));
            }
            tokio::time::sleep(POLL).await;
        }
    }
}

/// The tool, for `tools/list`.
pub(crate) fn tool() -> Tool {
    let object = |value: Value| match value {
        Value::Object(map) => Arc::new(map),
        _ => Arc::default(),
    };
    let input = json!({ "type": "object", "properties": {}, "additionalProperties": false });
    let output = json!({
        "type": "object",
        "properties": {
            "launched": { "type": "boolean", "description": "Whether this call started Colosso." },
            "alreadyRunning": { "type": "boolean", "description": "Whether Colosso was answering already." },
            "waitedMs": { "type": "integer", "minimum": 0, "description": "How long the call waited for Colosso to answer." }
        },
        "required": ["launched", "alreadyRunning", "waitedMs"],
        "additionalProperties": false
    });
    let annotations = ToolAnnotations::with_title(APP_LAUNCH_TITLE)
        .read_only(false)
        .destructive(false)
        .idempotent(true)
        .open_world(false);
    Tool::new(APP_LAUNCH, APP_LAUNCH_DESCRIPTION, object(input))
        .with_title(APP_LAUNCH_TITLE)
        .with_raw_output_schema(object(output))
        .with_annotations(annotations)
}

/// A one-line summary of a launch.
pub(crate) fn summary(output: &Value) -> String {
    if output["alreadyRunning"] == true {
        "Colosso is running already".to_owned()
    } else {
        format!(
            "Started Colosso; it answered after {} ms",
            output["waitedMs"]
        )
    }
}

fn spawn_this_app() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = match bundle(&executable) {
        Some(bundle) => {
            let mut open = Command::new("open");
            // Launch Services starts the app without this process's environment, so the
            // data folder it serves is passed on.
            if let Some(dir) = std::env::var_os("COLOSSO_DATA_DIR") {
                let mut pair = std::ffi::OsString::from("COLOSSO_DATA_DIR=");
                pair.push(dir);
                open.arg("--env").arg(pair);
            }
            open.arg("-a").arg(bundle);
            open
        }
        None => Command::new(executable),
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // A group of its own, so a Ctrl+C meant for the agent never reaches the app.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    // Reaped when it ends, which for the app itself is long after this relay is gone.
    let reaped = std::thread::Builder::new()
        .name("app-launch".into())
        .spawn(move || {
            let _ = child.wait();
        });
    if let Err(error) = reaped {
        log::warn!("couldn't wait for the started app: {error}");
    }
    Ok(())
}

/// The `.app` bundle holding `executable` in its `Contents/MacOS`, if it's in one.
fn bundle(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let inside = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app";
    inside.then(|| bundle.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_executable_in_a_bundle_is_opened_as_one() {
        assert_eq!(
            bundle(Path::new(
                "/Applications/Colosso.app/Contents/MacOS/colosso"
            )),
            Some(PathBuf::from("/Applications/Colosso.app"))
        );
        assert_eq!(
            bundle(Path::new("/work/colosso/target/debug/colosso")),
            None
        );
        assert_eq!(bundle(Path::new("/Contents/MacOS/colosso")), None);
    }
}
