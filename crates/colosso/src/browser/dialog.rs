//! Questions a page asks the person, in the app's dialogs over the window: alerts,
//! confirms, prompts, leaving a page, sign-ins, permissions and certificate warnings; file
//! choosers in the system's panels. The page waits until the person answers; one question
//! is asked at a time, the rest wait their turn, and a dialog put away without an answer
//! answers no.

use std::path::PathBuf;

use colosso_browser_ipc::{Answer, AppRequest, Dialog, FileMode, Permission};
use gpui_kit::{Context, PathPromptOptions, Window};

use super::view::BrowserView;
use crate::modal;
use window::PageDialog;

mod window;

/// A question the page waits on.
pub(super) struct Question {
    pub id: u64,
    pub origin: String,
    pub asked: Dialog,
}

const WIDTH: f32 = 420.;
/// A dialog's text runs so many characters a line.
const CHARS_PER_LINE: usize = 52;
const MOST_LINES: usize = 8;
/// A field and the `mt_2` over it.
const FIELD: f32 = modal::FIELD + 8.;

impl BrowserView {
    /// The page asks something. File choosers open the system panel; every other question
    /// waits for the dialog.
    pub(super) fn dialog_opened(
        &mut self,
        id: u64,
        origin: String,
        question: Dialog,
        cx: &mut Context<Self>,
    ) {
        if let Dialog::File {
            mode, default_path, ..
        } = &question
        {
            self.choose_files(id, *mode, default_path, cx);
            return;
        }
        self.dialogs.push_back(Question {
            id,
            origin,
            asked: question,
        });
        cx.notify();
    }

    /// The host withdrew a question: the page navigated away, or the engine answered it
    /// itself.
    pub(super) fn dialog_closed(&mut self, id: u64, cx: &mut Context<Self>) {
        self.dialogs.retain(|question| question.id != id);
        if self.asked.take_if(|asked| *asked == id).is_some() {
            modal::close(cx);
        }
        cx.notify();
    }

    /// Asks the first waiting question, unless one is on screen. The dialog opens once the
    /// render is over.
    pub(super) fn ask_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.asked.is_some() {
            return;
        }
        let Some(question) = self.dialogs.front() else {
            return;
        };
        let id = question.id;
        let words = words(&question.asked, &question.origin);
        let fields = words.carries.fields();
        self.asked = Some(id);
        let view = cx.weak_entity();
        let heading = modal::heading_height(&words.heading.clone().into(), WIDTH, window);
        let height = height(&words, heading, fields.len());
        window.defer(cx, move |window, cx| {
            modal::open(WIDTH, height, window, cx, |window, cx| {
                PageDialog::new(view, id, words, fields, window, cx)
            });
        });
    }

    /// The person answered `id`, or its dialog went without an answer.
    pub(super) fn answered(&mut self, id: u64, answer: Answer, cx: &mut Context<Self>) {
        if self.asked == Some(id) {
            self.asked = None;
        }
        if let Some(at) = self.dialogs.iter().position(|question| question.id == id) {
            self.dialogs.remove(at);
            self.send_answer(id, answer, cx);
        }
        cx.notify();
    }

    fn send_answer(&self, dialog: u64, answer: Answer, cx: &mut Context<Self>) {
        let pane = self.pane;
        self.send_to_page(
            AppRequest::Answer {
                pane,
                dialog,
                answer,
            },
            cx,
        );
    }

    /// The system's open or save panel, answered with what the person picks.
    fn choose_files(&self, id: u64, mode: FileMode, default_path: &str, cx: &mut Context<Self>) {
        if mode == FileMode::Save {
            let suggested = PathBuf::from(default_path);
            let directory = suggested
                .parent()
                .filter(|parent| suggested.is_absolute() && parent.is_dir())
                .map(PathBuf::from)
                .or_else(dirs::download_dir)
                .unwrap_or_default();
            let name = suggested
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            let panel = cx.prompt_for_new_path(&directory, name.as_deref());
            cx.spawn(async move |this, cx| {
                let path = panel.await.ok().and_then(Result::ok).flatten();
                let _ = this.update(cx, |this, cx| {
                    this.send_answer(id, files(path.into_iter().collect()), cx);
                });
            })
            .detach();
            return;
        }
        let panel = cx.prompt_for_paths(PathPromptOptions {
            files: mode != FileMode::Folder,
            directories: mode == FileMode::Folder,
            multiple: mode == FileMode::OpenMultiple,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let paths = panel
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .unwrap_or_default();
            let _ = this.update(cx, |this, cx| this.send_answer(id, files(paths), cx));
        })
        .detach();
    }
}

