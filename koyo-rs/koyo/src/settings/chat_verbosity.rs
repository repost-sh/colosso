//! How much of what an agent did along the way shows in a chat (`docs/chat.md` 5.3): for each
//! kind of step, its one line, a few lines of what it gave back, or all of it; and whether a
//! finished turn's work folds into one line. What it said always shows, whichever this is.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, NoAction};
use serde::{Deserialize, Serialize};

use super::checked_menu;

/// How much of a step shows until a person opens or closes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shows {
    /// What it did, on one line that opens on what it gave back.
    Line,
    /// That line, and a few lines of what it gave back that open on the rest.
    Preview,
    /// All it gave back.
    All,
}

/// The kinds of step a person chooses how much of to see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Files an agent changed, as their diffs.
    Changes,
    /// Commands it ran, as what they printed.
    Commands,
    /// Every other tool: what it read, searched for, fetched or was told.
    Results,
    Thinking,
    Todos,
}

/// How much of the work shows: what's settled on for each kind of step, and the fold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Verbosity {
    /// Whether a finished turn's work folds into one line where it started.
    pub fold_turns: bool,
    pub changes: Shows,
    pub commands: Shows,
    pub results: Shows,
    pub thinking: Shows,
    pub todos: Shows,
}

/// A verbosity under a name, from least to most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// Once a turn's over, its work behind one line, each step a line of its own.
    Quiet,
    /// As Claude Code shows it: diffs, the end of what commands printed, the to-dos.
    Normal,
    /// Every step with all it gave back, and all the thinking.
    Verbose,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Quiet, Preset::Normal, Preset::Verbose];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Quiet => "Quiet",
            Preset::Normal => "Normal",
            Preset::Verbose => "Verbose",
        }
    }

    pub fn verbosity(self) -> Verbosity {
        let line = Verbosity {
            fold_turns: true,
            changes: Shows::Line,
            commands: Shows::Line,
            results: Shows::Line,
            thinking: Shows::Line,
            todos: Shows::Line,
        };
        match self {
            Preset::Quiet => line,
            Preset::Normal => Verbosity {
                fold_turns: false,
                changes: Shows::All,
                commands: Shows::Preview,
                todos: Shows::All,
                ..line
            },
            Preset::Verbose => Verbosity {
                fold_turns: false,
                changes: Shows::All,
                commands: Shows::All,
                results: Shows::All,
                thinking: Shows::All,
                todos: Shows::All,
            },
        }
    }
}

impl Default for Verbosity {
    fn default() -> Self {
        Preset::Normal.verbosity()
    }
}

/// Sets how much of the work shows in a chat, from the Chat settings' menus.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetChatVerbosity(pub Verbosity);

impl Verbosity {
    /// The preset it is, if it's one.
    pub fn preset(self) -> Option<Preset> {
        Preset::ALL
            .into_iter()
            .find(|preset| preset.verbosity() == self)
    }

