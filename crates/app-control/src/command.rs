//! The registry: one [`Command`] variant per operation, generated from one list, so adding
//! an operation is one line here plus its types under `ops`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ControlError, Descriptor, Hints, Info, Operation, Scope, ops};

/// An operation that is in the registry, so its input can be wrapped in a [`Command`].
/// Implemented by the registry macro only.
pub trait Registered: Operation {
    /// Wraps `input` in this operation's command.
    fn command(input: Self::Input) -> Command;
}

macro_rules! registry {
    ($($variant:ident => $op:ty),+ $(,)?) => {
        /// One call: the operation's name as `operation` and its typed input as `input`.
        /// The variants' `snake_case` names are the operations' [`Operation::NAME`]s; a
        /// test holds them equal.
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(tag = "operation", content = "input", rename_all = "snake_case")]
        pub enum Command {
            $(
                #[doc = concat!("See [`", stringify!($op), "`].")]
                $variant(<$op as Operation>::Input),
            )+
        }

        impl Command {
            /// What the catalog says about the operation.
            pub fn info(&self) -> Info {
                match self {
                    $(Command::$variant(_) => Info::of::<$op>(),)+
                }
            }

            /// Every scope this call needs, given its input: the operation's, then its
            /// [`Operation::ALSO`] when the input asks for them ([`Operation::needs_also`]).
            pub fn scopes(&self) -> Vec<Scope> {
                match self {
                    $(Command::$variant(input) => {
                        let mut scopes = vec![<$op as Operation>::SCOPE];
                        if <$op as Operation>::needs_also(input) {
                            scopes.extend_from_slice(<$op as Operation>::ALSO);
                        }
                        scopes
                    })+
                }
            }

            /// A one-line summary of `output`, when it is this operation's output.
            pub fn summary(&self, output: &Value) -> Option<String> {
                summarize(self.name(), output)
            }
        }

        $(
            impl Registered for $op {
                fn command(input: Self::Input) -> Command {
                    Command::$variant(input)
                }
            }
        )+

        /// Every operation's catalog entry with its schemas, in registry order.
        pub fn catalog() -> Vec<Descriptor> {
            vec![$(Descriptor::of::<$op>(),)+]
        }

        /// Every operation's [`Info`], in registry order, without building schemas.
        pub fn infos() -> Vec<Info> {
            vec![$(Info::of::<$op>(),)+]
        }

        /// A one-line summary of `output` from the operation named `name`, or `None` when
        /// the name is unknown or the output isn't its output.
        pub fn summarize(name: &str, output: &Value) -> Option<String> {
            $(
                if name == <$op as Operation>::NAME {
                    return <<$op as Operation>::Output as Deserialize>::deserialize(output)
                        .ok()
                        .map(|output| <$op as Operation>::summary(&output));
                }
            )+
            None
        }
    };
}

/// The [`Info`] of the operation named `name`, without building its schemas.
pub fn info(name: &str) -> Option<Info> {
    infos().into_iter().find(|info| info.name == name)
}

impl Command {
    /// The operation's tool name.
    pub fn name(&self) -> &'static str {
        self.info().name
    }

    /// The scope every call of the operation needs.
    pub fn scope(&self) -> Scope {
        self.info().scope
    }

    /// How the operation behaves.
    pub fn hints(&self) -> Hints {
        self.info().hints
    }

    /// Whether the person confirms this call in the window first: the operation is
    /// confirmed, or this input makes it need `destructive`.
    pub fn confirmed(&self) -> bool {
        self.info().confirm || self.scopes().contains(&Scope::Destructive)
    }

    /// The call named `operation` with `input`, as a call frame carries it.
    ///
    /// # Errors
    /// `invalid_input` when no operation has that name, or the input isn't its input (an
    /// unknown or missing field, a wrong type); the message says which.
    pub fn parse(operation: &str, input: Value) -> Result<Command, ControlError> {
        if info(operation).is_none() {
            return Err(ControlError::invalid_input(format!(
                "Koyo has no operation named `{operation}`."
            )));
        }
        let input = if input.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            input
        };
        let call = serde_json::json!({ "operation": operation, "input": input });
        Command::deserialize(call).map_err(|error| {
            ControlError::invalid_input(format!("The input of {operation} is invalid: {error}"))
        })
    }

    /// The operation's name and its input as JSON, as a call frame carries them.
    pub fn to_parts(&self) -> (&'static str, Value) {
        let input = match serde_json::to_value(self) {
            Ok(Value::Object(mut call)) => call.remove("input"),
            _ => None,
        };
        (
            self.name(),
            input.unwrap_or_else(|| Value::Object(serde_json::Map::new())),
        )
    }
}

registry! {
    AppStatus => ops::app::AppStatus,
    AppFocus => ops::app::AppFocus,
    LayoutSet => ops::app::LayoutSet,
    SettingsOpen => ops::app::SettingsOpen,
    SettingsClose => ops::app::SettingsClose,
    ProjectList => ops::project::ProjectList,
    ProjectAdd => ops::project::ProjectAdd,
    ProjectSelect => ops::project::ProjectSelect,
    ProjectRemove => ops::project::ProjectRemove,
    ProjectMove => ops::project::ProjectMove,
    WorktreeBranches => ops::worktree::WorktreeBranches,
    WorktreeCreate => ops::worktree::WorktreeCreate,
    WorktreeSelect => ops::worktree::WorktreeSelect,
    WorktreeDelete => ops::worktree::WorktreeDelete,
    TabList => ops::tab::TabList,
    TabOpen => ops::tab::TabOpen,
    TabSelect => ops::tab::TabSelect,
    TabMove => ops::tab::TabMove,
    TabClose => ops::tab::TabClose,
    TabReopen => ops::tab::TabReopen,
    PaneSplit => ops::pane::PaneSplit,
    PaneFocus => ops::pane::PaneFocus,
    PaneResize => ops::pane::PaneResize,
    PaneDetach => ops::pane::PaneDetach,
    PaneClose => ops::pane::PaneClose,
    FileOpen => ops::file::FileOpen,
    FileSave => ops::file::FileSave,
    ChangesOpen => ops::file::ChangesOpen,
    BrowserList => ops::browser::BrowserList,
    BrowserNavigate => ops::browser::BrowserNavigate,
    BrowserGo => ops::browser::BrowserGo,
    BrowserRead => ops::browser::BrowserRead,
    BrowserSnapshot => ops::browser::BrowserSnapshot,
    BrowserScreenshot => ops::browser::BrowserScreenshot,
    BrowserEval => ops::browser::BrowserEval,
    BrowserClick => ops::browser::BrowserClick,
    BrowserType => ops::browser::BrowserType,
    BrowserPressKey => ops::browser::BrowserPressKey,
    BrowserScroll => ops::browser::BrowserScroll,
    BrowserWait => ops::browser::BrowserWait,
    BrowserViewport => ops::browser::BrowserViewport,
    BrowserClaim => ops::browser::BrowserClaim,
    BrowserRelease => ops::browser::BrowserRelease,
    SettingsGet => ops::settings::SettingsGet,
    SettingsSet => ops::settings::SettingsSet,
}
