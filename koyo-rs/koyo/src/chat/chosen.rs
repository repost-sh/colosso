//! What a chat is set to run with, and the rules that say what it runs with (`docs/chat.md` 5.3):
//! the composer's chips and app control's chat operations both go by them, so what a tool says
//! of a chat is what its composer shows, which is what runs.

use std::path::Path;

use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::methods::{Catalog, CreateParams, Model, UpdateParams};
use koyo_agent::protocol::model::{Effort, Mode, Thread, ThreadSummary};

/// What a chat is set to: what's `None` the person's and the agent's settings choose.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Settings {
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<Effort>,
    pub(crate) fast: bool,
    pub(crate) permission: Option<Mode>,
    pub(crate) plan: bool,
}

impl Settings {
    pub(crate) fn of(thread: &Thread) -> Settings {
        Settings {
            model: thread.model.clone(),
            effort: thread.effort,
            fast: thread.fast,
            permission: thread.permission,
            plan: thread.plan,
        }
    }

    pub(crate) fn of_summary(summary: &ThreadSummary) -> Settings {
        Settings {
            model: summary.model.clone(),
            effort: summary.effort,
            fast: summary.fast,
            permission: summary.permission,
            plan: summary.plan,
        }
    }

    /// The model these are on, if the catalog lists it: the chosen one, or else the one the
    /// agent's settings run.
    pub(crate) fn model_in<'a>(&self, catalog: &'a Catalog) -> Option<&'a Model> {
        catalog.model(self.model.as_deref())
    }

    /// A chat in the checkout at `cwd` that starts with these, on the host's first provider.
    pub(crate) fn to_create(&self, cwd: &Path) -> CreateParams {
        CreateParams {
            cwd: cwd.to_string_lossy().into_owned(),
            provider: None,
            model: self.model.clone(),
            effort: self.effort,
            fast: self.fast,
            permission: self.permission,
            plan: self.plan,
        }
    }

    /// What thread `id`, set to these, has to hear to be set to `to`, and nothing else.
    pub(crate) fn changes(&self, to: &Settings, id: ThreadId) -> UpdateParams {
        UpdateParams {
            model: (self.model != to.model).then(|| to.model.clone()),
            effort: (self.effort != to.effort).then_some(to.effort),
            fast: (self.fast != to.fast).then_some(to.fast),
            permission: (self.permission != to.permission).then_some(to.permission),
            plan: (self.plan != to.plan).then_some(to.plan),
            ..UpdateParams::of(id)
        }
    }

    /// On `model`, keeping only what it can do: the effort it was on or the nearest below, else
    /// the least it has; Fast if it can; and a mode it can't run in, even if only the person's
    /// settings chose it, has it ask.
    pub(crate) fn with_model(&mut self, model: &Model, default_mode: Option<Mode>) {
        self.model = Some(model.id.clone());
        self.effort = self.effort.and_then(|effort| model.fitted(effort));
        self.fast &= model.fast;
        if self.permission.or(default_mode) == Some(Mode::Auto) && !model.auto {
            self.permission = Some(Mode::Ask);
        }
    }

    /// The effort that shows on `model`, and that the chat takes with its next message: what it
    /// chose, or else `chat_effort`, Koyo's setting for new chats, as near as the model runs.
    /// None if it has no efforts to choose.
    pub(crate) fn effort_on(&self, model: &Model, chat_effort: Effort) -> Option<Effort> {
        model.fitted(self.effort.unwrap_or(chat_effort))
    }

    /// The mode the chat runs in: what it chose, or else what the person's and the agent's
    /// settings choose, which isn't known until the host says.
    pub(crate) fn mode(&self, catalog: &Catalog) -> Option<Mode> {
        let known = self.permission.is_some() || !catalog.models.is_empty();
        known.then(|| self.mode_by(catalog.default_mode))
    }

    /// The mode the chat runs in where the settings choose `default_mode` for a chat that chose
    /// none: with none of them, the agent asks.
    pub(crate) fn mode_by(&self, default_mode: Option<Mode>) -> Mode {
        self.permission.or(default_mode).unwrap_or(Mode::Ask)
    }

    /// Whether the chat's model can run in Auto, or isn't listed to say it can't.
    pub(crate) fn can_auto(&self, catalog: &Catalog) -> bool {
        self.model_in(catalog).is_none_or(|model| model.auto)
    }
}
