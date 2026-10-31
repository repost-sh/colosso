//! Questions a page asks the person: JavaScript dialogs, sign-ins,
//! permissions, certificate errors and file choosers.
//!
//! CEF hands each one a callback and waits. The host keeps the callback
//! under a number, asks the app with [`HostEvent::Dialog`], and resolves it
//! when [`AppRequest::Answer`] comes back. Questions arrive on CEF's UI and
//! IO threads, so the waiting callbacks sit behind a lock that is never held
//! across a call into CEF.
//!
//! [`AppRequest::Answer`]: colosso_browser_ipc::AppRequest::Answer
use crate::host;
use crate::pending::Pending;
use cef::{
    AuthCallback, Callback, CefStringList, Errorcode, FileDialogCallback, FileDialogMode,
    ImplAuthCallback, ImplCallback, ImplFileDialogCallback, ImplJsdialogCallback,
    ImplMediaAccessCallback, ImplPermissionPromptCallback, JsdialogCallback, MediaAccessCallback,
    PermissionPromptCallback, PermissionRequestResult,
};
use colosso_browser_ipc::{Answer, Dialog, FileMode, HostEvent, PaneId, Permission};
use std::sync::Mutex;

/// A callback CEF waits on.
pub(crate) enum Waiting {
    /// `alert`, `confirm`, `prompt` and `beforeunload`.
    Script(JsdialogCallback),
    /// A sign-in.
    Auth(AuthCallback),
    /// A permission prompt, with CEF's id for it.
    Prompt(u64, PermissionPromptCallback),
    /// Camera, microphone or screen, with the bits asked for.
    Media(u32, MediaAccessCallback),
    /// An untrusted certificate.
    Certificate(Callback),
    /// A file chooser.
    File(FileDialogCallback),
}

// SAFETY: CEF's callbacks are reference counted atomically and may be
// resolved from any thread; the host resolves them on the UI thread.
unsafe impl Send for Waiting {}

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

/// The person answered.
pub(crate) fn answer(pane: PaneId, dialog: u64, answer: Answer) {
    if let Some(waiting) = with(|dialogs| dialogs.remove(pane, dialog)) {
        resolve(waiting, answer);
    }
}

/// The page navigated or reloaded: its script dialogs are void.
pub(crate) fn reset(pane: PaneId) {
    withdraw(pane, |waiting| matches!(waiting, Waiting::Script(_)));
}

/// The engine took a permission prompt back.
pub(crate) fn prompt_dismissed(pane: PaneId, prompt: u64) {
    withdraw(
        pane,
        |waiting| matches!(waiting, Waiting::Prompt(id, _) if *id == prompt),
    );
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

fn withdraw(pane: PaneId, which: impl Fn(&Waiting) -> bool) {
    let withdrawn = with(|dialogs| dialogs.take(|p, waiting| p == pane && which(waiting)));
    for (dialog, _) in withdrawn {
        host::send(&HostEvent::DialogClosed { pane, dialog });
    }
}

fn decline(waiting: Waiting) {
    match waiting {
        Waiting::Prompt(_, callback) => callback.cont(PermissionRequestResult::DISMISS),
        waiting => resolve(waiting, Answer::Dismiss),
    }
}

fn resolve(waiting: Waiting, answer: Answer) {
    let accepted = !matches!(answer, Answer::Dismiss);
    match waiting {
        Waiting::Script(callback) => {
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
        Waiting::Prompt(_, callback) => callback.cont(if accepted {
            PermissionRequestResult::ACCEPT
        } else {
            PermissionRequestResult::DENY
        }),
        Waiting::Media(requested, callback) => {
            if accepted {
                callback.cont(requested);
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
