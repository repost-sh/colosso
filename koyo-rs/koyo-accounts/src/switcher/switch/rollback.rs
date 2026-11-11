//! Port of claude-swap's `models.py` `SwitchTransaction` (0fe22a8): a switch's completed steps,
//! undone in reverse when a later one fails. The config is restored by temp file and rename,
//! never rewritten in place: a running Claude Code watches the global config, and a
//! truncate-then-write shows it an empty file (#419).
//!
//! What the switch found before it changed anything is a read-only [`Originals`] beside the
//! completed steps, so the steps can read it while they record themselves.

use std::path::PathBuf;

use crate::switcher::Switcher;
use crate::switcher::store::set_active;
use crate::{Result, fsutil};

/// One step of a switch the transaction can undo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    CredentialsWritten,
    ConfigWritten,
    SequenceUpdated,
}

impl Step {
    fn as_str(self) -> &'static str {
        match self {
            Step::CredentialsWritten => "credentials_written",
            Step::ConfigWritten => "config_written",
            Step::SequenceUpdated => "sequence_updated",
        }
    }
}

/// What the switch found before it changed anything: cswap's `SwitchTransaction`'s
/// `original_*` fields.
pub(crate) struct Originals {
    pub credentials: String,
    pub config: String,
    pub account_num: String,
    pub config_path: PathBuf,
}

/// cswap's `SwitchTransaction`.
pub(crate) struct SwitchTransaction {
    pub originals: Originals,
    pub completed_steps: Vec<Step>,
}

impl SwitchTransaction {
    /// Undoes the completed steps in reverse: whether every one was undone.
    pub fn rollback(&self, switcher: &Switcher) -> bool {
        let mut success = true;
        for step in self.completed_steps.iter().rev() {
            match self.undo(switcher, *step) {
                Ok(()) => log::info!("Rolled back step: {}", step.as_str()),
                Err(error) => {
                    log::error!("Failed to rollback step {}: {error}", step.as_str());
                    success = false;
                }
            }
        }
        success
    }

    fn undo(&self, switcher: &Switcher, step: Step) -> Result<()> {
        let originals = &self.originals;
        match step {
            Step::CredentialsWritten => switcher.store.write_credentials(&originals.credentials),
            Step::ConfigWritten => fsutil::atomic_write(
                &switcher.env,
                &originals.config_path,
                originals.config.as_bytes(),
            ),
            Step::SequenceUpdated => {
                let Some(mut data) = switcher
                    .get_sequence_data()?
                    .filter(|data| !data.is_empty())
                else {
                    return Ok(());
                };
                set_active(&mut data, &originals.account_num)?;
                switcher.save_sequence(&mut data)
            }
        }
    }
}
