//! What a chat runs with, and what a call may set it to: read off the host's catalog for its
//! checkout by the rules the composer goes by (`chat::chosen`), so a caller is told what the
//! window shows, which is what runs; and what a call asks for, checked against the same catalog.

use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::methods::{Catalog, Model, UpdateParams};
use koyo_agent::protocol::model::{Effort, Mode};
use koyo_agent::words::{model_name, row_name};
use koyo_app_control::ControlError;
use koyo_app_control::ops::chat::{
    ChatChoices, ChatCreateInput, ChatEffort, ChatMode, ChatModelChoice, ChatRunsWith, ChatSetInput,
};

use crate::chat::chosen::Settings;

/// The id of the row that runs what the agent recommends, which a model's own row says more of.
const RECOMMENDED: &str = "default";

/// What a call asks a chat to run with: what's `None` it doesn't ask about.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Asked<'a> {
    pub model: Option<&'a str>,
    pub effort: Option<ChatEffort>,
    pub mode: Option<ChatMode>,
    pub fast: Option<bool>,
    pub plan: Option<bool>,
}

impl Asked<'_> {
    pub fn of(input: &ChatCreateInput) -> Asked<'_> {
        Asked {
            model: input.model.as_deref(),
            effort: input.effort,
            mode: input.mode,
            fast: input.fast,
            plan: input.plan,
        }
    }

    pub fn of_set(input: &ChatSetInput) -> Asked<'_> {
        Asked {
            model: input.model.as_deref(),
            effort: input.effort,
            mode: input.mode,
            fast: input.fast,
            plan: input.plan,
        }
    }

    /// Whether it asks about anything.
    pub fn asks(&self) -> bool {
        self.model.is_some()
            || self.effort.is_some()
            || self.mode.is_some()
            || self.fast.is_some()
            || self.plan.is_some()
    }
}

/// What thread `id`, set to `current` as far as the app knows, has to hear to be set to `next`
/// for `asked`: what differs, and what was asked about and what another model changes with it,
/// said whether or not the app's copy has it already, as that may lag the host's.
pub(crate) fn changes(
    current: &Settings,
    next: &Settings,
    asked: &Asked,
    id: ThreadId,
) -> UpdateParams {
    let mut params = current.changes(next, id);
    let model = asked.model.is_some();
    if model {
        params.model = Some(next.model.clone());
    }
    if model || asked.effort.is_some() {
        params.effort = Some(next.effort);
    }
    if model || asked.fast.is_some() {
        params.fast = Some(next.fast);
    }
    if asked.mode.is_some() {
        params.permission = Some(next.permission);
    }
    if asked.plan.is_some() {
        params.plan = Some(next.plan);
    }
    params
}

/// What a chat set to `chosen` runs with, as its composer shows it; `chat_effort` is Koyo's
/// setting for new chats. An empty catalog, which the host gives while the agent isn't ready,
/// tells only what the chat chose.
pub(crate) fn runs_with(chosen: &Settings, catalog: &Catalog, chat_effort: Effort) -> ChatRunsWith {
    let model = chosen.model_in(catalog);
    let effort = model.map_or(chosen.effort, |model| chosen.effort_on(model, chat_effort));
    ChatRunsWith {
        model: catalog.model_id(chosen.model.as_deref()).map(str::to_owned),
        model_name: model.map(|model| model_name(model).to_owned()),
        effort: effort.map(effort_out),
        mode: chosen.mode(catalog).map(mode_out),
        fast: chosen.fast,
        plan: chosen.plan,
    }
}

/// The effort a chat set to `chosen` has to take before its next message for what its composer
/// shows to be what runs, as the composer has it take: none if it's on it already, or its model
/// has no efforts or isn't listed.
pub(crate) fn to_settle(
    chosen: &Settings,
    catalog: &Catalog,
    chat_effort: Effort,
) -> Option<Effort> {
    let model = chosen.model_in(catalog)?;
    (chosen.effort_on(model, chat_effort)).filter(|effort| Some(*effort) != chosen.effort)
}

