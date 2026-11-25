//! Questions a page asks the person: JavaScript dialogs, sign-ins,
//! permissions, certificate errors and file choosers.
//!
//! CEF hands each one a callback and waits. The host keeps the callback
//! under a number, asks the app with [`HostEvent::Dialog`], and resolves it
//! when [`AppRequest::Answer`] comes back. Questions arrive on CEF's UI and
//! IO threads, so the waiting callbacks sit behind a lock that is never held
//! across a call into CEF.
//!
//! [`AppRequest::Answer`]: koyo_browser_ipc::AppRequest::Answer
use crate::host;
use crate::pending::Pending;
use cef::{
    AuthCallback, Callback, CefStringList, Errorcode, FileDialogCallback, FileDialogMode,
    ImplAuthCallback, ImplCallback, ImplFileDialogCallback, ImplJsdialogCallback,
    ImplMediaAccessCallback, ImplPermissionPromptCallback, JsdialogCallback, MediaAccessCallback,
    PermissionPromptCallback, PermissionRequestResult,
};
use koyo_browser_ipc::{Answer, Dialog, FileMode, HostEvent, PaneId, Permission};
use std::sync::Mutex;

/// A callback CEF waits on.
pub(crate) enum Waiting {
    /// `alert`, `confirm` and `prompt`.
    Script(JsdialogCallback),
    /// Whether to leave the page or reload it, as its `beforeunload` asks.
    BeforeUnload(JsdialogCallback),
    /// A sign-in.
    Auth(AuthCallback),
    /// A permission prompt, with CEF's id for it.
    Prompt {
        id: u64,
        /// Whether the page asks to know where the person is.
        location: bool,
        callback: PermissionPromptCallback,
    },
    /// Camera, microphone or screen, with the bits asked for and who asks.
    Media {
        bits: u32,
        origin: String,
        callback: MediaAccessCallback,
    },
    /// An untrusted certificate.
    Certificate(Callback),
    /// A file chooser.
    File(FileDialogCallback),
}

// SAFETY: CEF's callbacks are reference counted atomically and may be
// resolved from any thread; the host resolves them on the UI thread.
unsafe impl Send for Waiting {}

/// What kind of question a [`Waiting`] callback is, which an answer has to fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
    Script,
    Auth,
    Prompt,
    Media,
    Certificate,
    File,
}

impl Waiting {
    fn asked(&self) -> Asked {
        match self {
            Waiting::Script(_) | Waiting::BeforeUnload(_) => Asked::Script,
            Waiting::Auth(_) => Asked::Auth,
            Waiting::Prompt { .. } => Asked::Prompt,
            Waiting::Media { .. } => Asked::Media,
            Waiting::Certificate(_) => Asked::Certificate,
            Waiting::File(_) => Asked::File,
        }
    }
}

/// Whether `answer` answers a question of kind `asked`: no answers any, and each other answer
/// only its own kind, so files never allow a camera or a certificate, whatever question
/// number they come back with.
fn fits(asked: Asked, answer: &Answer) -> bool {
    matches!(
        (asked, answer),
        (_, Answer::Dismiss)
            | (Asked::Script, Answer::Accept | Answer::Text { .. })
            | (Asked::Auth, Answer::Credentials { .. })
            | (Asked::Prompt | Asked::Media | Asked::Certificate, Answer::Accept)
            | (Asked::File, Answer::Files { .. })
    )
}

static DIALOGS: Mutex<Pending<Waiting>> = Mutex::new(Pending::new());

fn with<R>(f: impl FnOnce(&mut Pending<Waiting>) -> R) -> R {
    f(&mut DIALOGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner))
}

/// Asks the app, keeping `waiting` until it answers.
pub(crate) fn ask(pane: PaneId, origin: &str, question: Dialog, waiting: Waiting) {
    let dialog = with(|dialogs| dialogs.add(pane, waiting));
    host::send(&HostEvent::Dialog {
        pane,
        dialog,
        origin: origin_of(origin),
        question,
    });
}