    pub fn label(self) -> &'static str {
        self.preset().map_or("Custom", Preset::label)
    }

    /// Where what's settled on for a kind of step is kept.
    fn slot(&mut self, step: Step) -> &mut Shows {
        match step {
            Step::Changes => &mut self.changes,
            Step::Commands => &mut self.commands,
            Step::Results => &mut self.results,
            Step::Thinking => &mut self.thinking,
            Step::Todos => &mut self.todos,
        }
    }

    /// How much of a kind of step shows, of what it can.
    pub fn shows(mut self, step: Step) -> Shows {
        let shows = *self.slot(step);
        // A file that says a few lines of a kind with none to pick shows all of it.
        if shows == Shows::Preview && !step.choices().contains(&Shows::Preview) {
            Shows::All
        } else {
            shows
        }
    }

    pub fn with(mut self, step: Step, shows: Shows) -> Self {
        *self.slot(step) = shows;
        self
    }

    /// The presets, with the one it is checked, or else a Custom item that says so.
    pub fn menu(self) -> NativeMenu {
        let presets = Preset::ALL
            .into_iter()
            .fold(NativeMenu::new(), |menu, preset| {
                let action = Box::new(SetChatVerbosity(preset.verbosity()));
                menu.menu_with_check(preset.label(), self.preset() == Some(preset), action)
            });
        if self.preset().is_some() {
            return presets;
        }
        // Checked and greyed out, it only says the steps were chosen one by one.
        presets
            .separator()
            .menu_with_disabled("Custom", true, Box::new(NoAction))
    }

    /// What a kind of step can show, with what it shows now checked.
    pub fn step_menu(self, step: Step) -> NativeMenu {
        checked_menu(
            step.choices().iter().copied(),
            self.shows(step),
            |shows| step.says(shows),
            |shows| Box::new(SetChatVerbosity(self.with(step, shows))),
        )
    }

    pub fn turns_label(self) -> &'static str {
        if self.fold_turns {
            "One line"
        } else {
            "Every step"
        }
    }

    /// Whether a finished turn folds, with what it does now checked.
    pub fn turns_menu(self) -> NativeMenu {
        let folding = |fold_turns| Verbosity { fold_turns, ..self };
        checked_menu(
            [false, true],
            self.fold_turns,
            |fold| folding(fold).turns_label(),
            |fold| Box::new(SetChatVerbosity(folding(fold))),
        )
    }
}

impl Step {
    pub const ALL: [Step; 5] = [
        Step::Changes,
        Step::Commands,
        Step::Results,
        Step::Thinking,
        Step::Todos,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Step::Changes => "Code changes",
            Step::Commands => "Command output",
            Step::Results => "Other tools",
            Step::Thinking => "Thinking",
            Step::Todos => "To-dos",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Step::Changes => "A file the agent edited or wrote, with the lines it changed.",
            Step::Commands => "What a command printed. Its last lines say how it went.",
            Step::Results => "What a file read, a search, a web page or an MCP tool gave back.",
            Step::Thinking => "What the agent thought before it acted.",
            Step::Todos => "The agent's to-do list, each time it updates it.",
        }
    }

    /// What a person can pick for it, from least to most.
    pub fn choices(self) -> &'static [Shows] {
        match self {
            Step::Commands | Step::Results => &[Shows::Line, Shows::Preview, Shows::All],
            Step::Changes | Step::Thinking | Step::Todos => &[Shows::Line, Shows::All],
        }
    }

    /// What a choice is called for it.
    pub fn says(self, shows: Shows) -> &'static str {
        match (self, shows) {
            (_, Shows::Line) => "One line",
            (Step::Changes, _) => "The diff",
            (Step::Todos, _) => "The list",
            (Step::Commands, Shows::Preview) => "The last lines",
            (_, Shows::Preview) => "The first lines",
            (_, Shows::All) => "All of it",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preset_is_known_again_and_a_step_changed_by_hand_is_custom() {
        for preset in Preset::ALL {
            assert_eq!(preset.verbosity().preset(), Some(preset));
        }
        let normal = Verbosity::default();
        assert_eq!(normal.label(), "Normal");
        let custom = normal.with(Step::Thinking, Shows::All);
        assert_eq!(custom.preset(), None);
        assert_eq!(custom.label(), "Custom");
        assert_eq!(custom.shows(Step::Thinking), Shows::All);
    }

    #[test]
    fn a_few_lines_of_a_kind_with_none_to_pick_is_all_of_it() {
        let verbosity = Verbosity::default().with(Step::Todos, Shows::Preview);
        assert_eq!(verbosity.shows(Step::Todos), Shows::All);
    }

    #[test]
    fn what_a_file_leaves_out_is_the_default() {
        let read: Verbosity = serde_json::from_str(r#"{ "thinking": "All" }"#).unwrap();
        assert_eq!(read, Verbosity::default().with(Step::Thinking, Shows::All));
    }
}