/// What a chat set to `current` runs with once `asked` changes it, checked against `catalog` as
/// the composer's chips offer it: another model keeps only what it can do
/// (`Settings::with_model`); an effort, fast or `auto` it can't is refused, naming what it can.
/// What's not asked about stays. `own` is the mode of the chat whose agent asks, if a chat's
/// does: a mode above it is refused (decided 2026-11-11).
pub(crate) fn changed_by(
    current: &Settings,
    asked: &Asked,
    catalog: &Catalog,
    own: Option<Mode>,
) -> Result<Settings, ControlError> {
    let mut next = current.clone();
    if let Some(named) = asked.model {
        next.with_model(model_named(catalog, named)?, catalog.default_mode);
    }
    // The model it runs on: the one it chose, or the one the agent's settings choose.
    let on = next.model_in(catalog);
    let checking = |what: &str| match on {
        Some(on) => Ok(on),
        None if catalog.models.is_empty() => Err(not_ready(what)),
        None => Err(unlisted(catalog, next.model.as_deref(), what)),
    };
    if let Some(effort) = asked.effort {
        next.effort = Some(fits(checking("effort")?, effort_in(effort))?);
    }
    if let Some(fast) = asked.fast {
        if fast {
            let on = checking("fast")?;
            if !on.fast {
                return Err(no_fast(catalog, on));
            }
        }
        next.fast = fast;
    }
    if let Some(mode) = asked.mode.map(mode_in) {
        if let Some(on) = on.filter(|on| mode == Mode::Auto && !on.auto) {
            return Err(no_auto(on));
        }
        if let Some(own) = own.filter(|own| mode > *own) {
            return Err(above_own(mode, own));
        }
        next.permission = Some(mode);
    }
    if let Some(plan) = asked.plan {
        next.plan = plan;
    }
    Ok(next)
}

/// What a new chat runs with for what `asked` asks, checked against `catalog`: what it leaves out
/// is as a new chat in the window starts, and its effort is taken now, as the composer takes it
/// with the first message, so what shows is what runs. Started by the agent of a chat in `own`,
/// it gets no more than that: a mode left out that the settings would set higher is `own`
/// instead, or `ask` on a model that can't run in an `own` of `auto`.
pub(crate) fn chosen_for(
    asked: &Asked,
    catalog: &Catalog,
    chat_effort: Effort,
    own: Option<Mode>,
) -> Result<Settings, ControlError> {
    let mut chosen = changed_by(&Settings::default(), asked, catalog, own)?;
    if let Some(model) = chosen.model_in(catalog) {
        chosen.effort = chosen.effort_on(model, chat_effort);
    }
    if let Some(own) = own.filter(|own| chosen.mode_by(catalog.default_mode) > *own) {
        let runnable = own != Mode::Auto || chosen.can_auto(catalog);
        chosen.permission = Some(if runnable { own } else { Mode::Ask });
    }
    Ok(chosen)
}

/// A chat's agent asked for `asked`, above its own chat's `own`.
fn above_own(asked: Mode, own: Mode) -> ControlError {
    let may = (Mode::ALL.into_iter())
        .filter(|mode| *mode <= own)
        .map(|mode| mode_out(mode).id());
    ControlError::invalid_input(format!(
        "The chat you're in runs in `{}`, and a chat's agent gives no chat more than its own: \
         not `{}`. Choose one of: {}; or ask the person to change your chat's mode in the window.",
        mode_out(own).id(),
        mode_out(asked).id(),
        ticked(may)
    ))
}

/// What a worktree's chats can choose from, and what a chat that chooses nothing runs with.
pub(crate) fn choices(catalog: &Catalog, chat_effort: Effort) -> ChatChoices {
    let defaults = chosen_for(&Asked::default(), catalog, chat_effort, None).unwrap_or_default();
    ChatChoices {
        models: catalog.models.iter().map(model_choice).collect(),
        modes: ChatMode::ALL.to_vec(),
        defaults: runs_with(&defaults, catalog, chat_effort),
    }
}

fn model_choice(model: &Model) -> ChatModelChoice {
    let name = model_name(model).to_owned();
    let row = row_name(model);
    ChatModelChoice {
        row: (row != name).then_some(row),
        id: model.id.clone(),
        name,
        efforts: model.efforts.iter().copied().map(effort_out).collect(),
        fast: model.fast,
        auto: model.auto,
    }
}

/// The model `asked` names: by its id, or else by what it runs or what it goes by, a model's
/// own row ahead of the one that runs what's recommended.
fn model_named<'a>(catalog: &'a Catalog, asked: &str) -> Result<&'a Model, ControlError> {
    if catalog.models.is_empty() {
        return Err(not_ready("model"));
    }
    if let Some(model) = catalog.models.iter().find(|model| model.id == asked) {
        return Ok(model);
    }
    let goes_by = |model: &&Model| {
        model.id.eq_ignore_ascii_case(asked)
            || model.runs.as_deref() == Some(asked)
            || model_name(model).eq_ignore_ascii_case(asked)
            || model.name.eq_ignore_ascii_case(asked)
    };
    let mut matching = catalog.models.iter().filter(goes_by);
    let own = matching.clone().find(|model| model.id != RECOMMENDED);
    own.or_else(|| matching.next()).ok_or_else(|| {
        ControlError::invalid_input(format!(
            "Koyo lists no model `{asked}` for this worktree's chats. Choose one of: {}.",
            listed(catalog.models.iter())
        ))
    })
}