/// The person answered: an answer that doesn't fit the question says no.
pub(crate) fn answer(pane: PaneId, dialog: u64, answer: Answer) {
    if let Some(waiting) = with(|dialogs| dialogs.remove(pane, dialog)) {
        let answer = if fits(waiting.asked(), &answer) {
            answer
        } else {
            log::warn!("pane {pane}'s question {dialog} was answered as another kind; said no");
            Answer::Dismiss
        };
        if let Waiting::Media { bits, origin, .. } = &waiting
            && !matches!(answer, Answer::Dismiss)
        {
            host::keep_media(pane, origin, *bits);
        }
        if matches!((&waiting, &answer), (Waiting::BeforeUnload(_), Answer::Dismiss)) {
            host::answer_leave(pane, false);
        }
        resolve(waiting, answer);
    }
}

/// The app let `pane`'s page go: whether to leave it, asked still, is answered yes, as nobody
/// is left to answer it, and the app hears the question is withdrawn.
pub(crate) fn let_go(pane: PaneId) {
    for waiting in withdraw(pane, |waiting| matches!(waiting, Waiting::BeforeUnload(_))) {
        resolve(waiting, Answer::Accept);
    }
}

/// Whether `pane`'s page asks the person whether to leave it.
pub(crate) fn asks_to_leave(pane: PaneId) -> bool {
    with(|dialogs| dialogs.any(|p, waiting| p == pane && matches!(waiting, Waiting::BeforeUnload(_))))
}

/// The page navigated or reloaded: its script dialogs are void.
pub(crate) fn reset(pane: PaneId) {
    let withdrawn = withdraw(pane, |waiting| matches!(waiting, Waiting::Script(_) | Waiting::BeforeUnload(_)));
    if withdrawn.iter().any(|waiting| matches!(waiting, Waiting::BeforeUnload(_))) {
        host::answer_leave(pane, false);
    }
}

/// The engine took a permission prompt back.
pub(crate) fn prompt_dismissed(pane: PaneId, prompt: u64) {
    drop(withdraw(
        pane,
        |waiting| matches!(waiting, Waiting::Prompt { id, .. } if *id == prompt),
    ));
}

/// The pane closed: everything it was asked is declined.
pub(crate) fn pane_closed(pane: PaneId) {
    for (_, waiting) in with(|dialogs| dialogs.take(|p, _| p == pane)) {
        decline(waiting);
    }
}

/// The host is shutting down; CEF waits for every callback to go.
pub(crate) fn clear() {
    for (_, waiting) in with(|dialogs| dialogs.take(|_, _| true)) {
        decline(waiting);
    }
}

/// Takes back `pane`'s questions that are `which`, which the app hears.
fn withdraw(pane: PaneId, which: impl Fn(&Waiting) -> bool) -> Vec<Waiting> {
    let withdrawn = with(|dialogs| dialogs.take(|p, waiting| p == pane && which(waiting)));
    let taken = withdrawn.into_iter().map(|(dialog, waiting)| {
        host::send(&HostEvent::DialogClosed { pane, dialog });
        waiting
    });
    taken.collect()
}

fn decline(waiting: Waiting) {
    resolve(waiting, Answer::Dismiss);
}

fn resolve(waiting: Waiting, answer: Answer) {
    let accepted = !matches!(answer, Answer::Dismiss);
    match waiting {
        Waiting::Script(callback) | Waiting::BeforeUnload(callback) => {
            let text = match &answer {
                Answer::Text { text } => Some(text.as_str().into()),
                _ => None,
            };
            callback.cont(i32::from(accepted), text.as_ref());
        }
        Waiting::Auth(callback) => match answer {
            Answer::Credentials { username, password } => {
                callback.cont(
                    Some(&username.as_str().into()),
                    Some(&password.as_str().into()),
                );
            }
            _ => callback.cancel(),
        },
        Waiting::Prompt {
            location, callback, ..
        } => {
            callback.cont(if accepted {
                PermissionRequestResult::ACCEPT
            } else {
                PermissionRequestResult::DISMISS
            });
            if accepted && location {
                crate::location::allowed();
            }
        }
        Waiting::Media { bits, callback, .. } => {
            if accepted {
                callback.cont(bits);
            } else {
                callback.cancel();
            }
        }
        Waiting::Certificate(callback) => {
            if accepted {
                callback.cont();
            } else {
                callback.cancel();
            }
        }
        Waiting::File(callback) => match answer {
            Answer::Files { paths } if !paths.is_empty() => {
                let mut list = CefStringList::new();
                for path in &paths {
                    list.append(path);
                }
                callback.cont(Some(&mut list));
            }
            _ => callback.cancel(),
        },
    }
}

