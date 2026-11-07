//! The gate every call passes first: what the caller is granted now, and whether that
//! covers every scope the call needs.

use gpui_kit::App;
use koyo_app_control::{Command, ControlError, ErrorCode, Scope};

use super::Caller;
use crate::settings;

/// What `caller` may do now: an agent in a Koyo terminal what such agents may, and a chat's
/// as long as Koyo tools are on; an external client what the person allowed it, as long as
/// external control is on.
pub(crate) fn grant(caller: &Caller, cx: &App) -> Result<Vec<Scope>, ControlError> {
    if caller.chat.is_some() && !settings::get(cx).koyo_tools {
        return Err(ControlError::new(
            ErrorCode::ControlDisabled,
            "Koyo tools are off for chats; the person can turn them on in Settings → Chat.",
        ));
    }
    if !caller.external() {
        return Ok(Scope::AGENT.to_vec());
    }
    let policy = &settings::get(cx).external_control;
    if !policy.enabled {
        return Err(ControlError::control_disabled());
    }
    match policy.client(&caller.client.name) {
        Some(client) => Ok(client.scopes.clone()),
        None => Err(ControlError::user_declined(format!(
            "“{}” isn't allowed to control Koyo any more. Say hello again to ask the person.",
            caller.client.name
        ))),
    }
}

/// Lets `command` through for `caller` when its grant covers every scope the call needs.
pub(crate) fn check(caller: &Caller, command: &Command, cx: &App) -> Result<(), ControlError> {
    let grant = grant(caller, cx)?;
    match command
        .scopes()
        .into_iter()
        .find(|scope| !grant.contains(scope))
    {
        Some(missing) => Err(ControlError::scope_denied(missing)),
        None => Ok(()),
    }
}