/// `effort`, if `model` runs at it.
fn fits(model: &Model, effort: Effort) -> Result<Effort, ControlError> {
    if model.efforts.contains(&effort) {
        return Ok(effort);
    }
    let name = model_name(model);
    let message = if model.efforts.is_empty() {
        format!("{name} has no efforts to choose: leave `effort` out.")
    } else {
        let efforts = model.efforts.iter().map(|effort| effort_out(*effort).id());
        format!(
            "{name} doesn't run at `{}`. Choose one of: {}.",
            effort_out(effort).id(),
            ticked(efforts)
        )
    };
    Err(ControlError::invalid_input(message))
}

fn no_fast(catalog: &Catalog, model: &Model) -> ControlError {
    let fast = catalog.models.iter().filter(|model| model.fast);
    let others = listed(fast);
    let choose = if others.is_empty() {
        "no model Koyo lists has one".to_owned()
    } else {
        format!("these have one: {others}")
    };
    ControlError::invalid_input(format!(
        "{} has no fast mode: leave `fast` out, or choose a `model` that has one; {choose}.",
        model_name(model)
    ))
}

fn no_auto(model: &Model) -> ControlError {
    let others = (ChatMode::ALL.into_iter())
        .filter(|mode| *mode != ChatMode::Auto)
        .map(ChatMode::id);
    ControlError::invalid_input(format!(
        "{} can't run in `auto`. Choose one of: {}.",
        model_name(model),
        ticked(others)
    ))
}

/// The model the chat runs on, `chosen` or the one the agent's settings name, isn't listed, so
/// what `what` asks can't be checked.
fn unlisted(catalog: &Catalog, chosen: Option<&str>, what: &str) -> ControlError {
    let named = catalog.model_id(chosen).unwrap_or("its own");
    let whose = if chosen.is_some() {
        "the chat chose"
    } else {
        "the agent's own settings name"
    };
    ControlError::invalid_input(format!(
        "The chat runs on `{named}`, as {whose}, which Koyo doesn't list, so it can't check \
         `{what}`: name a `model` too, one of: {}.",
        listed(catalog.models.iter())
    ))
}

fn not_ready(what: &str) -> ControlError {
    ControlError::invalid_input(format!(
        "The agent can't say yet what this worktree's chats can run with, so Koyo can't check \
         `{what}`: is Claude Code installed and signed in (Settings → Chat)? Leave `model`, \
         `effort` and `fast` out to start the chat as a new one starts, or try again."
    ))
}

/// "`low`, `high`".
fn ticked<'a>(ids: impl Iterator<Item = &'a str>) -> String {
    ids.map(|id| format!("`{id}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// "`opus` (Opus 5.5), `haiku` (Haiku 5.5)".
fn listed<'a>(models: impl Iterator<Item = &'a Model>) -> String {
    let named: Vec<String> = models
        .map(|model| format!("`{}` ({})", model.id, model_name(model)))
        .collect();
    named.join(", ")
}

fn effort_in(effort: ChatEffort) -> Effort {
    match effort {
        ChatEffort::Low => Effort::Low,
        ChatEffort::Medium => Effort::Medium,
        ChatEffort::High => Effort::High,
        ChatEffort::Xhigh => Effort::Xhigh,
        ChatEffort::Max => Effort::Max,
    }
}

fn effort_out(effort: Effort) -> ChatEffort {
    match effort {
        Effort::Low => ChatEffort::Low,
        Effort::Medium => ChatEffort::Medium,
        Effort::High => ChatEffort::High,
        Effort::Xhigh => ChatEffort::Xhigh,
        Effort::Max => ChatEffort::Max,
    }
}

fn mode_in(mode: ChatMode) -> Mode {
    match mode {
        ChatMode::Ask => Mode::Ask,
        ChatMode::AcceptEdits => Mode::AcceptEdits,
        ChatMode::Auto => Mode::Auto,
        ChatMode::FullAccess => Mode::FullAccess,
    }
}

fn mode_out(mode: Mode) -> ChatMode {
    match mode {
        Mode::Ask => ChatMode::Ask,
        Mode::AcceptEdits => ChatMode::AcceptEdits,
        Mode::Auto => ChatMode::Auto,
        Mode::FullAccess => ChatMode::FullAccess,
    }
}