/// The paths picked in a file panel; none is a cancel.
fn files(paths: Vec<PathBuf>) -> Answer {
    if paths.is_empty() {
        Answer::Dismiss
    } else {
        Answer::Files {
            paths: paths
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        }
    }
}

/// What a question's dialog says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Words {
    heading: String,
    text: Option<String>,
    /// The label of the button that says no, if the question takes a no.
    cancel: Option<&'static str>,
    confirm: &'static str,
    /// What Return chooses. An untrusted certificate defaults to going back.
    accept_default: bool,
    /// What a yes carries: the prompt's text, the sign-in's credentials, or nothing.
    carries: Carries,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Carries {
    Nothing,
    /// What the field starts with.
    Text {
        default: String,
    },
    Credentials,
}

impl Carries {
    /// The dialog's fields for it: each one's label, its first text, and whether it hides it.
    fn fields(&self) -> Vec<(&'static str, String, bool)> {
        match self {
            Carries::Nothing => Vec::new(),
            Carries::Text { default } => vec![("Answer", default.clone(), false)],
            Carries::Credentials => vec![
                ("User name", String::new(), false),
                ("Password", String::new(), true),
            ],
        }
    }
}

/// How high the dialog for `words`, under a `heading` this high, with `fields` fields is.
fn height(words: &Words, heading: f32, fields: usize) -> f32 {
    let lines = words.text.as_ref().map_or(0, |text| {
        (text.len() / CHARS_PER_LINE + 1 + text.matches('\n').count()).min(MOST_LINES)
    });
    2. * modal::PADDING
        + heading
        + 8.
        + lines as f32 * modal::LINE
        + fields as f32 * FIELD
        + 12.
        + modal::FOOTER
}

