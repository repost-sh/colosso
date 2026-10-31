//! Every terminal the daemon has, and the file that remembers them across a restart so the
//! app and agents can offer to run them again. `docs/terminals.md` 5.2 and 5.9.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use colosso_term_control::{
    Control, Exit, Handle, Status, TerminalRecord, WorktreeRef, WorktreeSummary,
};
use serde::{Deserialize, Serialize};

const FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Stored {
    format: u32,
    terminals: Vec<TerminalRecord>,
}

pub struct Registry {
    path: PathBuf,
    terminals: BTreeMap<Handle, TerminalRecord>,
    dirty: bool,
}

impl Registry {
    /// Reads the file, marking every terminal in it as ended by the restart; a missing file
    /// is an empty registry, an unreadable one is set aside and logged.
    pub fn load(path: PathBuf) -> Registry {
        let mut registry = Registry {
            path,
            terminals: BTreeMap::new(),
            dirty: false,
        };
        let text = match std::fs::read_to_string(&registry.path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return registry,
            Err(error) => {
                log::warn!("starting without {}: {error}", registry.path.display());
                return registry;
            }
        };
        match serde_json::from_str::<Stored>(&text) {
            Ok(stored) if stored.format == FORMAT => {
                for mut record in stored.terminals {
                    record.pid = None;
                    record.status = Status::Exited;
                    record.attached_clients = 0;
                    record.control = Control::default();
                    if record.exit.is_none() {
                        record.exit = Some(Exit {
                            code: None,
                            signal: None,
                            daemon_restart: true,
                        });
                        record.status_reason = "the daemon restarted under it".into();
                    }
                    registry.terminals.insert(record.handle.clone(), record);
                }
            }
            Ok(stored) => log::warn!(
                "ignoring {}: format {} isn't {FORMAT}",
                registry.path.display(),
                stored.format
            ),
            Err(error) => {
                let aside = registry.path.with_extension("json.unreadable");
                log::warn!(
                    "setting {} aside as {}: {error}",
                    registry.path.display(),
                    aside.display()
                );
                let _ = std::fs::rename(&registry.path, aside);
            }
        }
        registry
    }

    pub fn get(&self, handle: &Handle) -> Option<&TerminalRecord> {
        self.terminals.get(handle)
    }

    pub fn iter(&self) -> impl Iterator<Item = &TerminalRecord> {
        self.terminals.values()
    }

    /// The terminal called `name` in `worktree` (`None` for the group outside any checkout).
    pub fn by_name(&self, worktree: Option<&Path>, name: &str) -> Option<&TerminalRecord> {
        self.terminals.values().find(|record| {
            record.name == name
                && record
                    .worktree
                    .as_ref()
                    .map(|worktree| worktree.path.as_path())
                    == worktree
        })
    }

    /// Stores a record, new or changed, to be written out at the next `persist`.
    pub fn put(&mut self, record: TerminalRecord) {
        self.terminals.insert(record.handle.clone(), record);
        self.dirty = true;
    }

    pub fn remove(&mut self, handle: &Handle) -> Option<TerminalRecord> {
        let removed = self.terminals.remove(handle);
        self.dirty |= removed.is_some();
        removed
    }

    /// The worktrees with terminals, each with how many and how many want a person.
    pub fn worktrees(&self) -> Vec<WorktreeSummary> {
        let mut found: BTreeMap<&Path, WorktreeSummary> = BTreeMap::new();
        for record in self.terminals.values() {
            let Some(worktree) = &record.worktree else {
                continue;
            };
            let summary = found
                .entry(&worktree.path)
                .or_insert_with(|| WorktreeSummary {
                    worktree: worktree.clone(),
                    terminals: 0,
                    needs_input: 0,
                });
            summary.terminals += 1;
            summary.needs_input += u32::from(record.status == Status::NeedsInput);
        }
        found.into_values().collect()
    }

    /// The distinct worktrees, for resolving selectors against.
    pub fn known_worktrees(&self) -> Vec<WorktreeRef> {
        self.worktrees()
            .into_iter()
            .map(|summary| summary.worktree)
            .collect()
    }

    pub fn list(&self, worktree: Option<&Path>, status: Option<&[Status]>) -> Vec<TerminalRecord> {
        self.terminals
            .values()
            .filter(|record| {
                worktree.is_none_or(|path| {
                    record
                        .worktree
                        .as_ref()
                        .is_some_and(|worktree| worktree.path == path)
                })
            })
            .filter(|record| status.is_none_or(|wanted| wanted.contains(&record.status)))
            .cloned()
            .collect()
    }

    /// Writes the file whole, through a temporary beside it, when anything changed.
    pub fn persist(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let stored = Stored {
            format: FORMAT,
            terminals: self.terminals.values().cloned().collect(),
        };
        let text = serde_json::to_string_pretty(&stored).map_err(io::Error::other)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, &self.path)?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use colosso_term_control::{Actor, Size};

    use super::*;

    fn record(name: &str, worktree: &str) -> TerminalRecord {
        TerminalRecord {
            handle: Handle::generate(),
            worktree: Some(WorktreeRef {
                path: worktree.into(),
                repo_main: "/w/main".into(),
                branch: Some(name.into()),
            }),
            name: name.into(),
            title: None,
            cwd: worktree.into(),
            command: vec![],
            env: BTreeMap::new(),
            size: Size::default(),
            labels: BTreeSet::new(),
            created_ms: 1,
            created_by: Actor::system(),
            incarnation: 1,
            pid: Some(42),
            exit: None,
            status: Status::NeedsInput,
            status_reason: "bell".into(),
            last_exit: None,
            progress: None,
            control: Control::default(),
            attached_clients: 1,
            last_output_ms: 1,
        }
    }

    #[test]
    fn persists_and_reloads_as_ended_by_the_restart() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-termd/registry");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("registry.json");
        let mut registry = Registry::load(path.clone());
        let a = record("a", "/w/feat-a");
        let b = record("b", "/w/feat-a");
        registry.put(a.clone());
        registry.put(b);
        registry.put(record("c", "/w/feat-c"));
        assert_eq!(registry.worktrees().len(), 2);
        assert_eq!(registry.worktrees()[0].terminals, 2);
        assert_eq!(registry.list(Some(Path::new("/w/feat-c")), None).len(), 1);
        assert_eq!(
            registry
                .by_name(Some(Path::new("/w/feat-a")), "a")
                .map(|r| &r.handle),
            Some(&a.handle)
        );
        registry.persist().unwrap();

        let reloaded = Registry::load(path);
        assert_eq!(reloaded.iter().count(), 3);
        let a_again = reloaded.get(&a.handle).unwrap();
        assert_eq!(a_again.status, Status::Exited);
        assert_eq!(a_again.pid, None);
        assert!(a_again.exit.is_some_and(|exit| exit.daemon_restart));
        assert_eq!(a_again.attached_clients, 0);
    }
}