/// Who asks, as the person should read it: the scheme, host and port of an
/// address; other addresses (`data:`, `file:`) as they are.
pub(crate) fn origin_of(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.split(',').next().unwrap_or_default().to_owned();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    format!("{scheme}://{host}")
}

/// The permissions in CEF's prompt bits.
pub(crate) fn permissions(bits: u32) -> Vec<Permission> {
    use cef::sys::cef_permission_request_types_t as Type;
    const NAMED: [(u32, Permission); 13] = [
        (
            Type::CEF_PERMISSION_TYPE_CAMERA_STREAM as u32,
            Permission::Camera,
        ),
        (
            Type::CEF_PERMISSION_TYPE_MIC_STREAM as u32,
            Permission::Microphone,
        ),
        (
            Type::CEF_PERMISSION_TYPE_CAPTURED_SURFACE_CONTROL as u32,
            Permission::Screen,
        ),
        (
            Type::CEF_PERMISSION_TYPE_GEOLOCATION as u32,
            Permission::Location,
        ),
        (
            Type::CEF_PERMISSION_TYPE_NOTIFICATIONS as u32,
            Permission::Notifications,
        ),
        (
            Type::CEF_PERMISSION_TYPE_CLIPBOARD as u32,
            Permission::Clipboard,
        ),
        (
            Type::CEF_PERMISSION_TYPE_MIDI_SYSEX as u32,
            Permission::Midi,
        ),
        (
            Type::CEF_PERMISSION_TYPE_SENSORS as u32,
            Permission::Sensors,
        ),
        (
            Type::CEF_PERMISSION_TYPE_LOCAL_FONTS as u32,
            Permission::Fonts,
        ),
        (
            Type::CEF_PERMISSION_TYPE_WINDOW_MANAGEMENT as u32,
            Permission::Windows,
        ),
        (
            Type::CEF_PERMISSION_TYPE_LOCAL_NETWORK as u32
                | Type::CEF_PERMISSION_TYPE_LOOPBACK_NETWORK as u32
                | Type::CEF_PERMISSION_TYPE_LOCAL_NETWORK_ACCESS_DEPRECATED as u32,
            Permission::LocalNetwork,
        ),
        (
            Type::CEF_PERMISSION_TYPE_FILE_SYSTEM_ACCESS as u32,
            Permission::Files,
        ),
        (
            Type::CEF_PERMISSION_TYPE_MULTIPLE_DOWNLOADS as u32,
            Permission::Downloads,
        ),
    ];
    let mut out: Vec<Permission> = NAMED
        .iter()
        .filter(|(mask, _)| bits & mask != 0)
        .map(|(_, permission)| *permission)
        .collect();
    let known = NAMED.iter().fold(0, |all, (mask, _)| all | mask);
    if bits & !known != 0 {
        out.push(Permission::Other);
    }
    out
}

/// The permissions in CEF's media access bits.
pub(crate) fn media(bits: u32) -> Vec<Permission> {
    use cef::sys::cef_media_access_permission_types_t as Media;
    let screen = Media::CEF_MEDIA_PERMISSION_DESKTOP_AUDIO_CAPTURE as u32
        | Media::CEF_MEDIA_PERMISSION_DESKTOP_VIDEO_CAPTURE as u32;
    [
        (
            Media::CEF_MEDIA_PERMISSION_DEVICE_AUDIO_CAPTURE as u32,
            Permission::Microphone,
        ),
        (
            Media::CEF_MEDIA_PERMISSION_DEVICE_VIDEO_CAPTURE as u32,
            Permission::Camera,
        ),
        (screen, Permission::Screen),
    ]
    .into_iter()
    .filter(|(mask, _)| bits & mask != 0)
    .map(|(_, permission)| permission)
    .collect()
}