fn words(question: &Dialog, origin: &str) -> Words {
    let who = who(origin);
    let says = |message: &str, cancel, carries| Words {
        heading: format!("{who} says"),
        text: Some(message.to_owned()),
        cancel,
        confirm: "OK",
        accept_default: true,
        carries,
    };
    match question {
        Dialog::Alert { message } => says(message, None, Carries::Nothing),
        Dialog::Confirm { message } => says(message, Some("Cancel"), Carries::Nothing),
        Dialog::Prompt {
            message,
            default_text,
        } => {
            let carries = Carries::Text {
                default: default_text.clone(),
            };
            says(message, Some("Cancel"), carries)
        }
        Dialog::BeforeUnload { reload } => Words {
            heading: if *reload {
                "Reload site?"
            } else {
                "Leave site?"
            }
            .into(),
            text: Some("Changes you made may not be saved.".into()),
            cancel: Some("Cancel"),
            confirm: if *reload { "Reload" } else { "Leave" },
            accept_default: true,
            carries: Carries::Nothing,
        },
        Dialog::Auth { host, realm, proxy } => {
            let mut text = if *proxy {
                format!("The proxy {host} asks for a user name and password.")
            } else {
                format!("{host} asks for a user name and password.")
            };
            if !realm.is_empty() {
                use std::fmt::Write as _;
                let _ = write!(text, " It says: “{realm}”.");
            }
            if origin.starts_with("http://") {
                text.push_str(" The connection is not private.");
            }
            Words {
                heading: "Sign in".into(),
                text: Some(text),
                cancel: Some("Cancel"),
                confirm: "Sign in",
                accept_default: true,
                carries: Carries::Credentials,
            }
        }
        Dialog::Permission { permissions } => Words {
            heading: format!("{who} wants to"),
            text: Some(
                permissions
                    .iter()
                    .map(|permission| permission_label(*permission))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            cancel: Some("Block"),
            confirm: "Allow",
            accept_default: true,
            carries: Carries::Nothing,
        },
        Dialog::Certificate { error } => Words {
            heading: "Your connection is not private".into(),
            text: Some(format!(
                "{error} Someone may be pretending to be {who}. Continue only if you trust \
                 this site, such as a local server you run."
            )),
            cancel: Some("Back to safety"),
            confirm: "Continue anyway",
            accept_default: false,
            carries: Carries::Nothing,
        },
        // File choosers use the system panel and never show a dialog.
        Dialog::File { .. } => Words {
            heading: "Choose a file".into(),
            text: None,
            cancel: Some("Cancel"),
            confirm: "Choose",
            accept_default: true,
            carries: Carries::Nothing,
        },
    }
}

/// Who asks, for the person: the site's host, or "This page" when the page has no
/// address of its own.
fn who(origin: &str) -> String {
    match origin.split_once("://") {
        Some(("http" | "https", host)) if !host.is_empty() => host.to_owned(),
        Some(("file", _)) => "A local file".into(),
        _ => "This page".into(),
    }
}

fn permission_label(permission: Permission) -> &'static str {
    match permission {
        Permission::Camera => "Use your camera",
        Permission::Microphone => "Use your microphone",
        Permission::Screen => "See your screen",
        Permission::Location => "Know your location",
        Permission::Notifications => "Show notifications",
        Permission::Clipboard => "See text and images copied to the clipboard",
        Permission::Midi => "Use your MIDI devices",
        Permission::Sensors => "Use your motion and light sensors",
        Permission::Fonts => "Use the fonts on your computer",
        Permission::Windows => "Place windows on all your displays",
        Permission::LocalNetwork => "Find and connect to devices on your local network",
        Permission::Files => "Edit files and folders on your computer",
        Permission::Downloads => "Download several files",
        Permission::Other => "Use another capability of your computer",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_names_who_asks() {
        assert_eq!(who("https://example.com:8443"), "example.com:8443");
        assert_eq!(who("http://localhost:3000"), "localhost:3000");
        assert_eq!(who("data:text/html"), "This page");
        assert_eq!(who(""), "This page");
        assert_eq!(who("file://"), "A local file");
    }

    #[test]
    fn certificates_default_to_safety_and_sign_ins_say_when_they_are_not_private() {
        let auth = words(
            &Dialog::Auth {
                host: "intranet:8080".into(),
                realm: "Staff".into(),
                proxy: false,
            },
            "http://intranet:8080",
        );
        assert_eq!(
            auth.text.as_deref(),
            Some(
                "intranet:8080 asks for a user name and password. It says: “Staff”. The connection is not private."
            )
        );
        assert_eq!((auth.cancel, auth.confirm), (Some("Cancel"), "Sign in"));
        assert_eq!(auth.carries, Carries::Credentials);
        let certificate = words(
            &Dialog::Certificate {
                error: "The certificate is self-signed.".into(),
            },
            "https://localhost:8443",
        );
        assert_eq!(certificate.heading, "Your connection is not private");
        assert!(!certificate.accept_default);
        assert!(height(&certificate, 24., 0) > height(&auth, 24., 0) - 2. * FIELD);
    }

    #[test]
    fn answers_carry_the_files_picked() {
        assert_eq!(files(Vec::new()), Answer::Dismiss);
        assert_eq!(
            files(vec![PathBuf::from("/tmp/a.png")]),
            Answer::Files {
                paths: vec!["/tmp/a.png".into()]
            }
        );
    }
}