pub(crate) fn file_mode(mode: FileDialogMode) -> FileMode {
    if mode == FileDialogMode::OPEN_MULTIPLE {
        FileMode::OpenMultiple
    } else if mode == FileDialogMode::OPEN_FOLDER {
        FileMode::Folder
    } else if mode == FileDialogMode::SAVE {
        FileMode::Save
    } else {
        FileMode::Open
    }
}

/// A borrowed CEF string list, read without taking it over.
pub(crate) fn strings(list: Option<&mut CefStringList>) -> Vec<String> {
    list.map(|list| {
        let raw: *mut cef::sys::_cef_string_list_t = list.into();
        CefStringList::from(raw).into_iter().collect()
    })
    .unwrap_or_default()
}

/// A certificate error, as the person should read it.
pub(crate) fn certificate_error(error: Errorcode) -> String {
    use cef::sys::cef_errorcode_t as Code;
    match Code::from(error) {
        Code::ERR_CERT_COMMON_NAME_INVALID => "The certificate is for another site.",
        Code::ERR_CERT_DATE_INVALID => "The certificate has expired or is not valid yet.",
        Code::ERR_CERT_AUTHORITY_INVALID => "The certificate is not issued by a trusted authority.",
        Code::ERR_CERT_REVOKED => "The certificate has been revoked.",
        Code::ERR_CERT_WEAK_SIGNATURE_ALGORITHM | Code::ERR_CERT_WEAK_KEY => {
            "The certificate uses weak cryptography."
        }
        Code::ERR_CERT_SELF_SIGNED_LOCAL_NETWORK => "The certificate is self-signed.",
        _ => "The certificate is not valid.",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_keep_scheme_host_and_port() {
        assert_eq!(
            origin_of("https://user:pw@example.com:8443/path?q#x"),
            "https://example.com:8443"
        );
        assert_eq!(origin_of("http://localhost:3000"), "http://localhost:3000");
        assert_eq!(origin_of("data:text/html,<p>hi"), "data:text/html");
        assert_eq!(origin_of("about:blank"), "about:blank");
    }

    #[test]
    fn an_answer_holds_only_for_its_own_kind_of_question() {
        let files = Answer::Files {
            paths: vec!["/w/a.png".into()],
        };
        let text = Answer::Text { text: "hi".into() };
        let signed_in = Answer::Credentials {
            username: "a".into(),
            password: "b".into(),
        };
        for asked in [Asked::Script, Asked::Auth, Asked::Prompt, Asked::Media, Asked::Certificate] {
            assert!(!fits(asked, &files), "files answer no {asked:?}");
        }
        assert!(fits(Asked::File, &files));
        for answer in [&Answer::Accept, &text, &signed_in] {
            assert!(!fits(Asked::File, answer), "{answer:?} chooses no files");
        }
        for asked in [Asked::Prompt, Asked::Media, Asked::Certificate] {
            assert!(fits(asked, &Answer::Accept));
            assert!(!fits(asked, &text) && !fits(asked, &signed_in), "{asked:?}");
        }
        assert!(fits(Asked::Script, &Answer::Accept) && fits(Asked::Script, &text));
        assert!(fits(Asked::Auth, &signed_in) && !fits(Asked::Auth, &Answer::Accept));
        for asked in [Asked::Script, Asked::Auth, Asked::Prompt, Asked::Media, Asked::Certificate, Asked::File] {
            assert!(fits(asked, &Answer::Dismiss), "no answers any {asked:?}");
        }
    }

    #[test]
    fn prompt_and_media_bits_name_their_permissions() {
        assert_eq!(
            permissions(4 | 4096),
            [Permission::Camera, Permission::Microphone]
        );
        assert_eq!(permissions(256), [Permission::Location]);
        assert_eq!(permissions(1), [Permission::Other]);
        assert_eq!(media(1 | 2), [Permission::Microphone, Permission::Camera]);
        assert_eq!(media(8), [Permission::Screen]);
    }
}
